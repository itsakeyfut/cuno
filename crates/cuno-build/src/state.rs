//! What each step read and ran with when it last succeeded, and whether it
//! must run again (`docs/en/specs/build.md`, "Deciding what changed").
//!
//! A step's record is a file beside its output, `<output>.cuno-state`:
//!
//! ```text
//! cuno-state <cuno version>
//! command <hash of what identifies the command>
//! <modified ns> <size> <content hash> <input path>
//! end <number of inputs>
//! ```
//!
//! The format is cuno's own. A record another cuno wrote, or one that
//! cannot be read, counts as none.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::graph::Step;

/// How long before a step's start an input's modification time still
/// counts as during the step. A file's time is stamped by a coarser clock
/// than the one a step's start is read from, so a file saved just after
/// the start can carry a time before it (`docs/en/specs/build.md`,
/// "Deciding what changed").
const SETTLE: Duration = Duration::from_secs(2);

/// The first line of every record.
const HEADER: &str = concat!("cuno-state ", env!("CARGO_PKG_VERSION"));

/// An input as it was: when it was modified, its size and its content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    modified: u128,
    size: u64,
    hash: u64,
}

/// A step's record.
struct Record {
    command: u64,
    inputs: Vec<(PathBuf, Stamp)>,
}

/// The records of one build: what besides a step's own arguments
/// identifies its command, and the content hashes already taken, since
/// many compiles read the same headers.
pub struct Records {
    identity: Vec<u8>,
    known: Mutex<HashMap<PathBuf, Stamp>>,
}

impl Records {
    /// Records for a build whose commands are also identified by
    /// `identity`: the compiler's version and the environment variables
    /// cuno builds that decide what it reads (`build.md`, "Deciding what
    /// changed").
    pub fn new<'a>(identity: impl IntoIterator<Item = &'a OsStr>) -> Records {
        let mut bytes = Vec::new();
        for part in identity {
            framed(&mut bytes, part.as_encoded_bytes());
        }
        Records {
            identity: bytes,
            known: Mutex::new(HashMap::new()),
        }
    }

    /// Whether `step` need not run: its output is there, its record says
    /// it last ran with this command, and every input it read is
    /// unchanged. An input whose time or size moved but whose content did
    /// not counts as unchanged, and its record is brought up to date.
    pub fn up_to_date(&self, step: &Step) -> bool {
        let Some(record) = read(&state_path(&step.output)) else {
            return false;
        };
        {
            // What was recorded is what is known until it is seen to differ.
            let mut known = self.known.lock().unwrap_or_else(|e| e.into_inner());
            for (path, stamp) in &record.inputs {
                known.entry(path.clone()).or_insert(*stamp);
            }
        }
        if record.command != self.command(step) || !step.output.is_file() {
            return false;
        }
        let mut moved = false;
        for (path, recorded) in &record.inputs {
            let Ok(now) = self.stamp(path) else {
                return false;
            };
            if now.hash != recorded.hash {
                return false;
            }
            moved |= now != *recorded;
        }
        if moved {
            let inputs: Vec<PathBuf> = record.inputs.into_iter().map(|(p, _)| p).collect();
            let inputs: Vec<(PathBuf, Duration)> =
                inputs.into_iter().map(|p| (p, Duration::ZERO)).collect();
            let _ = self.write(step, &inputs, None);
        }
        true
    }

    /// Removes `step`'s record before it runs, so a step that then fails
    /// has none and runs again next time.
    pub fn forget(&self, step: &Step) {
        let _ = fs::remove_file(state_path(&step.output));
    }

    /// Records that `step`, started at `started`, succeeded having read
    /// `inputs` and `built`, the outputs of the steps it waited for. The
    /// step is left unrecorded, and so runs again, when an input was
    /// modified two seconds or less before it started, or later, or one
    /// of `built` after it started (the compiler may have read either
    /// version), or a path cannot be written in the record: it is never
    /// judged on what it may not have read. `built` has no margin: this
    /// build finished writing them before the step started, so nobody's
    /// edit can race it.
    pub fn record(
        &self,
        step: &Step,
        inputs: &[PathBuf],
        built: &[PathBuf],
        started: SystemTime,
    ) -> io::Result<()> {
        let read: Vec<(PathBuf, Duration)> = inputs
            .iter()
            .map(|p| (p.clone(), SETTLE))
            .chain(built.iter().map(|p| (p.clone(), Duration::ZERO)))
            .collect();
        self.write(step, &read, Some(started))
    }

    /// Writes `step`'s record. When `started` is given, an input modified
    /// later than its margin before `started` is refused. The record is
    /// written beside and then renamed over the old one, so it is never
    /// seen half written.
    fn write(
        &self,
        step: &Step,
        inputs: &[(PathBuf, Duration)],
        started: Option<SystemTime>,
    ) -> io::Result<()> {
        let limit = |margin: Duration| {
            started.map(|t| {
                t.checked_sub(margin)
                    .unwrap_or(UNIX_EPOCH)
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |d| d.as_nanos())
            })
        };
        let mut text = format!("{HEADER}\ncommand {:016x}\n", self.command(step));
        let mut seen = std::collections::HashSet::new();
        for (path, margin) in inputs {
            if !seen.insert(path) {
                continue;
            }
            let stamp = self.stamp(path)?;
            if limit(*margin).is_some_and(|limit| stamp.modified >= limit) {
                return Err(io::Error::other("an input changed while the step ran"));
            }
            // A path that is not Unicode cannot be written. (A line break,
            // which no Windows path has, would leave the record unreadable,
            // and so the step would run again.)
            let Some(name) = path.to_str() else {
                return Err(io::Error::other("an input's path cannot be recorded"));
            };
            text.push_str(&format!(
                "{} {} {:016x} {name}\n",
                stamp.modified, stamp.size, stamp.hash
            ));
        }
        text.push_str(&format!("end {}\n", seen.len()));
        let path = state_path(&step.output);
        let mut partial = path.clone().into_os_string();
        partial.push(".partial");
        fs::write(&partial, text)?;
        fs::rename(&partial, &path)
    }

    /// The hash of everything that identifies `step`'s command: cuno's
    /// identity for this build, the program and every argument.
    fn command(&self, step: &Step) -> u64 {
        let mut bytes = self.identity.clone();
        framed(&mut bytes, step.program.as_os_str().as_encoded_bytes());
        for arg in &step.args {
            framed(&mut bytes, arg.as_encoded_bytes());
        }
        fnv(&bytes)
    }

    /// `path` as it is now. Its content is hashed only when its time or
    /// size differs from what is known of it.
    fn stamp(&self, path: &Path) -> io::Result<Stamp> {
        let meta = fs::metadata(path)?;
        let modified = meta
            .modified()?
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let size = meta.len();
        let known = self.known.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(stamp) = known.get(path)
            && stamp.modified == modified
            && stamp.size == size
        {
            return Ok(*stamp);
        }
        drop(known);
        let stamp = Stamp {
            modified,
            size,
            hash: fnv(&fs::read(path)?),
        };
        self.known
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(path.to_owned(), stamp);
        Ok(stamp)
    }
}

