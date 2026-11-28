//! Updating and uninstalling cuno itself: telling how it was installed, the
//! latest release, downloading it with `curl`, checking it against its
//! SHA-256, and replacing the running executable. See
//! `docs/en/specs/distribution.md`, "Downloads" and "Self-update", and
//! `docs/en/specs/commands.md`, "`cuno self update`".

pub mod fetch;
pub mod install;
pub mod release;
pub mod uninstall;
pub mod update;

use std::fmt;
use std::path::PathBuf;

use cuno_core::{CunoError, ExitClass};

/// The one-liner that installs cuno, which an error points to.
pub const INSTALL_COMMAND: &str = "powershell -c \"irm https://github.com/itsakeyfut/cuno/releases/latest/download/install.ps1 | iex\"";

/// Why cuno cannot update or uninstall itself. Every one is a setup
/// problem (exit code 3), nothing being wrong with the user's code, but
/// [`SelfError::NoAnswer`], a usage error (exit code 2).
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
    /// The cuno home is a drive's root, the user's home or above it.
    NotOwnHome {
        /// The cuno home.
        home: PathBuf,
    },
    /// Something in the cuno home cannot be removed.
    Remove {
        /// What could not be removed.
        path: PathBuf,
        /// Why.
        reason: String,
    },
    /// The user's `PATH` cannot be read or written.
    PathEdit {
        /// What PowerShell said.
        reason: String,
    },
    /// The process that removes what is left cannot be started.
    Helper {
        /// Why.
        reason: String,
    },
    /// The cuno home, or its `bin`, is a link.
    Link {
        /// The link.
        path: PathBuf,
    },
    /// Uninstalling would ask, and standard input is not a terminal.
    NoAnswer,
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
            SelfError::NotOwnHome { home } => write!(
                f,
                "refusing to remove {}: it is not a directory of cuno's own",
                home.display()
            ),
            SelfError::Remove { path, reason } => {
                write!(f, "cannot remove {}: {reason}", path.display())
            }
            SelfError::PathEdit { reason } => write!(f, "cannot edit the user's PATH: {reason}"),
            SelfError::Helper { reason } => {
                write!(
                    f,
                    "cannot start the process that removes cuno.exe: {reason}"
                )
            }
            SelfError::Link { path } => write!(
                f,
                "refusing to remove {}: it is a link; remove it and what it leads to by hand",
                path.display()
            ),
            SelfError::NoAnswer => f.write_str(
                "cuno self uninstall asks before deleting, and standard input is not a terminal",
            ),
        }
    }
}

impl std::error::Error for SelfError {}

impl CunoError for SelfError {
    fn exit_class(&self) -> ExitClass {
        match self {
            SelfError::NoAnswer => ExitClass::Usage,
            SelfError::Exe(_)
            | SelfError::Cargo
            | SelfError::NotInstaller { .. }
            | SelfError::NoCurl { .. }
            | SelfError::Download { .. }
            | SelfError::NotRedirected { .. }
            | SelfError::Tag { .. }
            | SelfError::Checksum { .. }
            | SelfError::Unpack { .. }
            | SelfError::Replace { .. }
            | SelfError::Io { .. }
            | SelfError::Temp(_)
            | SelfError::NotOwnHome { .. }
            | SelfError::Remove { .. }
            | SelfError::PathEdit { .. }
            | SelfError::Helper { .. }
            | SelfError::Link { .. } => ExitClass::Setup,
        }
    }

