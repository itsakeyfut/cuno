//! What can go wrong while finding and preparing a toolchain. Every case is a
//! setup problem (exit code 3): the user's environment, not their code.

use std::fmt;
use std::path::PathBuf;

use cuno_core::{CunoError, ExitClass};

const BUILD_TOOLS_HELP: &str = "install Build Tools for Visual Studio with the \"Desktop development with C++\" workload: https://visualstudio.microsoft.com/visual-cpp-build-tools/";

/// A toolchain problem.
#[derive(Debug)]
pub enum ToolchainError {
    /// vswhere.exe is not where every Visual Studio 2017+ installer puts it.
    VswhereMissing {
        /// Where it was looked for.
        path: PathBuf,
    },
    /// vswhere.exe ran but failed, or printed something that is not its JSON.
    VswhereFailed {
        /// What went wrong, with vswhere's own output when it gave any.
        detail: String,
    },
    /// No Visual Studio or Build Tools installation has the C++ x64 tools.
    NoVisualStudio,
    /// cl.exe could not be run, or printed no version.
    ClVersionUnknown {
        /// The cl.exe that was run.
        path: PathBuf,
    },
    /// A tool path the user configured has no file.
    ConfiguredToolMissing {
        /// The setting, such as `[toolchain.paths] clang-tidy`.
        setting: String,
        /// The path it gives.
        path: PathBuf,
    },
}

impl fmt::Display for ToolchainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ToolchainError::VswhereMissing { path } => {
                write!(f, "vswhere.exe was not found at {}", path.display())
            }
            ToolchainError::VswhereFailed { detail } => {
                write!(f, "vswhere.exe failed: {detail}")
            }
            ToolchainError::NoVisualStudio => {
                f.write_str("no Visual Studio or Build Tools with the C++ x64 tools was found")
            }
            ToolchainError::ClVersionUnknown { path } => {
                write!(f, "could not read the version of {}", path.display())
            }
            ToolchainError::ConfiguredToolMissing { setting, path } => {
                write!(
                    f,
                    "{setting} is {}, but there is no file there",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for ToolchainError {}

impl CunoError for ToolchainError {
    fn exit_class(&self) -> ExitClass {
        ExitClass::Setup
    }

    fn help(&self) -> Option<String> {
        match self {
            ToolchainError::ConfiguredToolMissing { setting, .. } => Some(format!(
                "correct {setting}, or remove it to let cuno find the tool"
            )),
            ToolchainError::VswhereMissing { .. } | ToolchainError::NoVisualStudio => {
                Some(BUILD_TOOLS_HELP.to_owned())
            }
            ToolchainError::VswhereFailed { .. } | ToolchainError::ClVersionUnknown { .. } => Some(
                "repair the Visual Studio installation with the Visual Studio Installer".to_owned(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mutation: give any variant another exit class; this test fails.
    #[test]
    fn every_toolchain_error_is_a_setup_problem() {
        for e in [
            ToolchainError::VswhereMissing { path: "x".into() },
            ToolchainError::VswhereFailed { detail: "x".into() },
            ToolchainError::NoVisualStudio,
            ToolchainError::ClVersionUnknown { path: "x".into() },
            ToolchainError::ConfiguredToolMissing {
                setting: "x".into(),
                path: "x".into(),
            },
        ] {
            assert_eq!(e.exit_class(), ExitClass::Setup, "{e}");
            assert!(e.help().is_some(), "{e}");
        }
    }

    /// Mutation: drop what was looked for from any message, or give a
    /// missing Visual Studio the repair advice instead of the Build Tools
    /// workload; this test fails.
    #[test]
    fn messages_name_what_was_looked_for_and_how_to_fix_it() {
        let cases = [
            (
                ToolchainError::VswhereMissing {
                    path: "Z:/x/vswhere.exe".into(),
                },
                "Z:/x/vswhere.exe",
                "Desktop development with C++",
            ),
            (
                ToolchainError::VswhereFailed {
                    detail: "access denied".into(),
                },
                "access denied",
                "Visual Studio Installer",
            ),
            (
                ToolchainError::NoVisualStudio,
                "C++ x64 tools",
                "Desktop development with C++",
            ),
            (
                ToolchainError::ClVersionUnknown {
                    path: "Z:/t/cl.exe".into(),
                },
                "Z:/t/cl.exe",
                "Visual Studio Installer",
            ),
            (
                ToolchainError::ConfiguredToolMissing {
                    setting: "[toolchain.paths] clang".into(),
                    path: "Z:/c/clang.exe".into(),
                },
                "Z:/c/clang.exe",
                "[toolchain.paths] clang",
            ),
        ];
        for (error, in_message, in_help) in cases {
            let message = error.to_string();
            let help = error.help().unwrap();
            assert!(message.contains(in_message), "{message}");
            assert!(help.contains(in_help), "{help}");
        }
    }
}
