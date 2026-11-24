//! `cuno clean`: remove the output directory, or one profile's part of it.
//! See `docs/en/specs/commands.md`, "`cuno clean`".

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use cuno_build::layout::{is_claimed, profile_dir_name};
use cuno_core::{CunoError, ExitClass, Shell};
use cuno_manifest::{ConfigError, Settings};

use crate::build::{CommandError, boxed, profile_named};

/// The options of `cuno clean`.
#[derive(Debug, Default)]
pub struct CleanArgs {
    /// `--release`.
    pub release: bool,
    /// `--profile`.
    pub profile: Option<String>,
}

/// Removes the output directory of the project around `cwd`, or with a
/// profile only that profile's directory in it, and says how many files
/// went. Nothing is removed unless the settings accept the output
/// directory and a build marked it as cuno's (`docs/en/specs/commands.md`,
/// "`cuno clean`"). `resolved` are the settings read before dispatch;
/// outside a project their error is not reported.
pub fn clean(
    args: CleanArgs,
    resolved: Result<Settings, ConfigError>,
    cwd: &Path,
    shell: &mut Shell,
) -> Result<(), Box<dyn CunoError>> {
    let profile = match (args.release, &args.profile) {
        (false, None) => None,
        (release, name) => Some(profile_named(release, name.as_deref())?),
    };
    // Outside a project the error says so, rather than judging an output
    // directory there is nothing to clean in.
    if cuno_manifest::find_project(cwd).is_none() {
        return Err(Box::new(CommandError::NoProject {
            cwd: cwd.to_owned(),
        }));
    }
    let target_dir = resolved.map_err(boxed)?.target_dir;
    let removed = if std::fs::symlink_metadata(&target_dir).is_err() {
        0
    } else if !is_claimed(&target_dir) {
        return Err(Box::new(CleanError::NotMadeByCuno { dir: target_dir }));
    } else {
        let dir = match profile {
            None => target_dir,
            Some(profile) => target_dir.join(profile_dir_name(profile)),
        };
        remove(&dir).map_err(|e| {
            Box::new(CleanError::Failed {
                dir: dir.clone(),
                reason: e.to_string(),
            }) as Box<dyn CunoError>
        })?
    };
    let noun = if removed == 1 { "file" } else { "files" };
    shell.status("Removed", format!("{removed} {noun}"));
    Ok(())
}

/// Removes `dir` and everything in it, and returns how many files it held:
/// files and links, counted before anything is removed, so a removal that
/// fails part way has counted more than went. A directory that is not
/// there holds none. A link, `dir` itself or one inside it, is removed and
/// not followed, so nothing it points at is removed or counted.
///
/// The count is a walk of its own because the removal is the standard
/// library's, which already handles read-only files and links on Windows.
fn remove(dir: &Path) -> io::Result<u64> {
    let mut files = 0;
    let mut stack = Vec::new();
    // `symlink_metadata`, not `metadata`: a link at the top is not entered.
    match std::fs::symlink_metadata(dir) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e),
        Ok(meta) if meta.is_dir() => stack.push(dir.to_owned()),
        // A link: removed below without being entered.
        Ok(_) => {}
    }
    while let Some(next) = stack.pop() {
        for entry in std::fs::read_dir(&next)? {
            let entry = entry?;
            // A `DirEntry`'s type is the link's own, so a link inside is
            // counted as one file and not entered.
            if entry.file_type()?.is_dir() {
                stack.push(entry.path());
            } else {
                files += 1;
            }
        }
    }
    std::fs::remove_dir_all(dir)?;
    Ok(files)
}

/// Why `cuno clean` removed nothing, or not everything.
#[derive(Debug)]
enum CleanError {
    /// The output directory has no marker: cuno did not make it.
    NotMadeByCuno { dir: PathBuf },
    /// Removing failed part way: a file in use, or one that may not be
    /// deleted.
    Failed { dir: PathBuf, reason: String },
}

impl fmt::Display for CleanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CleanError::NotMadeByCuno { dir } => {
                write!(
                    f,
                    "`{}` was not made by cuno; not removing it",
                    dir.display()
                )
            }
            CleanError::Failed { dir, reason } => {
                write!(f, "could not remove {}: {reason}", dir.display())
            }
        }
    }
}

impl std::error::Error for CleanError {}

impl CunoError for CleanError {
    // Both are the setup cuno runs in, not the user's code: what the
    // settings point at, or a program still using the directory
    // (commands.md, "`cuno clean`").
    fn exit_class(&self) -> ExitClass {
        match self {
            CleanError::NotMadeByCuno { .. } | CleanError::Failed { .. } => ExitClass::Setup,
        }
    }

    fn help(&self) -> Option<String> {
        match self {
            CleanError::NotMadeByCuno { .. } => Some(
                "remove it yourself, or set target-dir to a directory cuno builds into".to_owned(),
            ),
            CleanError::Failed { .. } => None,
        }
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

    /// An output directory, or a directory in it, that is a junction to
    /// somewhere else loses the junction only: what it points at stays, and
    /// is not counted as removed.
    ///
    /// Mutation: read the top with `metadata` instead of
    /// `symlink_metadata`, or a nested entry's type through its path, so a
    /// junction is entered and `keep` counted; this test fails.
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

        // The same junction one level down counts as one file, not as the
        // two it points at.
        std::fs::write(elsewhere.join("keep2"), "").unwrap();
        let dir = root.path().join("target2");
        std::fs::create_dir(&dir).unwrap();
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(dir.join("debug"))
            .arg(&elsewhere)
            .output()
            .unwrap();
        assert!(made.status.success(), "{made:?}");
        assert_eq!(remove(&dir).unwrap(), 1);
        assert!(elsewhere.join("keep").is_file());
    }
}