/// Where `output`'s record is kept.
fn state_path(output: &Path) -> PathBuf {
    let mut path = output.as_os_str().to_owned();
    path.push(".cuno-state");
    PathBuf::from(path)
}

/// The record at `path`, or `None` when there is none, another cuno wrote
/// it, or it cannot be read.
fn read(path: &Path) -> Option<Record> {
    let text = fs::read_to_string(path).ok()?;
    let mut lines: Vec<&str> = text.lines().collect();
    // A record cut short (cuno stopped while writing it) has no end line
    // giving its own length, and is not trusted on what it holds.
    let count: usize = lines.pop()?.strip_prefix("end ")?.parse().ok()?;
    let mut lines = lines.into_iter();
    if lines.next()? != HEADER {
        return None;
    }
    let command = u64::from_str_radix(lines.next()?.strip_prefix("command ")?, 16).ok()?;
    let inputs = lines
        .map(|line| {
            let mut parts = line.splitn(4, ' ');
            let modified = parts.next()?.parse().ok()?;
            let size = parts.next()?.parse().ok()?;
            let hash = u64::from_str_radix(parts.next()?, 16).ok()?;
            let path = PathBuf::from(parts.next()?);
            Some((
                path,
                Stamp {
                    modified,
                    size,
                    hash,
                },
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    (inputs.len() == count).then_some(Record { command, inputs })
}

/// Appends `part` with its length first, so no two lists of parts make the
/// same bytes.
fn framed(bytes: &mut Vec<u8>, part: &[u8]) {
    bytes.extend_from_slice(&(part.len() as u64).to_le_bytes());
    bytes.extend_from_slice(part);
}

/// 64-bit FNV-1a: fixed, so a hash recorded by one run means the same to
/// the next, which the standard library's hasher does not promise.
fn fnv(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests;
