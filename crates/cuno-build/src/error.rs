//! What can stop a build before a compiler runs.

use std::fmt;
use std::path::PathBuf;

use cuno_core::{CunoError, ExitClass};

/// A problem with the package's layout, found before anything is compiled
/// (`docs/en/specs/project-layout.md`, "Entry files and the package
/// body"). Every one is a setup problem: exit code 3.
#[derive(Debug)]
pub enum BuildError {
    /// No entry file, nothing in `src/bin/`, no `[[bin]]`.
    NoExecutable {
        /// The default entry file looked for, as shown (`src/main.cpp`).
        default: String,
    },
    /// A `[[bin]]` path with no file.
    BinPathMissing {
        /// The executable's name.
        name: String,
        /// The path, as shown.
        path: String,
    },
    /// An entry file outside the project.
    OutsideProject {
        /// The executable's name.
        name: String,
        /// The path.
        path: PathBuf,
    },
    /// Two executables with the same name.
    DuplicateName {
        /// The name.
        name: String,
        /// The two entry files, as shown.
        entries: [String; 2],
    },
    /// A directory of sources that could not be read.
    Unreadable {
        /// The directory, as shown.
        dir: String,
        /// Why.
        reason: String,
    },
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BuildError::NoExecutable { .. } => f.write_str("no executable to build"),
            BuildError::BinPathMissing { name, path } => {
                write!(f, "[[bin]] `{name}` is {path}, but there is no file there")
            }
            BuildError::OutsideProject { name, path } => write!(
                f,
                "the entry file of `{name}` is outside the project: {}",
                path.display()
            ),
            BuildError::DuplicateName { name, entries } => write!(
                f,
                "two executables are named `{name}`: {} and {}",
                entries[0], entries[1]
            ),
            BuildError::Unreadable { dir, reason } => write!(f, "cannot read {dir}: {reason}"),
        }
    }
}

impl std::error::Error for BuildError {}

impl CunoError for BuildError {
    fn exit_class(&self) -> ExitClass {
        ExitClass::Setup
    }

    fn help(&self) -> Option<String> {
        match self {
            BuildError::NoExecutable { default } => {
                Some(format!("add {default}, or list one in [[bin]]"))
            }
            BuildError::BinPathMissing { name, .. } => {
                Some(format!("correct the path of [[bin]] `{name}`"))
            }
            BuildError::OutsideProject { .. } => Some(
                "move it into the project, or point [[bin]] path at a file inside it".to_owned(),
            ),
            BuildError::DuplicateName { .. } => {
                Some("give one of them another name with [[bin]] name".to_owned())
            }
            BuildError::Unreadable { .. } => None,
        }
    }
}
