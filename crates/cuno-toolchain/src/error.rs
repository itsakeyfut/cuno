//! What can go wrong while finding and preparing a toolchain. Every case is a
//! setup problem (exit code 3): the user's environment, not their code.

use std::fmt;
use std::path::PathBuf;

use cuno_core::{CunoError, ExitClass};

use crate::Version;

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
    /// No Windows SDK with the x64 headers and libraries.
    NoWindowsSdk {
        /// The SDK directories looked in, in order.
        looked_in: Vec<PathBuf>,
    },
    /// A compiler name cuno does not know.
    UnknownCompiler {
        /// The name.
        name: String,
        /// Where it was given (`CUNO_COMPILER`, ...).
        source: String,
    },
    /// A compiler cuno does not support yet (`gcc`, `safec`).
    UnsupportedCompiler {
        /// The name.
        name: String,
        /// Where it was chosen.
        source: String,
    },
    /// No copy of the chosen compiler was found.
    CompilerNotFound {
        /// The compiler (`clang`).
        compiler: &'static str,
    },
    /// Copies were found, but none is eligible (a preview, a MinGW build, one
    /// that does not run).
    NoUsableCompiler {
        /// The compiler.
        compiler: &'static str,
        /// Each copy found, with why it was left out.
        excluded: Vec<String>,
    },
    /// Copies were found, but none satisfies the requirements.
    NoMatchingVersion {
        /// The compiler.
        compiler: &'static str,
        /// Each requirement, with where it comes from.
        requirements: Vec<String>,
        /// The versions found.
        found: Vec<Version>,
    },
    /// A `[toolchain.version]` value that is not a requirement.
    InvalidVersionRequirement {
        /// The compiler it is for.
        compiler: String,
        /// The value as written.
        text: String,
    },
    /// `[toolchain.paths] msvc` names a directory with no usable Visual Studio.
    VisualStudioNotAt {
        /// The directory.
        path: PathBuf,
    },
    /// The MSVC standard library rejects this Clang (STL1000).
    RejectedByStl {
        /// The compiler (`clang`, `clang-cl`).
        compiler: &'static str,
        /// Its version.
        version: Version,
        /// The Clang version the library asks for, as written, if it says.
        needed: Option<String>,
        /// The library's message.
        message: String,
    },
    /// The GCC selected did not say where its headers or libraries are
    /// (`toolchain.md`, "MinGW (GCC)"), which the build needs to know.
    GccUnreadable {
        /// The `gcc`.
        path: PathBuf,
        /// What it was asked.
        asked: &'static str,
        /// What it printed, or why it could not run.
        output: String,
    },
    /// The standard library probe failed for another reason, so no verdict.
    ProbeFailed {
        /// The compiler.
        compiler: &'static str,
        /// What it printed, or why it could not run.
        output: String,
    },
    /// The selected compiler is too old for the package's edition.
    EditionNotSupported {
        /// The compiler.
        compiler: &'static str,
        /// Its version.
        version: Version,
        /// The edition, as written.
        edition: &'static str,
        /// The oldest version that accepts it.
        needed: Version,
    },
    /// A tool path the user configured has no file.
    ConfiguredToolMissing {
        /// The setting, such as `[toolchain.paths] clang-tidy`.
        setting: String,
        /// The path it gives.
        path: PathBuf,
    },
    /// A tool path the user configured names a tool that cannot be used (it
    /// does not run, prints no version, is a preview, or targets MinGW).
    ConfiguredToolUnusable {
        /// The setting, such as `[toolchain.paths] clang-tidy`.
        setting: String,
        /// The path it gives.
        path: PathBuf,
        /// Why it cannot be used, such as `it did not run`.
        why: String,
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
            ToolchainError::NoWindowsSdk { looked_in } => {
                let places: Vec<String> =
                    looked_in.iter().map(|p| p.display().to_string()).collect();
                write!(
                    f,
                    "no Windows SDK with the x64 headers and libraries was found in {}",
                    places.join(" or ")
                )
            }
            ToolchainError::UnknownCompiler { name, source } => {
                write!(
                    f,
                    "{source} names `{name}`, which is not a compiler cuno knows"
                )
            }
            ToolchainError::UnsupportedCompiler { name, source } => {
                write!(f, "{name} is not supported yet (chosen by {source})")
            }
            ToolchainError::CompilerNotFound { compiler } => write!(f, "no {compiler} was found"),
            ToolchainError::NoUsableCompiler { compiler, excluded } => write!(
                f,
                "no usable {compiler} was found; left out: {}",
                excluded.join("; ")
            ),
            ToolchainError::NoMatchingVersion {
                compiler,
                requirements,
                found,
            } => {
                let found: Vec<String> = found.iter().map(Version::to_string).collect();
                write!(
                    f,
                    "no {compiler} found satisfies {}; found {}",
                    requirements.join(" and "),
                    found.join(", ")
                )
            }
            ToolchainError::InvalidVersionRequirement { compiler, text } => write!(
                f,
                "[toolchain.version] {compiler} = \"{text}\" is not a version requirement"
            ),
            ToolchainError::VisualStudioNotAt { path } => write!(
                f,
                "[toolchain.paths] msvc is {}, but no Visual Studio with the C++ x64 tools is installed there",
                path.display()
            ),
            ToolchainError::RejectedByStl {
                compiler, version, ..
            } => write!(
                f,
                "{compiler} {version} is not accepted by the MSVC standard library in use (STL1000)"
            ),
            ToolchainError::ProbeFailed { compiler, output } => write!(
                f,
                "could not tell whether {compiler} works with the MSVC standard library:\n{output}"
            ),
            ToolchainError::GccUnreadable {
                path,
                asked,
                output,
            } => write!(f, "{} did not say {asked}:\n{output}", path.display()),
            ToolchainError::EditionNotSupported {
                compiler,
                version,
                edition,
                ..
            } => write!(
                f,
                "{compiler} {version} does not support edition = \"{edition}\""
            ),
            ToolchainError::ConfiguredToolUnusable { setting, path, why } => {
                write!(f, "{setting} is {}, but {why}", path.display())
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

/// GCC's name, as the errors carry it.
const GCC: &str = crate::compiler::CompilerKind::Gcc.name();

/// How to install the GCC cuno uses: MSYS2's UCRT64 environment.
const MSYS2_GCC: &str = "https://www.msys2.org/, then `pacman -S mingw-w64-ucrt-x86_64-gcc`";

impl CunoError for ToolchainError {
    fn exit_class(&self) -> ExitClass {
        ExitClass::Setup
    }

    fn help(&self) -> Option<String> {
        match self {
            ToolchainError::UnknownCompiler { .. } | ToolchainError::UnsupportedCompiler { .. } => {
                Some(format!("choose {}", crate::compiler::CompilerKind::names()))
            }
            // GCC is MSYS2's UCRT64 one (`toolchain.md`, "MinGW (GCC)").
            ToolchainError::CompilerNotFound { compiler } if *compiler == GCC => Some(format!(
                "install MSYS2's UCRT64 GCC ({MSYS2_GCC}), or write where gcc is in [toolchain.paths] gcc"
            )),
            ToolchainError::NoUsableCompiler { compiler, .. } if *compiler == GCC => Some(format!(
                "install MSYS2's UCRT64 GCC ({MSYS2_GCC}), or write where a usable gcc is in [toolchain.paths] gcc"
            )),
            ToolchainError::CompilerNotFound { compiler } => Some(format!(
                "install LLVM (https://github.com/llvm/llvm-project/releases), or write where {compiler} is in [toolchain.paths] {compiler}"
            )),
            ToolchainError::NoUsableCompiler { compiler, .. } => Some(format!(
                "install a release of LLVM for Windows (https://github.com/llvm/llvm-project/releases), or write where a usable {compiler} is in [toolchain.paths] {compiler}"
            )),
            ToolchainError::NoMatchingVersion { compiler, .. } => Some(format!(
                "install a {compiler} that satisfies it, or change [toolchain.version] {compiler}"
            )),
            ToolchainError::InvalidVersionRequirement { .. } => Some(
                "write conditions such as \">=17\" or \">=19.29, <19.50\"".to_owned(),
            ),
            ToolchainError::VisualStudioNotAt { .. } => Some(
                "correct [toolchain.paths] msvc, or remove it to let cuno find Visual Studio"
                    .to_owned(),
            ),
            ToolchainError::RejectedByStl {
                compiler: "clang-tidy",
                needed: Some(needed),
                ..
            } => Some(format!("install clang-tidy {needed} or newer")),
            ToolchainError::RejectedByStl {
                needed, message, ..
            } => Some(match needed {
                Some(needed) => {
                    format!("install Clang {needed} or newer, or select another compiler")
                }
                None => format!("the standard library says: {message}"),
            }),
            ToolchainError::ProbeFailed { .. } => Some(
                "check `[build.<compiler>] flags` in cuno.toml, and the Visual Studio and LLVM installations".to_owned(),
            ),
            ToolchainError::GccUnreadable { .. } => Some(format!(
                "reinstall MSYS2's UCRT64 GCC ({MSYS2_GCC}), or write where a working gcc is in [toolchain.paths] gcc"
            )),

            ToolchainError::EditionNotSupported {
                compiler, needed, ..
            } => Some(format!(
                "use {compiler} {needed} or newer, or choose another edition or compiler"
            )),
            ToolchainError::ConfiguredToolMissing { setting, .. }
            | ToolchainError::ConfiguredToolUnusable { setting, .. } => Some(format!(
                "correct {setting}, or remove it to let cuno find the tool"
            )),
            ToolchainError::VswhereMissing { .. } | ToolchainError::NoVisualStudio => {
                Some(BUILD_TOOLS_HELP.to_owned())
            }
            ToolchainError::NoWindowsSdk { .. } => Some(
                "add a Windows SDK in the Visual Studio Installer (a \"Windows 11 SDK\" component of the \"Desktop development with C++\" workload)".to_owned(),
            ),
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
            ToolchainError::NoWindowsSdk {
                looked_in: vec!["x".into()],
            },
            ToolchainError::UnknownCompiler {
                name: "x".into(),
                source: "x".into(),
            },
            ToolchainError::UnsupportedCompiler {
                name: "x".into(),
                source: "x".into(),
            },
            ToolchainError::CompilerNotFound { compiler: "x" },
            ToolchainError::NoUsableCompiler {
                compiler: "x",
                excluded: vec![],
            },
            ToolchainError::NoMatchingVersion {
                compiler: "x",
                requirements: vec![],
                found: vec![],
            },
            ToolchainError::InvalidVersionRequirement {
                compiler: "x".into(),
                text: "x".into(),
            },
            ToolchainError::VisualStudioNotAt { path: "x".into() },
            ToolchainError::RejectedByStl {
                compiler: "x",
                version: Version {
                    major: 0,
                    minor: 0,
                    patch: 0,
                },
                needed: None,
                message: "x".into(),
            },
            ToolchainError::ProbeFailed {
                compiler: "x",
                output: "x".into(),
            },
            ToolchainError::EditionNotSupported {
                compiler: "x",
                version: Version {
                    major: 0,
                    minor: 0,
                    patch: 0,
                },
                edition: "x",
                needed: Version {
                    major: 0,
                    minor: 0,
                    patch: 0,
                },
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
            (
                ToolchainError::NoWindowsSdk {
                    looked_in: vec!["Y:/Kits".into(), "Z:/Windows Kits/10".into()],
                },
                "Y:/Kits or Z:/Windows Kits/10",
                "Windows 11 SDK",
            ),
            (
                ToolchainError::UnsupportedCompiler {
                    name: "safec".into(),
                    source: "CUNO_COMPILER".into(),
                },
                "safec is not supported yet (chosen by CUNO_COMPILER)",
                "msvc, clang, clang-cl or gcc",
            ),
            (
                ToolchainError::NoMatchingVersion {
                    compiler: "msvc",
                    requirements: vec![">=19.29 (cuno's minimum)".into()],
                    found: vec![Version {
                        major: 19,
                        minor: 28,
                        patch: 1,
                    }],
                },
                "satisfies >=19.29 (cuno's minimum); found 19.28.1",
                "[toolchain.version] msvc",
            ),
            (
                ToolchainError::RejectedByStl {
                    compiler: "clang",
                    version: Version {
                        major: 20,
                        minor: 1,
                        patch: 8,
                    },
                    needed: Some("22".into()),
                    message: "STL1000: ...".into(),
                },
                "clang 20.1.8 is not accepted by the MSVC standard library in use (STL1000)",
                "install Clang 22 or newer, or select another compiler",
            ),
            (
                ToolchainError::RejectedByStl {
                    compiler: "clang",
                    version: Version {
                        major: 20,
                        minor: 1,
                        patch: 8,
                    },
                    needed: None,
                    message: "STL1000: odd wording".into(),
                },
                "(STL1000)",
                "STL1000: odd wording",
            ),
            (
                ToolchainError::UnknownCompiler {
                    name: "nope".into(),
                    source: "--compiler".into(),
                },
                "--compiler names `nope`, which is not a compiler cuno knows",
                "msvc, clang, clang-cl or gcc",
            ),
            (
                ToolchainError::CompilerNotFound { compiler: "clang" },
                "no clang was found",
                "[toolchain.paths] clang",
            ),
            (
                ToolchainError::NoUsableCompiler {
                    compiler: "clang",
                    excluded: vec!["a (x)".into(), "b (y)".into()],
                },
                "no usable clang was found; left out: a (x); b (y)",
                "release of LLVM",
            ),
            (
                ToolchainError::CompilerNotFound { compiler: "gcc" },
                "no gcc was found",
                "pacman -S mingw-w64-ucrt-x86_64-gcc`), or write where gcc is in [toolchain.paths] gcc",
            ),
            (
                ToolchainError::NoUsableCompiler {
                    compiler: "gcc",
                    excluded: vec![
                        "C:/mingw64/bin/gcc.exe (its C runtime is MSVCRT, not UCRT)".into(),
                    ],
                },
                "left out: C:/mingw64/bin/gcc.exe (its C runtime is MSVCRT, not UCRT)",
                "install MSYS2's UCRT64 GCC",
            ),
            (
                ToolchainError::InvalidVersionRequirement {
                    compiler: "clang".into(),
                    text: "~17".into(),
                },
                "[toolchain.version] clang = \"~17\" is not a version requirement",
                "\">=19.29, <19.50\"",
            ),
            (
                ToolchainError::VisualStudioNotAt {
                    path: "Z:/VS".into(),
                },
                "[toolchain.paths] msvc is Z:/VS, but no Visual Studio",
                "correct [toolchain.paths] msvc",
            ),
            (
                ToolchainError::ProbeFailed {
                    compiler: "clang-cl",
                    output: "fatal error: x".into(),
                },
                "whether clang-cl works with the MSVC standard library:\nfatal error: x",
                "[build.<compiler>] flags",
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
