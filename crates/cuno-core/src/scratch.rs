//! A directory for one run only, shared with nothing else and removed when
//! the run is done (`docs/en/specs/architecture.md`, "What goes in
//! `cuno-core`").

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use crate::{CunoError, ExitClass};

/// How many names are tried before giving up: far more than a run that
/// does not leak directories ever meets.
const TRIES: u32 = 100;

/// A directory of this run's own, removed when dropped.
#[derive(Debug)]
pub struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    /// Makes `parent/cuno-<command>-<process id>-<n>`, the first `n` whose
    /// name is not taken. A name that exists, whoever made it, is passed
    /// over: a killed run leaves its directory behind and its process id is
    /// reused. `parent` is not created: a missing parent is the caller's
    /// problem to report. The error names the directory that could not be
    /// made.
    pub fn new(parent: &Path, command: &str) -> Result<Scratch, (PathBuf, io::Error)> {
        // The process id keeps two cuno processes apart. It is the
        // process's identity, not its environment, so a library may ask.
        let id = std::process::id();
        for n in 0..TRIES {
            let dir = parent.join(format!("cuno-{command}-{id}-{n}"));
            match std::fs::create_dir(&dir) {
                Ok(()) => return Ok(Scratch { dir }),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err((dir, e)),
            }
        }
        Err((
            parent.to_owned(),
            io::Error::other("every scratch name is taken"),
        ))
    }

    /// The directory.
    pub fn path(&self) -> &Path {
        &self.dir
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // Left behind only if it cannot be removed; nothing else uses it.
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A scratch directory of `command`'s, for `purpose` (`check clang`,
/// `format`), in `temp`, the temporary directory.
pub fn in_temp(temp: &Path, command: &str, purpose: &str) -> Result<Scratch, TempError> {
    Scratch::new(temp, command).map_err(|(dir, e)| TempError {
        dir,
        purpose: purpose.to_owned(),
        reason: e.to_string(),
    })
}

/// The temporary directory cannot hold a scratch directory: `TEMP` names one
/// that does not exist or cannot be written. Every command says it alike.
#[derive(Debug)]
pub struct TempError {
    /// The directory that could not be made.
    pub dir: PathBuf,
    /// What it was for (`check clang`, `format`).
    pub purpose: String,
    /// Why, as the system said.
    pub reason: String,
}

impl fmt::Display for TempError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "cannot create {} to {}: {}",
            self.dir.display(),
            self.purpose,
            self.reason
        )
    }
}

impl std::error::Error for TempError {}

impl CunoError for TempError {
    fn exit_class(&self) -> ExitClass {
        ExitClass::Setup
    }

    fn help(&self) -> Option<String> {
        Some("set TEMP to a directory that exists and can be written".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The directory is made under the parent given, named for the command
    /// and this process, and is gone once dropped.
    ///
    /// Mutation: leave the process id out of the name, or skip the removal;
    /// this test fails.
    #[test]
    fn a_scratch_directory_is_made_and_removed() {
        let parent = tempfile::tempdir().unwrap();
        let scratch = Scratch::new(parent.path(), "fmt").unwrap();
        let dir = scratch.path().to_owned();
        assert!(dir.is_dir());
        assert_eq!(dir.parent(), Some(parent.path()));
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(name, format!("cuno-fmt-{}-0", std::process::id()));
        std::fs::write(dir.join("file"), "").unwrap();
        drop(scratch);
        assert!(!dir.exists(), "the scratch directory was left behind");
    }

    /// A name a killed run left behind is passed over and left alone.
    ///
    /// Mutation: fail on a name already taken, or take it over; this test
    /// fails.
    #[test]
    fn a_taken_name_is_passed_over() {
        let parent = tempfile::tempdir().unwrap();
        let left = parent
            .path()
            .join(format!("cuno-lint-{}-0", std::process::id()));
        std::fs::create_dir(&left).unwrap();
        std::fs::write(left.join("theirs"), "").unwrap();
        let scratch = Scratch::new(parent.path(), "lint").unwrap();
        assert_ne!(scratch.path(), left);
        drop(scratch);
        assert!(left.join("theirs").is_file());
    }

    /// A missing temporary directory is a setup problem that says what the
    /// directory was for and how to fix it, and is not created.
    ///
    /// Mutation: create the parent with `create_dir_all`, or drop the help;
    /// this test fails.
    #[test]
    fn a_missing_temporary_directory_is_a_setup_problem() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("missing");
        let err = in_temp(&missing, "fmt", "format").unwrap_err();
        assert_eq!(err.exit_class(), ExitClass::Setup);
        let shown = err.to_string();
        let start = format!(
            "cannot create {} to format: ",
            missing
                .join(format!("cuno-fmt-{}-0", std::process::id()))
                .display()
        );
        assert!(shown.starts_with(&start), "{shown}");
        assert_eq!(
            err.help().as_deref(),
            Some("set TEMP to a directory that exists and can be written")
        );
        assert!(!missing.exists());
    }
}
