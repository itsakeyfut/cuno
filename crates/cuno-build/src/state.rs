//! What each step read and ran with when it last succeeded, and whether it
//! must run again (`docs/en/specs/build.md`, "Deciding what changed").
//!
//! A step's record is a file beside its output, `<output>.cuno-state`:
//!
//! ```text
//! cuno-state <cuno version>
//! command <hash of what identifies the command>
//! <modified ns> <size> <content hash> <input path>
//! present <path of a same-named file the search could also find>
//! end <number of inputs>
//! ```
//!
//! The format is cuno's own. A record another cuno wrote, or one that
//! cannot be read, counts as none.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cuno_core::NameCase;
use cuno_core::paths::Place;

use crate::graph::{Step, StepKind};

/// How long before a step's start an input's modification time still
/// counts as during the step. A file's time is stamped by a coarser clock
/// than the one a step's start is read from, so a file saved just after
/// the start can carry a time before it (`docs/en/specs/build.md`,
/// "Deciding what changed").
const SETTLE: Duration = Duration::from_secs(2);

/// Why a step was left unrecorded, and so runs again next time.
#[derive(Debug)]
pub enum NotRecorded {
    /// Inputs, or same-named files its search could find, dated later than
    /// when the step was recorded: in the future, so nothing tells whether
    /// they changed while the step ran (`docs/en/specs/build.md`, "Deciding
    /// what changed"). Every one, so each can be named.
    DatedAhead(Vec<PathBuf>),
    /// Anything else: an input that changed while the step ran, one that
    /// cannot be read or written in the record, or a failed write.
    Other(io::Error),
}

impl From<io::Error> for NotRecorded {
    fn from(e: io::Error) -> NotRecorded {
        NotRecorded::Other(e)
    }
}

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
    /// The same-named files its search could also have found that were
    /// there when it was recorded: see [`Records::found_elsewhere`].
    present: BTreeSet<PathBuf>,
}

/// The records of one build: what besides a step's own arguments
/// identifies its command, and the content hashes already taken, since
/// many compiles read the same headers.
pub struct Records {
    identity: Vec<u8>,
    known: Mutex<HashMap<PathBuf, Stamp>>,
    /// The compiler's own include directories (`INCLUDE`), where a header
    /// found is named relative to its directory but no file is looked for.
    system: Vec<Place>,
    /// How the host compares names.
    case: NameCase,
    /// Where each path leads, worked out once for the build.
    places: Mutex<HashMap<PathBuf, Place>>,
    /// The names in each directory looked in, folded as `case` compares
    /// them, read once for the build.
    listings: Mutex<HashMap<PathBuf, HashSet<OsString>>>,
    /// For each folded name, the directories listed so far that hold it.
    holders: Mutex<HashMap<OsString, Vec<PathBuf>>>,
}

