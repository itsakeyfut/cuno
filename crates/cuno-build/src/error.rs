//! What can stop a build before a compiler runs.

use std::fmt;
use std::path::PathBuf;

use cuno_core::{CunoError, ExitClass};

/// What stops a build: a problem with the package's layout, found before
/// anything is compiled (`docs/en/specs/project-layout.md`, "Entry files
/// and the package body"), a `--bin` that names no executable, or a step
/// that failed.
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
    /// A `[[bin]]` path that is not a C or C++ source.
    NotASource {
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
    /// `cuno run` without `--bin`, and more than one executable.
    SeveralBins {
        /// The executables' names, comma separated.
        available: String,
    },
    /// `--bin` names no executable.
    NoSuchBin {
        /// The name given.
        name: String,
        /// The executables' names, comma separated.
        available: String,
    },
    /// A compile or a link failed; the compiler said why.
    Failed {
        /// The package.
        package: String,
        /// Whether the first failure was a link.
        link: bool,
    },
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BuildError::NoExecutable { .. } => f.write_str("no executable to build"),
            BuildError::NotASource { name, path } => write!(
                f,
                "[[bin]] `{name}` is {path}, which is not a C or C++ source"
            ),
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
            BuildError::SeveralBins { .. } => {
                f.write_str("several executables can be run; choose one with --bin")
            }
            BuildError::NoSuchBin { name, .. } => write!(f, "no executable named `{name}`"),
            BuildError::Failed { package, link } => {
                let what = if *link { "link" } else { "compile" };
                write!(f, "could not {what} `{package}`")
            }
        }
    }
}

impl std::error::Error for BuildError {}

impl CunoError for BuildError {
    fn exit_class(&self) -> ExitClass {
        match self {
            BuildError::Failed { .. } => ExitClass::Failure,
            BuildError::SeveralBins { .. } | BuildError::NoSuchBin { .. } => ExitClass::Usage,
            BuildError::NoExecutable { .. }
            | BuildError::BinPathMissing { .. }
            | BuildError::NotASource { .. }
            | BuildError::OutsideProject { .. }
            | BuildError::DuplicateName { .. }
            | BuildError::Unreadable { .. } => ExitClass::Setup,
        }
    }

    fn help(&self) -> Option<String> {
        match self {
            BuildError::NoExecutable { default } => {
                Some(format!("add {default}, or list one in [[bin]]"))
            }
            BuildError::BinPathMissing { name, .. } | BuildError::NotASource { name, .. } => {
                Some(format!("correct the path of [[bin]] `{name}`"))
            }
            BuildError::OutsideProject { .. } => Some(
                "move it into the project, or point [[bin]] path at a file inside it".to_owned(),
            ),
            BuildError::DuplicateName { .. } => {
                Some("give one of them another name with [[bin]] name".to_owned())
            }
            BuildError::SeveralBins { available } | BuildError::NoSuchBin { available, .. } => {
                Some(format!("available: {available}"))
            }
            BuildError::Unreadable { .. } | BuildError::Failed { .. } => None,
        }
    }
}
