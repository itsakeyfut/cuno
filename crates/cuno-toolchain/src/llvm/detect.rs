//! Finding every copy of the LLVM tools cuno uses, in the order the spec
//! gives, with each copy's version.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use cuno_core::Env;

use crate::msvc::VsInstallation;
use crate::{HostOs, Runner, ToolchainError, Version};

/// An LLVM tool cuno uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// `clang` (GNU-style driver).
    Clang,
    /// `clang-cl` (MSVC-style driver).
    ClangCl,
    /// `clang-format`.
    ClangFormat,
    /// `clang-tidy`.
    ClangTidy,
}

impl Tool {
    /// Every tool, in a fixed order.
    pub const ALL: [Tool; 4] = [
        Tool::Clang,
        Tool::ClangCl,
        Tool::ClangFormat,
        Tool::ClangTidy,
    ];

    /// The executable's name without a suffix, which is also its key in
    /// `[toolchain.paths]`.
    pub fn name(self) -> &'static str {
        match self {
            Tool::Clang => "clang",
            Tool::ClangCl => "clang-cl",
            Tool::ClangFormat => "clang-format",
            Tool::ClangTidy => "clang-tidy",
        }
    }
}

/// Where a copy of a tool was found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    /// The path written in `[toolchain.paths]`.
    Configured,
    /// A directory on `PATH`.
    Path,
    /// `%ProgramFiles%\LLVM\bin`.
    LlvmInstall,
    /// The Clang bundled with the Visual Studio installed here.
    VisualStudio(PathBuf),
}

/// The paths written in `[toolchain.paths]`, as `cuno-manifest` will give
/// them.
#[derive(Clone, Debug, Default)]
pub struct ExplicitPaths {
    /// `[toolchain.paths] clang`.
    pub clang: Option<PathBuf>,
    /// `[toolchain.paths] clang-cl`.
    pub clang_cl: Option<PathBuf>,
    /// `[toolchain.paths] clang-format`.
    pub clang_format: Option<PathBuf>,
    /// `[toolchain.paths] clang-tidy`.
    pub clang_tidy: Option<PathBuf>,
}

impl ExplicitPaths {
    fn get(&self, tool: Tool) -> Option<&Path> {
        match tool {
            Tool::Clang => self.clang.as_deref(),
            Tool::ClangCl => self.clang_cl.as_deref(),
            Tool::ClangFormat => self.clang_format.as_deref(),
            Tool::ClangTidy => self.clang_tidy.as_deref(),
        }
    }
}

/// One copy of an LLVM tool.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LlvmTool {
    /// Which tool.
    pub tool: Tool,
    /// The executable.
    pub path: PathBuf,
    /// Its version, if `--version` told one.
    pub version: Option<Version>,
    /// Where it was found.
    pub origin: Origin,
}

/// Every copy of every tool, in the spec's search order per tool: the
/// configured path, `PATH`, the LLVM installer's location, then each Visual
/// Studio's bundled Clang (the last two on Windows only). The same file found
/// twice is listed once. Not finding a tool is not an error; a configured path
/// with no file is.
pub fn find_llvm_tools(
    env: &Env,
    host: HostOs,
    explicit: &ExplicitPaths,
    installs: &[VsInstallation],
    run: &dyn Runner,
) -> Result<Vec<LlvmTool>, ToolchainError> {
    let mut dirs: Vec<(PathBuf, Origin)> = Vec::new();
    if let Some(path) = env.get("PATH") {
        dirs.extend(std::env::split_paths(path).map(|d| (d, Origin::Path)));
    }
    if host == HostOs::Windows {
        if let Some(program_files) = env.get("ProgramFiles") {
            dirs.push((
                PathBuf::from(program_files).join("LLVM").join("bin"),
                Origin::LlvmInstall,
            ));
        }
        for install in installs {
            dirs.push((
                install
                    .path
                    .join("VC")
                    .join("Tools")
                    .join("Llvm")
                    .join("x64")
                    .join("bin"),
                Origin::VisualStudio(install.path.clone()),
            ));
        }
    }

    let mut found = Vec::new();
    for tool in Tool::ALL {
        let mut seen: Vec<PathBuf> = Vec::new();
        let mut candidates: Vec<(PathBuf, Origin)> = Vec::new();
        if let Some(path) = explicit.get(tool) {
            if !path.is_file() {
                return Err(ToolchainError::ConfiguredToolMissing {
                    setting: format!("[toolchain.paths] {}", tool.name()),
                    path: path.to_owned(),
                });
            }
            candidates.push((path.to_owned(), Origin::Configured));
        }
        let file = host.executable(tool.name());
        candidates.extend(
            dirs.iter()
                .map(|(dir, origin)| (dir.join(&file), origin.clone()))
                .filter(|(path, _)| path.is_file()),
        );
        for (path, origin) in candidates {
            let identity = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            if seen.contains(&identity) {
                continue;
            }
            seen.push(identity);
            found.push(LlvmTool {
                tool,
                version: tool_version(&path, run, env),
                path,
                origin,
            });
        }
    }
    Ok(found)
}