impl Records {
    /// Records for a build whose commands are also identified by
    /// `identity`: the compiler's version and the environment variables
    /// cuno builds that decide what it reads (`build.md`, "Deciding what
    /// changed"). `system` is the compiler's own include directories, and
    /// `case` how the host compares names.
    pub fn new<'a>(
        identity: impl IntoIterator<Item = &'a OsStr>,
        system: &[PathBuf],
        case: NameCase,
    ) -> Records {
        let mut bytes = Vec::new();
        for part in identity {
            framed(&mut bytes, part.as_encoded_bytes());
        }
        Records {
            identity: bytes,
            known: Mutex::new(HashMap::new()),
            system: system.iter().map(|dir| Place::of(dir, case)).collect(),
            case,
            places: Mutex::new(HashMap::new()),
            listings: Mutex::new(HashMap::new()),
            holders: Mutex::new(HashMap::new()),
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
        let read: Vec<PathBuf> = record.inputs.iter().map(|(p, _)| p.clone()).collect();
        if !self.found_elsewhere(step, &read).is_subset(&record.present) {
            return false;
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
    ) -> Result<(), NotRecorded> {
        let read: Vec<(PathBuf, Duration)> = inputs
            .iter()
            .map(|p| (p.clone(), SETTLE))
            .chain(built.iter().map(|p| (p.clone(), Duration::ZERO)))
            .collect();
        self.write(step, &read, Some(started))
    }

    /// Writes `step`'s record. When `started` is given, an input modified
    /// later than its margin before `started` is refused, as are the
    /// same-named files its search could find; when any of them is dated
    /// after now, as [`NotRecorded::DatedAhead`] naming all such. The
    /// record is written beside and then renamed over the old one, so it is
    /// never seen half written.
    fn write(
        &self,
        step: &Step,
        inputs: &[(PathBuf, Duration)],
        started: Option<SystemTime>,
    ) -> Result<(), NotRecorded> {
        let limit = |margin: Duration| {
            started.map(|t| {
                t.checked_sub(margin)
                    .unwrap_or(UNIX_EPOCH)
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |d| d.as_nanos())
            })
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        // Every refused file is looked at, so all those dated ahead are
        // named at once rather than one a build.
        let mut ahead = Vec::new();
        let mut changed = None;
        let mut refuse = |path: &Path, modified: Option<u128>, why: &'static str| match modified {
            Some(modified) if modified > now => ahead.push(path.to_owned()),
            _ => changed = Some(why),
        };
        let mut text = format!("{HEADER}\ncommand {:016x}\n", self.command(step));
        let mut seen = std::collections::HashSet::new();
        for (path, margin) in inputs {
            if !seen.insert(path) {
                continue;
            }
            let stamp = self.stamp(path)?;
            if limit(*margin).is_some_and(|limit| stamp.modified >= limit) {
                refuse(
                    path,
                    Some(stamp.modified),
                    "an input changed while the step ran",
                );
                continue;
            }
            // A path that is not Unicode cannot be written. (A line break,
            // which no Windows path has, would leave the record unreadable,
            // and so the step would run again.)
            let Some(name) = path.to_str() else {
                return Err(io::Error::other("an input's path cannot be recorded").into());
            };
            text.push_str(&format!(
                "{} {} {:016x} {name}\n",
                stamp.modified, stamp.size, stamp.hash
            ));
        }
        let read: Vec<PathBuf> = inputs.iter().map(|(p, _)| p.clone()).collect();
        for path in self.found_elsewhere(step, &read) {
            // One that appeared while the step ran, or just before, may lie
            // ahead of what was read without the compiler having seen it.
            // A time that cannot be read is taken as just now.
            let modified = fs::metadata(&path)
                .and_then(|meta| meta.modified())
                .ok()
                .map(|t| t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
            if limit(SETTLE).is_some_and(|limit| modified.is_none_or(|m| m >= limit)) {
                refuse(
                    &path,
                    modified,
                    "a same-named file appeared as the step ran",
                );
                continue;
            }
            let Some(name) = path.to_str() else {
                return Err(io::Error::other("a path cannot be recorded").into());
            };
            text.push_str(&format!("present {name}\n"));
        }
        if !ahead.is_empty() {
            return Err(NotRecorded::DatedAhead(ahead));
        }
        if let Some(why) = changed {
            return Err(io::Error::other(why).into());
        }
        text.push_str(&format!("end {}\n", seen.len()));
        let path = state_path(&step.output);
        let mut partial = path.clone().into_os_string();
        partial.push(".partial");
        fs::write(&partial, text)?;
        fs::rename(&partial, &path)?;
        Ok(())
    }

    /// The hash of everything that identifies `step`'s command: cuno's
    /// identity for this build, the program and every argument.
    fn command(&self, step: &Step) -> u64 {
        let mut bytes = self.identity.clone();
        framed(&mut bytes, step.program.as_os_str().as_encoded_bytes());
        for arg in step.run_args() {
            framed(&mut bytes, arg.as_encoded_bytes());
        }
        fnv(&bytes)
    }

    /// The files `step`'s search could find in place of one it read, that
    /// exist now: for each file read, the same relative name in each place
    /// the step searches (`build.md`, "Deciding what changed"). The search
    /// order is not reproduced: one already there when the step was
    /// recorded lies later in it, so only one that appears since counts.
    ///
    /// A compile looks for what `read` holds (its source and headers) in
    /// its include directories and beside each file read, except in the
    /// compiler's own directories; a link looks for every library it
    /// links, found or not, in `lib-dirs`.
    fn found_elsewhere(&self, step: &Step, read: &[PathBuf]) -> BTreeSet<PathBuf> {
        let mut found = BTreeSet::new();
        // A match, so a new kind has to say how its search is checked
        // (`architecture.md`, "Crate layout").
        match step.kind {
            StepKind::Link { .. } => {
                for dir in &step.searched {
                    for name in &step.looked_for {
                        let candidate = dir.join(name);
                        if self.listed(dir, name) && !step.reads.contains(&candidate) {
                            found.insert(candidate);
                        }
                    }
                }
                return found;
            }
            StepKind::Compile { .. } | StepKind::Check { .. } => {}
        }
        // The compiler's own directories: `INCLUDE`, and where it is
        // installed (Clang's own headers are under it).
        let installed = step
            .program
            .parent()
            .and_then(Path::parent)
            .map(|dir| self.place(dir));
        let own = |place: &Place| {
            self.system.iter().any(|s| place.within(s))
                || installed.as_ref().is_some_and(|i| place.within(i))
        };
        let mut places: Vec<(PathBuf, Place)> = Vec::new();
        let beside = read.iter().filter_map(|file| file.parent());
        for dir in step.searched.iter().map(PathBuf::as_path).chain(beside) {
            let place = self.place(dir);
            if !own(&place) && !places.iter().any(|(_, p)| p.same(&place)) {
                places.push((dir.to_owned(), place));
            }
        }
        let searched: Vec<Place> = step.searched.iter().map(|dir| self.place(dir)).collect();
        let in_step: HashSet<PathBuf> = places.iter().map(|(dir, _)| dir.clone()).collect();
        for dir in &in_step {
            self.load(dir);
        }
        for file in read {
            let file_place = self.place(file);
            // Named from the directories searched by name: one found beside
            // its includer was found in the first place searched, so nothing
            // lies ahead of it there.
            let mut names: BTreeSet<PathBuf> = searched
                .iter()
                .chain(&self.system)
                .filter_map(|base| file_place.relative(base))
                .filter(|rel| rel.file_name().is_some())
                .collect();
            // One of the compiler's own outside `INCLUDE`, such as a header
            // that ships with Clang, is searched for by its file name.
            if names.is_empty() && own(&file_place) {
                names.extend(file.file_name().map(PathBuf::from));
            }
            for name in &names {
                // Only the places whose listing holds the name's first
                // part can hold it, so the rest are never asked.
                let Some(first) = name.components().next() else {
                    continue;
                };
                for dir in self.holding(first.as_os_str(), &in_step) {
                    if self.listed(&dir, name) {
                        let candidate = dir.join(name);
                        if !self.place(&candidate).same(&file_place) {
                            found.insert(candidate);
                        }
                    }
                }
            }
        }
        found
    }

    /// Where `path` leads, worked out once for the build.
    fn place(&self, path: &Path) -> Place {
        let mut places = self.places.lock().unwrap_or_else(|e| e.into_inner());
        places
            .entry(path.to_owned())
            .or_insert_with(|| Place::of(path, self.case))
            .clone()
    }

    /// Whether `dir` holds `name`, which may name a file in a directory
    /// under it, judged from each directory's listing, read once for the
    /// build: one listing answers every name looked for there, where a
    /// lookup per name made a no-op build of a project with many header
    /// directories more than ten times slower.
    fn listed(&self, dir: &Path, name: &Path) -> bool {
        let mut here = dir.to_owned();
        for part in name.components() {
            let part = self.case.fold(part.as_os_str());
            self.load(&here);
            let listings = self.listings.lock().unwrap_or_else(|e| e.into_inner());
            if !listings
                .get(&here)
                .is_some_and(|names| names.contains(&part))
            {
                return false;
            }
            here.push(&part);
        }
        true
    }

    /// Reads the names in `dir`, folded as the host compares them, once
    /// for the build, and notes `dir` under each name it holds.
    fn load(&self, dir: &Path) {
        let mut listings = self.listings.lock().unwrap_or_else(|e| e.into_inner());
        if listings.contains_key(dir) {
            return;
        }
        let listing: HashSet<OsString> = fs::read_dir(dir)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|entry| self.case.fold(&entry.file_name()))
                    .collect()
            })
            .unwrap_or_default();
        let mut holders = self.holders.lock().unwrap_or_else(|e| e.into_inner());
        for name in &listing {
            holders
                .entry(name.clone())
                .or_default()
                .push(dir.to_owned());
        }
        listings.insert(dir.to_owned(), listing);
    }

    /// The directories among `among`, all already listed, whose listing
    /// holds `name`.
    fn holding(&self, name: &OsStr, among: &HashSet<PathBuf>) -> Vec<PathBuf> {
        let holders = self.holders.lock().unwrap_or_else(|e| e.into_inner());
        holders
            .get(&self.case.fold(name))
            .map(|dirs| {
                dirs.iter()
                    .filter(|d| among.contains(*d))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
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
    let mut present = BTreeSet::new();
    let inputs = lines
        .filter(|line| match line.strip_prefix("present ") {
            Some(path) => {
                present.insert(PathBuf::from(path));
                false
            }
            None => true,
        })
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
    (inputs.len() == count).then_some(Record {
        command,
        inputs,
        present,
    })
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
mod tests {
    use super::*;
    use crate::graph::StepKind;
    use std::ffi::OsString;
    use std::time::{Duration, SystemTime};

    struct Case {
        _root: tempfile::TempDir,
        step: Step,
        input: PathBuf,
    }

    /// A compile of `a.cpp` into `a.obj` that has run and been recorded.
    fn recorded() -> (Case, Records) {
        let root = tempfile::tempdir().unwrap();
        let input = root.path().join("a.cpp");
        fs::write(&input, "int a;").unwrap();
        set_time(&input, 1_000);
        let output = root.path().join("a.obj");
        fs::write(&output, "obj").unwrap();
        let step = Step {
            kind: StepKind::Compile {
                source: input.clone(),
            },
            program: "cl.exe".into(),
            args: vec![OsString::from("/c")],
            output,
            after: Vec::new(),
            echo: None,
            headers: None,
            reads: Vec::new(),
            searched: Vec::new(),
            looked_for: Vec::new(),
            color: Vec::new(),
            dependency: Vec::new(),
        };
        let records = fresh();
        records
            .record(&step, std::slice::from_ref(&input), &[], later())
            .unwrap();
        (
            Case {
                _root: root,
                step,
                input,
            },
            records,
        )
    }

    /// Another build's records, so nothing is known from this one.
    fn fresh() -> Records {
        Records::new(
            [OsStr::new("19.52"), OsStr::new("C:/inc")],
            &[],
            NameCase::Insensitive,
        )
    }

    /// A start time after every file a test writes.
    fn later() -> SystemTime {
        SystemTime::now() + Duration::from_secs(60)
    }

    fn set_time(path: &Path, seconds: u64) {
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(seconds))
            .unwrap();
    }

    /// Nothing changed: the step need not run.
    ///
    /// Mutation: always run; this test fails.
    #[test]
    fn an_unchanged_step_is_up_to_date() {
        let (case, _) = recorded();
        assert!(fresh().up_to_date(&case.step));
    }

    /// A new time with the same bytes is no change, and the record takes the
    /// new time, so the bytes are not read again next time.
    ///
    /// Mutation: judge by time alone, or leave the record as it was; this test
    /// fails.
    #[test]
    fn a_new_time_with_the_same_bytes_is_no_change() {
        let (case, _) = recorded();
        set_time(&case.input, 2_000);
        assert!(fresh().up_to_date(&case.step));
        let record = read(&state_path(&case.step.output)).unwrap();
        assert_eq!(
            record.inputs[0].1.modified,
            Duration::from_secs(2_000).as_nanos()
        );
    }

    /// A file put back with an older time and other bytes of the same size,
    /// as a restore from a backup does, is a change: the record is compared,
    /// not the output's time.
    ///
    /// Mutation: judge by "newer than the output", or skip the hash; this test
    /// fails.
    #[test]
    fn an_older_file_with_other_bytes_is_a_change() {
        let (case, _) = recorded();
        fs::write(&case.input, "int b;").unwrap();
        set_time(&case.input, 500);
        assert!(!fresh().up_to_date(&case.step));
    }

    /// A different size is a change.
    ///
    /// Mutation: compare only times; this test fails.
    #[test]
    fn a_different_size_is_a_change() {
        let (case, _) = recorded();
        fs::write(&case.input, "int a; int b;").unwrap();
        set_time(&case.input, 1_000);
        assert!(!fresh().up_to_date(&case.step));
    }

    /// Another argument, another compiler version or another environment, or
    /// another cuno's record, means the step runs.
    ///
    /// Mutation: ignore the arguments, the identity or the header line; this
    /// test fails.
    #[test]
    fn another_command_or_cuno_is_a_change() {
        let (case, _) = recorded();
        let mut other = case.step.clone();
        other.args.push("/O2".into());
        assert!(!fresh().up_to_date(&other));
        let newer = Records::new(
            [OsStr::new("19.53"), OsStr::new("C:/inc")],
            &[],
            NameCase::Insensitive,
        );
        assert!(!newer.up_to_date(&case.step));
        let sdk = Records::new(
            [OsStr::new("19.52"), OsStr::new("C:/other")],
            &[],
            NameCase::Insensitive,
        );
        assert!(!sdk.up_to_date(&case.step));

        let path = state_path(&case.step.output);
        let text = fs::read_to_string(&path).unwrap();
        fs::write(&path, text.replacen(HEADER, "cuno-state another", 1)).unwrap();
        assert!(!fresh().up_to_date(&case.step));
    }

    /// A missing output or input, a record forgotten before a run, or no
    /// record at all, means the step runs.
    ///
    /// Mutation: trust the record without the output, or keep a record across
    /// a run that then fails; this test fails.
    #[test]
    fn what_is_missing_is_a_change() {
        let (case, _) = recorded();
        fs::remove_file(&case.step.output).unwrap();
        assert!(!fresh().up_to_date(&case.step));

        let (case, records) = recorded();
        records.forget(&case.step);
        assert!(!fresh().up_to_date(&case.step));

        let (case, _) = recorded();
        fs::remove_file(&case.input).unwrap();
        assert!(!fresh().up_to_date(&case.step));
    }

    /// A path that names inputs with a space, and one recorded twice, read
    /// back as they were written.
    ///
    /// Mutation: split the path at a space, or record a duplicate; this test
    /// fails.
    #[test]
    fn paths_with_spaces_are_recorded_once() {
        let (case, records) = recorded();
        let spaced = case.input.with_file_name("inc dir.h");
        fs::write(&spaced, "#pragma once").unwrap();
        records
            .record(
                &case.step,
                &[case.input.clone(), spaced.clone(), spaced.clone()],
                &[],
                later(),
            )
            .unwrap();
        let record = read(&state_path(&case.step.output)).unwrap();
        let paths: Vec<&PathBuf> = record.inputs.iter().map(|(p, _)| p).collect();
        assert_eq!(paths, [&case.input, &spaced]);
        assert!(fresh().up_to_date(&case.step));
    }

    /// An input modified after the step started may or may not be what the
    /// compiler read, so the step is left unrecorded and runs again.
    ///
    /// Mutation: record whatever the inputs are when the step ends; this test
    /// fails.
    #[test]
    fn an_input_changed_while_the_step_ran_is_not_recorded() {
        let (case, records) = recorded();
        records.forget(&case.step);
        let started = SystemTime::now() - Duration::from_secs(60);
        fs::write(&case.input, "int edited;").unwrap();
        assert!(
            records
                .record(&case.step, std::slice::from_ref(&case.input), &[], started)
                .is_err()
        );
        assert!(!state_path(&case.step.output).exists());
        assert!(!fresh().up_to_date(&case.step));
    }

    /// An input whose time is at the step's start, or within two seconds
    /// before it, may have been saved after the start under a coarser clock,
    /// so the step is left unrecorded; one older than that is recorded.
    ///
    /// Mutation: refuse only an input later than the start, drop the margin,
    /// widen it past two seconds, or refuse every input; this test fails.
    #[test]
    fn an_input_saved_just_before_the_step_is_not_recorded() {
        let (case, records) = recorded();
        let started = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000);
        let at = |time: SystemTime| {
            fs::File::options()
                .write(true)
                .open(&case.input)
                .unwrap()
                .set_modified(time)
                .unwrap();
            records
                .record(&case.step, std::slice::from_ref(&case.input), &[], started)
                .is_ok()
        };
        assert!(!at(started), "at the start");
        assert!(!at(started - Duration::from_secs(1)), "1 s before");
        assert!(!at(started - Duration::from_millis(1_999)), "just within");
        assert!(
            !at(started - Duration::from_secs(2)),
            "exactly two seconds before"
        );
        assert!(at(started - Duration::from_millis(2_001)), "just outside");
        assert!(at(started - Duration::from_secs(3)), "3 s before");
    }

    /// What this build made before the step started (a link's objects) has
    /// no margin: written moments before, it is recorded, while the same
    /// time on one of the step's own inputs is refused.
    ///
    /// Mutation: give the outputs of earlier steps the margin, none to the
    /// step's own inputs, or accept an output stamped at the start itself;
    /// this test fails.
    #[test]
    fn what_the_build_made_before_the_step_has_no_margin() {
        let (case, records) = recorded();
        let started = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000);
        fs::File::options()
            .write(true)
            .open(&case.input)
            .unwrap()
            .set_modified(started - Duration::from_millis(500))
            .unwrap();
        let input = std::slice::from_ref(&case.input);
        assert!(records.record(&case.step, &[], input, started).is_ok());
        assert!(records.record(&case.step, input, &[], started).is_err());
        // Written at the start itself, it may be after the start: refused.
        fs::File::options()
            .write(true)
            .open(&case.input)
            .unwrap()
            .set_modified(started)
            .unwrap();
        assert!(records.record(&case.step, &[], input, started).is_err());
    }

    /// A record cut short, as when cuno is stopped while writing it, is not
    /// trusted on the inputs it still holds.
    ///
    /// Mutation: drop the end line, or accept a record whose count differs;
    /// this test fails.
    #[test]
    fn a_record_cut_short_is_none() {
        let (case, records) = recorded();
        let header = case.input.with_file_name("a.h");
        fs::write(&header, "#define A 1").unwrap();
        records
            .record(
                &case.step,
                &[case.input.clone(), header.clone()],
                &[],
                later(),
            )
            .unwrap();
        let path = state_path(&case.step.output);
        let text = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.last(), Some(&"end 2"));
        fs::write(&path, format!("{}\n", lines[..3].join("\n"))).unwrap();
        assert!(!fresh().up_to_date(&case.step));
        fs::write(&path, format!("{}\nend 2\n", lines[..3].join("\n"))).unwrap();
        assert!(!fresh().up_to_date(&case.step));
    }

    /// Another program with the same arguments is another command, and the
    /// parts of a command are kept apart: `/Da` then `b` is not `/Dab`.
    ///
    /// Mutation: leave the program out of the command, or join the parts
    /// without their lengths; this test fails.
    #[test]
    fn the_program_and_each_part_identify_the_command() {
        let (case, _) = recorded();
        let mut other = case.step.clone();
        other.program = "C:/other/cl.exe".into();
        assert!(!fresh().up_to_date(&other));

        let split = Step {
            args: vec!["/Da".into(), "b".into()],
            ..case.step.clone()
        };
        let joined = Step {
            args: vec!["/Dab".into()],
            ..case.step.clone()
        };
        assert_ne!(fresh().command(&split), fresh().command(&joined));
        let a = Records::new(
            [OsStr::new("ab"), OsStr::new("c")],
            &[],
            NameCase::Insensitive,
        );
        let b = Records::new(
            [OsStr::new("a"), OsStr::new("bc")],
            &[],
            NameCase::Insensitive,
        );
        assert_ne!(a.command(&case.step), b.command(&case.step));
    }

    /// An input whose time and size match the record is not read again: a
    /// change that keeps both is the one miss the spec accepts, and it shows
    /// the bytes are not hashed on every build.
    ///
    /// Mutation: hash every input every time, or forget what the record says
    /// of it; this test fails.
    #[test]
    fn a_matching_time_and_size_is_not_read_again() {
        let (case, _) = recorded();
        fs::write(&case.input, "int b;").unwrap();
        set_time(&case.input, 1_000);
        assert!(fresh().up_to_date(&case.step));
    }

    /// A step over `files`, written and dated long ago, that searches
    /// `searched`, recorded by `records`.
    fn searching(kind: StepKind, root: &Path, files: &[&str], searched: &[&str]) -> Step {
        for file in files {
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, file).unwrap();
            set_time(&path, 1_000);
        }
        for dir in searched {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        let output = root.join("out");
        fs::write(&output, "out").unwrap();
        Step {
            kind,
            program: "cl.exe".into(),
            args: Vec::new(),
            output,
            after: Vec::new(),
            echo: None,
            headers: None,
            reads: Vec::new(),
            searched: searched.iter().map(|dir| root.join(dir)).collect(),
            looked_for: Vec::new(),
            color: Vec::new(),
            dependency: Vec::new(),
        }
    }

    /// A library created in an earlier `lib-dirs` directory than the one
    /// the link read makes the link run again, and so does one created in
    /// `lib-dirs` for a library the link took from the system; an object
    /// it waited for is not looked for there.
    ///
    /// Mutation: leave links out of the search for same-named files, look
    /// only for the libraries found in `lib-dirs`, or look for what the
    /// link read rather than for its libraries; this test fails.
    #[test]
    fn a_library_created_ahead_in_lib_dirs_relinks() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        let link = StepKind::Link { bin: "app".into() };
        let mut step = searching(
            link,
            root,
            &["second/x.lib", "obj/a.obj"],
            &["first", "second"],
        );
        step.reads = vec![root.join("second/x.lib")];
        step.looked_for = vec!["x.lib".into(), "version.lib".into()];
        let read = [root.join("second/x.lib"), root.join("obj/a.obj")];
        fresh().record(&step, &read, &[], later()).unwrap();
        fs::write(root.join("first/a.obj"), "unrelated").unwrap();
        assert!(fresh().up_to_date(&step));
        fs::write(root.join("first/x.lib"), "new").unwrap();
        assert!(!fresh().up_to_date(&step));
        fs::remove_file(root.join("first/x.lib")).unwrap();
        assert!(fresh().up_to_date(&step));
        fs::write(root.join("second/version.lib"), "shadow").unwrap();
        assert!(!fresh().up_to_date(&step));
    }

    /// A header created beside a file the compile read, in a directory it
    /// does not search by name, can be found first by an `#include "..."`
    /// there, so the step runs again; nothing is looked for in the
    /// compiler's own directories, and a header that ships with the
    /// compiler is looked for by its file name. A check is judged the same
    /// way as a compile.
    ///
    /// Mutation: leave the directories of the files read out of the places
    /// looked in, look in the compiler's own directories, drop the file
    /// name for a header of the compiler's own, or judge a check without
    /// looking; this test fails.
    #[test]
    fn a_header_is_looked_for_beside_what_read_it_but_not_in_the_compiler() {
        for kind in ["compile", "check"] {
            let root = tempfile::tempdir().unwrap();
            let root = root.path();
            let source = root.join("src/a.cpp");
            let step_kind = match kind {
                "compile" => StepKind::Compile { source },
                _ => StepKind::Check { source },
            };
            let mut step = searching(
                step_kind,
                root,
                &["src/a.cpp", "inc/x.h", "lib/y.h", "llvm/lib/clang/stddef.h"],
                &["inc"],
            );
            step.program = root.join("llvm/bin/clang.exe");
            let read = [
                root.join("src/a.cpp"),
                root.join("inc/x.h"),
                root.join("lib/y.h"),
                root.join("llvm/lib/clang/stddef.h"),
            ];
            fresh().record(&step, &read, &[], later()).unwrap();
            assert!(fresh().up_to_date(&step), "{kind}");
            fs::write(root.join("llvm/lib/clang/x.h"), "own").unwrap();
            assert!(fresh().up_to_date(&step), "{kind}");
            fs::write(root.join("lib/x.h"), "beside").unwrap();
            assert!(!fresh().up_to_date(&step), "{kind}");
            fs::remove_file(root.join("lib/x.h")).unwrap();
            fs::write(root.join("inc/stddef.h"), "shadow").unwrap();
            assert!(!fresh().up_to_date(&step), "{kind}");
        }
    }

    /// What was present when a step was recorded is kept in its record as
    /// `present` lines before the end line, which counts only the inputs,
    /// and read back as present.
    ///
    /// Mutation: write no `present` line, read none back, or count them in
    /// the end line; this test fails.
    #[test]
    fn a_record_keeps_what_was_present() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        let source = root.join("src/a.cpp");
        let step = searching(
            StepKind::Compile { source },
            root,
            &["src/a.cpp", "a/cfg.h", "b/cfg.h"],
            &["a", "b"],
        );
        let read = [root.join("src/a.cpp"), root.join("a/cfg.h")];
        fresh().record(&step, &read, &[], later()).unwrap();
        let text = fs::read_to_string(state_path(&step.output)).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines[lines.len() - 2],
            format!("present {}", root.join("b").join("cfg.h").display())
        );
        assert_eq!(lines[lines.len() - 1], "end 2");
        let record = super::read(&state_path(&step.output)).unwrap();
        assert_eq!(
            record.present,
            BTreeSet::from([root.join("b").join("cfg.h")])
        );
    }

    /// A same-named file saved after the step started is not taken as one
    /// that was there before: the step is left unrecorded, as for an input.
    ///
    /// Mutation: record whatever is found when the step ends; this test
    /// fails.
    #[test]
    fn a_header_created_while_the_step_ran_is_not_taken_as_there_before() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        let source = root.join("src/a.cpp");
        let step = searching(
            StepKind::Compile { source },
            root,
            &["src/a.cpp", "second/x.h"],
            &["first", "second"],
        );
        let read = [root.join("src/a.cpp"), root.join("second/x.h")];
        let started = SystemTime::now();
        fs::write(root.join("first/x.h"), "new").unwrap();
        assert!(fresh().record(&step, &read, &[], started).is_err());
    }

    /// A header found in the compiler's own directory is looked for by its
    /// path from that directory (`sys/types.h`), not its file name alone.
    ///
    /// Mutation: name a header found in a system directory by its file
    /// name only; this test fails.
    #[test]
    fn a_system_header_is_named_from_its_directory() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        let compile = StepKind::Compile {
            source: root.join("src/a.cpp"),
        };
        let step = searching(compile, root, &["src/a.cpp", "sys/sys/types.h"], &["inc"]);
        let records = || {
            Records::new(
                [OsStr::new("19.52")],
                &[root.join("sys")],
                NameCase::Insensitive,
            )
        };
        let read = [root.join("src/a.cpp"), root.join("sys/sys/types.h")];
        records().record(&step, &read, &[], later()).unwrap();
        assert!(records().up_to_date(&step));
        fs::write(root.join("inc/types.h"), "other").unwrap();
        assert!(records().up_to_date(&step));
        fs::create_dir_all(root.join("inc/sys")).unwrap();
        fs::write(root.join("inc/sys/types.h"), "shadow").unwrap();
        assert!(!records().up_to_date(&step));
    }
}