    fn help(&self) -> Option<String> {
        match self {
            SelfError::Cargo => Some("run cargo install cuno".to_owned()),
            SelfError::NotInstaller { .. } => Some(format!("install cuno with: {INSTALL_COMMAND}")),
            SelfError::NoCurl { .. } => Some(
                "curl.exe is part of Windows 10 (1803) and later; without it, install curl with: winget install cURL.cURL"
                    .to_owned(),
            ),
            SelfError::NoAnswer => Some("pass -y to remove without asking".to_owned()),
            SelfError::Exe(_)
            | SelfError::Download { .. }
            | SelfError::NotRedirected { .. }
            | SelfError::Tag { .. }
            | SelfError::Checksum { .. }
            | SelfError::Unpack { .. }
            | SelfError::Replace { .. }
            | SelfError::Io { .. }
            | SelfError::Temp(_)
            | SelfError::NotOwnHome { .. }
            | SelfError::Remove { .. }
            | SelfError::PathEdit { .. }
            | SelfError::Helper { .. }
            | SelfError::Link { .. } => None,
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

    /// Every error's message as the user reads it, each exit code 3, and
    /// the curl help naming where curl comes from.
    ///
    /// Mutation: reword any message, classify any variant otherwise, or
    /// drop the curl help; this test fails.
    #[test]
    fn every_error_reads_as_the_spec_words_it() {
        let p = PathBuf::from;
        let cases = [
            (
                SelfError::Exe("denied".into()),
                "cannot find where cuno itself is: denied",
            ),
            (
                SelfError::Cargo,
                "cuno was installed by cargo install, so cargo updates it",
            ),
            (
                SelfError::NoCurl {
                    reason: "gone".into(),
                },
                "cannot run curl: gone",
            ),
            (
                SelfError::Download {
                    url: "https://x/a.zip".into(),
                    reason: "curl: (22) 404".into(),
                },
                "cannot download https://x/a.zip: curl: (22) 404",
            ),
            (
                SelfError::NotRedirected {
                    url: "https://x/latest".into(),
                },
                "https://x/latest did not redirect to a release",
            ),
            (
                SelfError::Tag {
                    tag: "nightly".into(),
                },
                "the latest release's tag is not a version: nightly",
            ),
            (
                SelfError::Checksum {
                    zip: "a.zip".into(),
                },
                "a.zip does not match its checksum",
            ),
            (
                SelfError::Unpack {
                    zip: "a.zip".into(),
                    reason: "not found".into(),
                },
                "cannot take cuno.exe out of a.zip: not found",
            ),
            (
                SelfError::Replace {
                    exe: p(r"C:\h\bin\cuno.exe"),
                    reason: "denied".into(),
                },
                r"cannot replace C:\h\bin\cuno.exe: denied",
            ),
            (
                SelfError::Io {
                    path: p(r"C:\t\a.sha256"),
                    reason: "denied".into(),
                },
                r"cannot use C:\t\a.sha256: denied",
            ),
            (
                SelfError::Temp("cannot create the work directory".into()),
                "cannot create the work directory",
            ),
            (
                SelfError::NotOwnHome {
                    home: p(r"C:\Users\me"),
                },
                r"refusing to remove C:\Users\me: it is not a directory of cuno's own",
            ),
            (
                SelfError::Remove {
                    path: p(r"C:\h\x"),
                    reason: "denied".into(),
                },
                r"cannot remove C:\h\x: denied",
            ),
            (
                SelfError::PathEdit {
                    reason: "denied".into(),
                },
                "cannot edit the user's PATH: denied",
            ),
            (
                SelfError::Helper {
                    reason: "gone".into(),
                },
                "cannot start the process that removes cuno.exe: gone",
            ),
            (
                SelfError::Link { path: p(r"C:\h") },
                r"refusing to remove C:\h: it is a link; remove it and what it leads to by hand",
            ),
        ];
        for (error, message) in cases {
            assert_eq!(error.to_string(), message);
            assert_eq!(error.exit_class(), ExitClass::Setup, "{message}");
        }
        assert_eq!(
            SelfError::NoCurl {
                reason: String::new()
            }
            .help()
            .as_deref(),
            Some(
                "curl.exe is part of Windows 10 (1803) and later; without it, install curl with: winget install cURL.cURL"
            )
        );
        assert_eq!(SelfError::Checksum { zip: String::new() }.help(), None);
        // Asking with nobody to answer is a usage error.
        assert_eq!(
            SelfError::NoAnswer.to_string(),
            "cuno self uninstall asks before deleting, and standard input is not a terminal"
        );
        assert_eq!(SelfError::NoAnswer.exit_class(), ExitClass::Usage);
        assert_eq!(
            SelfError::NoAnswer.help().as_deref(),
            Some("pass -y to remove without asking")
        );
    }
}
