//! `cuno clean`: remove the output directory, or one profile's part of it.
//! See `docs/en/specs/commands.md`, "`cuno clean`".

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use cuno_build::layout::profile_dir_name;
use cuno_core::{CunoError, Env, ExitClass, Shell};
use cuno_manifest::{CliSettings, Inputs};

use crate::build::{CommandError, boxed, profile};

/// The options of `cuno clean`.
#[derive(Debug, Default)]
pub struct CleanArgs {
    /// `--release`.
    pub release: bool,
    /// `--profile`.
    pub profile: Option<String>,
    /// `--target-dir`.
    pub target_dir: Option<PathBuf>,
}

/// Removes the output directory of the project around `cwd`, or with a
/// profile only that profile's directory in it, and says how many files
/// went. The output directory is the one the settings resolve, which they
/// have already refused when it could hold the project
/// (`docs/en/specs/build.md`, "Output directory").
pub fn clean(
    args: CleanArgs,
    quiet: bool,
    env: &Env,
    cwd: &Path,
    shell: &mut Shell,
) -> Result<(), Box<dyn CunoError>> {
    let profile = match (args.release, &args.profile) {
        (false, None) => None,
        (release, name) => Some(profile(release, name.as_deref())?),
    };
    let cli = CliSettings {
        target_dir: args.target_dir,
        ..CliSettings::default()
    };
    let loaded = cuno_manifest::resolve(Inputs {
        cwd,
        env,
        cli: &cli,
    })
    .map_err(boxed)?;
    for warning in &loaded.warnings {
        shell.warning(warning);
    }
    let settings = loaded.value;
    if settings.project.is_none() {
        return Err(Box::new(CommandError::NoProject {
            cwd: cwd.to_owned(),
        }));
    }
    let dir = match profile {
        None => settings.target_dir.clone(),
        Some(profile) => settings.target_dir.join(profile_dir_name(profile)),
    };
    let removed = remove(&dir).map_err(|e| {
        Box::new(CleanError {
            dir: dir.clone(),
            reason: e.to_string(),
        }) as Box<dyn CunoError>
    })?;
    if !quiet {
        let noun = if removed == 1 { "file" } else { "files" };
        shell.status("Removed", format!("{removed} {noun}"));
    }
    Ok(())
}

/// Removes `dir` and everything in it, and returns how many files it held.
/// A directory that is not there holds none. A link, `dir` itself or one
/// inside it, is removed and not followed, so nothing it points at is
/// removed or counted.
fn remove(dir: &Path) -> io::Result<u64> {
    let mut files = 0;
    let mut stack = Vec::new();
    match std::fs::symlink_metadata(dir) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e),
        Ok(meta) if meta.is_dir() => stack.push(dir.to_owned()),
        Ok(_) => {}
    }
    while let Some(next) = stack.pop() {
        let entries = match std::fs::read_dir(&next) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        for entry in entries {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                stack.push(entry.path());
            } else {
                files += 1;
            }
        }
    }
    match std::fs::remove_dir_all(dir) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(0),
        other => other.map(|()| files),
    }
}

/// The output directory could not be removed: a file in it is in use, or
/// may not be deleted.
#[derive(Debug)]
struct CleanError {
    dir: PathBuf,
    reason: String,
}

impl fmt::Display for CleanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "could not remove {}: {}",
            self.dir.display(),
            self.reason
        )
    }
}

impl std::error::Error for CleanError {}

impl CunoError for CleanError {
    // The setup cuno runs in stops it, not the user's code: a program
    // still running from the directory, or permissions.
    fn exit_class(&self) -> ExitClass {
        ExitClass::Setup
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every file is counted, however deep, a missing directory holds
    /// none, and the directory is gone afterwards.
    ///
    /// Mutation: count only the top level, count directories, or fail on a
    /// missing directory; this test fails.
    #[test]
    fn remove_counts_every_file_and_tolerates_none() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("target");
        std::fs::create_dir_all(dir.join("debug/obj/app")).unwrap();
        for file in ["a", "debug/b", "debug/obj/app/c", "debug/obj/app/d"] {
            std::fs::write(dir.join(file), "").unwrap();
        }
        assert_eq!(remove(&dir).unwrap(), 4);
        assert!(!dir.exists());
        assert_eq!(remove(&dir).unwrap(), 0);
    }

    /// An output directory, or a profile's directory, that is a junction
    /// to somewhere else loses the junction only: what it points at stays,
    /// and is not counted as removed.
    ///
    /// Mutation: follow a link at the top when counting; this test fails.
    /// (Removing through it is what the standard library already refuses.)
    #[cfg(windows)]
    #[test]
    fn a_junction_is_removed_not_followed() {
        let root = tempfile::tempdir().unwrap();
        let elsewhere = root.path().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::write(elsewhere.join("keep"), "").unwrap();
        let link = root.path().join("target");
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&elsewhere)
            .output()
            .unwrap();
        assert!(made.status.success(), "{made:?}");
        assert_eq!(remove(&link).unwrap(), 0);
        assert!(std::fs::symlink_metadata(&link).is_err());
        assert!(elsewhere.join("keep").is_file());
    }
}
