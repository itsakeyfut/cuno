//! Updating and uninstalling cuno itself: telling how it was installed, the
//! latest release, downloading it with `curl`, checking it against its
//! SHA-256, and replacing the running executable. See
//! `docs/en/specs/distribution.md`, "Downloads" and "Self-update", and
//! `docs/en/specs/commands.md`, "`cuno self update`".

pub mod fetch;
pub mod install;
pub mod release;
pub mod update;

use std::fmt;
use std::path::PathBuf;

use cuno_core::{CunoError, ExitClass};

/// The one-liner that installs cuno, which an error points to.
pub const INSTALL_COMMAND: &str = "powershell -c \"irm https://github.com/itsakeyfut/cuno/releases/latest/download/install.ps1 | iex\"";

/// Why cuno cannot update itself. Every one is a setup problem (exit code
/// 3): nothing is wrong with the user's code.
#[derive(Debug, PartialEq, Eq)]
pub enum SelfError {
    /// The path of the running executable cannot be found.
    Exe(String),
    /// cuno was installed by `cargo install`, which updates it.
    Cargo,
    /// cuno is somewhere its installer does not put it.
    NotInstaller {
        /// Where the running cuno is.
        exe: PathBuf,
    },
    /// `curl` cannot be started.
    NoCurl {
        /// Why it could not be started.
        reason: String,
    },
    /// `curl` ran and failed: `reason` is its own message.
    Download {
        /// What was being downloaded.
        url: String,
        /// curl's own message.
        reason: String,
    },
    /// The latest-release address did not redirect to a release.
    NotRedirected {
        /// The latest-release address.
        url: String,
    },
    /// The latest release's tag is not `v<x.y.z>`.
    Tag {
        /// The tag as found.
        tag: String,
    },
    /// The zip's SHA-256 is not the one published beside it.
    Checksum {
        /// The zip's name.
        zip: String,
    },
    /// `cuno.exe` cannot be taken out of the zip.
    Unpack {
        /// The zip's name.
        zip: String,
        /// What `tar` said, or why it could not run.
        reason: String,
    },
    /// The new `cuno.exe` cannot be put in place.
    Replace {
        /// The executable being replaced.
        exe: PathBuf,
        /// Why.
        reason: String,
    },
    /// A file the update works with cannot be read or made.
    Io {
        /// The file.
        path: PathBuf,
        /// Why.
        reason: String,
    },
    /// No directory of this run's own can be made in the temporary one.
    Temp(String),
}

impl fmt::Display for SelfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SelfError::Exe(reason) => {
                write!(f, "cannot find where cuno itself is: {reason}")
            }
            SelfError::Cargo => write!(
                f,
                "cuno was installed by cargo install, so cargo updates it"
            ),
            SelfError::NotInstaller { exe } => write!(
                f,
                "{} was not installed by cuno's installer, so cuno cannot update it",
                exe.display()
            ),
            SelfError::NoCurl { reason } => write!(f, "cannot run curl: {reason}"),
            SelfError::Download { url, reason } => write!(f, "cannot download {url}: {reason}"),
            SelfError::NotRedirected { url } => {
                write!(f, "{url} did not redirect to a release")
            }
            SelfError::Tag { tag } => {
                write!(f, "the latest release's tag is not a version: {tag}")
            }
            SelfError::Checksum { zip } => write!(f, "{zip} does not match its checksum"),
            SelfError::Unpack { zip, reason } => {
                write!(f, "cannot take cuno.exe out of {zip}: {reason}")
            }
            SelfError::Replace { exe, reason } => {
                write!(f, "cannot replace {}: {reason}", exe.display())
            }
            SelfError::Io { path, reason } => write!(f, "cannot use {}: {reason}", path.display()),
            SelfError::Temp(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for SelfError {}

impl CunoError for SelfError {
    fn exit_class(&self) -> ExitClass {
        ExitClass::Setup
    }

    fn help(&self) -> Option<String> {
        match self {
            SelfError::Cargo => Some("run cargo install cuno".to_owned()),
            SelfError::NotInstaller { .. } => Some(format!("install cuno with: {INSTALL_COMMAND}")),
            SelfError::NoCurl { .. } => Some(
                "cuno downloads with curl, which Windows 10 (1803) and later include; make sure curl.exe is found on PATH"
                    .to_owned(),
            ),
            SelfError::Exe(_)
            | SelfError::Download { .. }
            | SelfError::NotRedirected { .. }
            | SelfError::Tag { .. }
            | SelfError::Checksum { .. }
            | SelfError::Unpack { .. }
            | SelfError::Replace { .. }
            | SelfError::Io { .. }
            | SelfError::Temp(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two refusals a script meets say what to run instead, as the spec
    /// words them, and every error is exit code 3.
    ///
    /// Mutation: drop either help, or classify one as another exit class;
    /// this test fails.
    #[test]
    fn refusals_say_what_to_run_instead() {
        assert_eq!(
            SelfError::Cargo.help().as_deref(),
            Some("run cargo install cuno")
        );
        let other = SelfError::NotInstaller {
            exe: PathBuf::from(r"C:\tools\cuno.exe"),
        };
        assert_eq!(
            other.to_string(),
            r"C:\tools\cuno.exe was not installed by cuno's installer, so cuno cannot update it"
        );
        assert_eq!(
            other.help().as_deref(),
            Some(
                "install cuno with: powershell -c \"irm https://github.com/itsakeyfut/cuno/releases/latest/download/install.ps1 | iex\""
            )
        );
        assert_eq!(SelfError::Cargo.exit_class(), ExitClass::Setup);
        assert_eq!(other.exit_class(), ExitClass::Setup);
    }
}
