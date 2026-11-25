//! What can stop a build before a compiler runs.

use std::fmt;
use std::path::PathBuf;

use cuno_core::{CunoError, ExitClass};

/// What stops a build: a problem with the package's layout, found before
/// anything is compiled (`docs/en/specs/project-layout.md`, "Entry files and
/// package body"), a `--bin` that names no executable, or a step that failed.
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
    /// Two test executables with the same name.
    DuplicateTestName {
        /// The name.
        name: String,
        /// What each is made from, as shown: `src/` for the in-source
        /// tests, the file otherwise.
        sources: [String; 2],
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
            BuildError::DuplicateTestName { name, sources } => write!(
                f,
                "two test executables are named `{name}`: {} and {}",
                sources[0], sources[1]
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
            | BuildError::DuplicateTestName { .. }
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
            // The in-source tests are named after the package, which is
            // the other thing that can be renamed.
            BuildError::DuplicateTestName { sources, .. } => Some(if sources[0] == "src/" {
                format!("rename {}, or the package", sources[1])
            } else {
                "rename one of the files in tests/".to_owned()
            }),
            BuildError::SeveralBins { available } | BuildError::NoSuchBin { available, .. } => {
                Some(format!("available: {available}"))
            }
            BuildError::Unreadable { .. } | BuildError::Failed { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a user reads, and the exit code, for the errors a build
    /// command meets once the package is laid out (`commands.md`, "`cuno
    /// run`" and "Exit codes").
    ///
    /// Mutation: change a message, a help line or an exit class of
    /// `SeveralBins`, `NoSuchBin` or `Failed`; this test fails.
    #[test]
    fn the_run_and_failure_errors_read_as_the_spec_says() {
        let cases = [
            (
                BuildError::SeveralBins {
                    available: "app, tool".into(),
                },
                "several executables can be run; choose one with --bin",
                Some("available: app, tool"),
                ExitClass::Usage,
            ),
            (
                BuildError::NoSuchBin {
                    name: "x".into(),
                    available: "app, tool".into(),
                },
                "no executable named `x`",
                Some("available: app, tool"),
                ExitClass::Usage,
            ),
            (
                BuildError::Failed {
                    package: "app".into(),
                    link: false,
                },
                "could not compile `app`",
                None,
                ExitClass::Failure,
            ),
            (
                BuildError::Failed {
                    package: "app".into(),
                    link: true,
                },
                "could not link `app`",
                None,
                ExitClass::Failure,
            ),
        ];
        for (error, message, help, class) in cases {
            assert_eq!(error.to_string(), message);
            assert_eq!(error.help().as_deref(), help, "{message}");
            assert_eq!(error.exit_class(), class, "{message}");
        }
        let layout = BuildError::NoExecutable {
            default: "src/main.cpp".into(),
        };
        assert_eq!(layout.exit_class(), ExitClass::Setup);
    }

    /// Two test executables of one name: what a user reads, the help and
    /// the exit code (`build.md`, "Output directory").
    ///
    /// Mutation: change the message, either help or the exit class of
    /// `DuplicateTestName`; this test fails.
    #[test]
    fn two_test_executables_of_one_name_read_as_the_spec_says() {
        let error = BuildError::DuplicateTestName {
            name: "tests-foo".into(),
            sources: ["src/".into(), "tests/foo.cpp".into()],
        };
        assert_eq!(
            error.to_string(),
            "two test executables are named `tests-foo`: src/ and tests/foo.cpp"
        );
        assert_eq!(
            error.help().as_deref(),
            Some("rename tests/foo.cpp, or the package")
        );
        assert_eq!(error.exit_class(), ExitClass::Setup);
        let two_files = BuildError::DuplicateTestName {
            name: "tests-foo".into(),
            sources: ["tests/foo.c".into(), "tests/foo.cpp".into()],
        };
        assert_eq!(
            two_files.help().as_deref(),
            Some("rename one of the files in tests/")
        );
    }
}