/// The version in `<tool> --version`: the first `<n>.<n>.<n>` after the word
/// `version` (`clang version 22.1.8`, `LLVM version 22.1.8`).
fn tool_version(path: &Path, run: &dyn Runner, env: &Env) -> Option<Version> {
    let output = run.run(path, &[OsStr::new("--version")], env).ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let after = &text[text.find("version")?..];
    Version::find_in(after)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Output;
    use cuno_core::NameCase;

    /// Answers `--version` with the version encoded in the file's parent
    /// directory name (`.../v22/clang.exe` answers `clang version 22.0.0`).
    struct ByDir;

    impl Runner for ByDir {
        fn run(&self, program: &Path, _args: &[&OsStr], _env: &Env) -> std::io::Result<Output> {
            let dir = program
                .parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy();
            let major = dir.trim_start_matches('v');
            Ok(Output {
                success: true,
                stdout: format!("clang version {major}.0.0 (test)\n").into_bytes(),
                stderr: Vec::new(),
            })
        }
    }

    /// Creates `root/<rel>/<file>` for each entry.
    fn files(root: &Path, entries: &[(&str, &str)]) {
        for (rel, file) in entries {
            let dir = root.join(rel);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(file), "").unwrap();
        }
    }

    fn env(root: &Path, path_dirs: &[&str]) -> Env {
        let path = std::env::join_paths(path_dirs.iter().map(|d| root.join(d))).unwrap();
        Env::from_pairs(
            [
                ("Path".into(), path),
                ("ProgramFiles".into(), root.join("pf").into_os_string()),
            ],
            NameCase::Insensitive,
        )
    }

    fn vs(root: &Path, name: &str) -> VsInstallation {
        VsInstallation {
            path: root.join(name),
            prerelease: false,
            complete: true,
            toolsets: Vec::new(),
            unusable_toolsets: Vec::new(),
            default_toolset: None,
        }
    }

    fn clangs(found: &[LlvmTool]) -> Vec<(Origin, String)> {
        found
            .iter()
            .filter(|t| t.tool == Tool::Clang)
            .map(|t| {
                (
                    t.origin.clone(),
                    t.version.map(|v| v.to_string()).unwrap_or_default(),
                )
            })
            .collect()
    }

    /// Mutation: skip any of the four places, or search them in another
    /// order; this test fails.
    #[test]
    fn every_place_is_searched_in_the_specified_order() {
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        files(
            r,
            &[
                ("cfg/v23", "clang.exe"),
                ("onpath/v20", "clang.exe"),
                ("pf/LLVM/bin", "clang.exe"),
                ("VS/VC/Tools/Llvm/x64/bin", "clang.exe"),
            ],
        );
        let explicit = ExplicitPaths {
            clang: Some(r.join("cfg/v23/clang.exe")),
            ..Default::default()
        };
        // The fake answers by directory name; give the installer and VS
        // copies recognisable versions by their own directory names.
        let found = find_llvm_tools(
            &env(r, &["onpath/v20"]),
            HostOs::Windows,
            &explicit,
            &[vs(r, "VS")],
            &ByDir,
        )
        .unwrap();
        let origins: Vec<_> = clangs(&found).into_iter().map(|(o, _)| o).collect();
        assert_eq!(
            origins,
            [
                Origin::Configured,
                Origin::Path,
                Origin::LlvmInstall,
                Origin::VisualStudio(r.join("VS")),
            ]
        );
    }

    /// Mutation: drop the de-duplication; this test fails.
    #[test]
    fn the_same_file_found_twice_is_listed_once() {
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        files(r, &[("pf/LLVM/bin", "clang.exe")]);
        // PATH names the installer's directory itself, twice.
        let found = find_llvm_tools(
            &env(r, &["pf/LLVM/bin", "pf/LLVM/bin"]),
            HostOs::Windows,
            &ExplicitPaths::default(),
            &[],
            &ByDir,
        )
        .unwrap();
        assert_eq!(clangs(&found).len(), 1, "{found:?}");
        assert_eq!(found[0].origin, Origin::Path);
    }

    /// Mutation: ignore a configured path that has no file, or fall back to
    /// the other places; this test fails.
    #[test]
    fn a_configured_path_without_a_file_names_the_setting() {
        let root = tempfile::tempdir().unwrap();
        let explicit = ExplicitPaths {
            clang_tidy: Some(root.path().join("nope/clang-tidy.exe")),
            ..Default::default()
        };
        let err = find_llvm_tools(
            &env(root.path(), &[]),
            HostOs::Windows,
            &explicit,
            &[],
            &ByDir,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("[toolchain.paths] clang-tidy"),
            "{err}"
        );
    }

    /// Mutation: look in `%ProgramFiles%\LLVM` or Visual Studio on a non-
    /// Windows host, or add `.exe` there; this test fails.
    #[test]
    fn other_hosts_look_on_path_only_without_a_suffix() {
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        files(
            r,
            &[
                ("onpath/v18", "clang"),
                ("pf/LLVM/bin", "clang"),
                ("VS/VC/Tools/Llvm/x64/bin", "clang"),
            ],
        );
        let found = find_llvm_tools(
            &env(r, &["onpath/v18"]),
            HostOs::Other,
            &ExplicitPaths::default(),
            &[vs(r, "VS")],
            &ByDir,
        )
        .unwrap();
        assert_eq!(clangs(&found), [(Origin::Path, "18.0.0".to_owned())]);
    }

    /// Mutation: read the first three-part number anywhere (a path or hash
    /// before the word `version`), or stop finding tools that print no
    /// version; this test fails.
    #[test]
    fn the_version_follows_the_word_version() {
        struct Banner;
        impl Runner for Banner {
            fn run(&self, _p: &Path, _a: &[&OsStr], _e: &Env) -> std::io::Result<Output> {
                Ok(Output {
                    success: true,
                    stdout: b"LLVM (http://llvm.org/ 1.2.3):\n  LLVM version 22.1.8\n".to_vec(),
                    stderr: Vec::new(),
                })
            }
        }
        let root = tempfile::tempdir().unwrap();
        files(root.path(), &[("bin", "clang-tidy.exe")]);
        let found = find_llvm_tools(
            &env(root.path(), &["bin"]),
            HostOs::Windows,
            &ExplicitPaths::default(),
            &[],
            &Banner,
        )
        .unwrap();
        assert_eq!(found[0].version.unwrap().to_string(), "22.1.8");
    }

    /// Mutation: drop a tool from `Tool::ALL`, or misname its executable;
    /// this test fails.
    #[test]
    fn all_four_tools_are_found() {
        let root = tempfile::tempdir().unwrap();
        files(
            root.path(),
            &[
                ("v22", "clang.exe"),
                ("v22", "clang-cl.exe"),
                ("v22", "clang-format.exe"),
                ("v22", "clang-tidy.exe"),
            ],
        );
        let found = find_llvm_tools(
            &env(root.path(), &["v22"]),
            HostOs::Windows,
            &ExplicitPaths::default(),
            &[],
            &ByDir,
        )
        .unwrap();
        let tools: Vec<_> = found.iter().map(|t| t.tool).collect();
        assert_eq!(tools, Tool::ALL);
    }
}
