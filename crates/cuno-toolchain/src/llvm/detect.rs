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
    /// What `--version` told about it.
    pub probe: Probe,
    /// Where it was found.
    pub origin: Origin,
}

/// What running a tool with `--version` told.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Probe {
    /// It ran and printed a version.
    Ran {
        /// The version on the line with the word `version`.
        version: Version,
        /// Whether the version is a pre-release (`19.1.0-rc1`, `23.0.0git`):
        /// something follows the number in the same word.
        prerelease: bool,
        /// The `Target:` it reports, if any (`x86_64-pc-windows-msvc`).
        target: Option<String>,
    },
    /// It ran but printed no version it could be read from.
    NoVersion,
    /// It could not be started, or exited with an error.
    Failed,
}

impl LlvmTool {
    /// The version, when `--version` gave one.
    pub fn version(&self) -> Option<Version> {
        match &self.probe {
            Probe::Ran { version, .. } => Some(*version),
            _ => None,
        }
    }
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
    dirs.extend(path_dirs(env).into_iter().map(|d| (d, Origin::Path)));
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
        let mut configured = None;
        if let Some(path) = explicit.get(tool) {
            if !path.is_file() {
                return Err(ToolchainError::ConfiguredToolMissing {
                    setting: format!("[toolchain.paths] {}", tool.name()),
                    path: path.to_owned(),
                });
            }
            // Run once, for what it is and for its version.
            let version = run.run(path, &[OsStr::new("--version")], env).ok();
            if let Err(why) = identity(tool, version.as_ref(), path, run, env) {
                return Err(ToolchainError::ConfiguredToolUnusable {
                    setting: format!("[toolchain.paths] {}", tool.name()),
                    path: path.to_owned(),
                    why,
                });
            }
            configured = Some(probe_of(version.as_ref()));
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
            let probe = match (&origin, configured.take()) {
                (Origin::Configured, Some(probe)) => probe,
                _ => probe(&path, run, env),
            };
            found.push(LlvmTool {
                tool,
                probe,
                path,
                origin,
            });
        }
    }
    Ok(found)
}

/// Whether the configured file `path` is `tool` and not another LLVM tool
/// (`docs/en/specs/toolchain.md`, "LLVM (clang / clang-cl / clang-format /
/// clang-tidy)"): its `--version` names the kind, and clang and clang-cl,
/// one program whose mode follows its file name, are told apart by how
/// `/?` answers. One that cannot run is left to the probe, which says so.
fn identity(
    tool: Tool,
    version: Option<&crate::Output>,
    path: &Path,
    run: &dyn Runner,
    env: &Env,
) -> Result<(), String> {
    let Some(output) = version.filter(|o| o.success) else {
        return Ok(());
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let is = if text.contains("clang-format version") {
        Tool::ClangFormat
    } else if text.contains("clang version") {
        // clang-cl answers `/?` with its option list; clang takes it for a
        // file and fails.
        let cl = run
            .run(path, &[OsStr::new("/?")], env)
            .is_ok_and(|o| o.success && o.stdout.starts_with(b"OVERVIEW:"));
        if cl { Tool::ClangCl } else { Tool::Clang }
    } else if text.contains("LLVM (http://llvm.org") || text.contains("LLVM version") {
        // clang-tidy: `LLVM (http://llvm.org/):` then `LLVM version`, or a
        // vendor's `Ubuntu LLVM version` alone.
        Tool::ClangTidy
    } else {
        return Err("it is not an LLVM tool cuno knows".to_owned());
    };
    if is == tool {
        Ok(())
    } else {
        Err(format!("it is {}, not {}", is.name(), tool.name()))
    }
}

/// The directories on `PATH` that are searched: only absolute ones. An empty
/// or relative entry would be resolved against cuno's working directory
/// (often the project), which can hold a file named like a tool.
fn path_dirs(env: &Env) -> Vec<PathBuf> {
    env.get("PATH")
        .map(|path| {
            std::env::split_paths(path)
                .filter(|d| d.is_absolute())
                .collect()
        })
        .unwrap_or_default()
}

/// Runs `<tool> --version`. The version is the first `<n>.<n>.<n>` after the
/// word `version` on the same line (`clang version 22.1.8`, `LLVM version
/// 22.1.8`), so a number on a later line (an install directory) is never
/// taken for it.
fn probe(path: &Path, run: &dyn Runner, env: &Env) -> Probe {
    probe_of(run.run(path, &[OsStr::new("--version")], env).ok().as_ref())
}

/// What a `--version` run told, as [`probe`] reads it; `None` for one that
/// could not be started.
fn probe_of(output: Option<&crate::Output>) -> Probe {
    let Some(output) = output.filter(|o| o.success) else {
        return Probe::Failed;
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let Some(line) = text.lines().find(|l| l.contains("version")) else {
        return Probe::NoVersion;
    };
    let after = &line[line.find("version").unwrap_or(0)..];
    let Some(version) = Version::find_in(after) else {
        return Probe::NoVersion;
    };
    // The word the version is in: anything after the number (`-rc1`, `git`)
    // marks a pre-release.
    let text_version = version.to_string();
    let prerelease = after
        .split_whitespace()
        .find(|word| word.contains(&text_version))
        .is_some_and(|word| word.len() > text_version.len());
    let target = text
        .lines()
        .find_map(|l| l.trim().strip_prefix("Target:"))
        .map(|t| t.trim().to_owned());
    Probe::Ran {
        version,
        prerelease,
        target,
    }
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
                    t.version().map(|v| v.to_string()).unwrap_or_default(),
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
        assert_eq!(found[0].version().unwrap().to_string(), "22.1.8");
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

    /// Answers as each real LLVM tool does, by its file name: what
    /// `--version` prints, and for clang-cl alone, `/?` succeeding with its
    /// option list.
    struct ByName;
    impl Runner for ByName {
        fn run(&self, program: &Path, args: &[&OsStr], _env: &Env) -> std::io::Result<Output> {
            let name = program.file_stem().unwrap().to_string_lossy().into_owned();
            let help = args.first().is_some_and(|a| *a == OsStr::new("/?"));
            let (success, stdout) = match (name.as_str(), help) {
                ("clang-cl", true) => (true, "OVERVIEW: clang LLVM compiler\n"),
                (_, true) => (false, ""),
                ("clang" | "clang-cl", false) => (
                    true,
                    "clang version 22.1.8 (x)\nTarget: x86_64-pc-windows-msvc\n",
                ),
                ("clang-format", false) => (true, "clang-format version 22.1.8 (x)\n"),
                ("clang-tidy", false) => {
                    (true, "LLVM (http://llvm.org/):\n  LLVM version 22.1.8\n")
                }
                // A vendor's clang-tidy names itself on one line.
                ("vendor-tidy", false) => (true, "Ubuntu LLVM version 22.1.0\n"),
                _ => (true, "some tool 1.0.0\n"),
            };
            Ok(Output {
                success,
                stdout: stdout.as_bytes().to_vec(),
                stderr: Vec::new(),
            })
        }
    }

    /// A configured file that is another LLVM tool than its key is refused,
    /// naming what it is; the right tool is taken. clang and clang-cl are
    /// told apart by `/?`, the rest by `--version`.
    ///
    /// Mutation: skip the `/?` check, skip the `--version` kinds, or check
    /// nothing; this test fails.
    #[test]
    fn a_configured_path_must_be_the_tool_its_key_names() {
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        files(
            r,
            &[
                ("a", "clang.exe"),
                ("a", "clang-cl.exe"),
                ("a", "clang-format.exe"),
                ("a", "clang-tidy.exe"),
                ("a", "other.exe"),
                ("a", "vendor-tidy.exe"),
            ],
        );
        let at = |name: &str| Some(r.join("a").join(name));
        let refused = |explicit: ExplicitPaths| match find_llvm_tools(
            &env(r, &[]),
            HostOs::Windows,
            &explicit,
            &[],
            &ByName,
        ) {
            Err(ToolchainError::ConfiguredToolUnusable { setting, why, .. }) => {
                Some(format!("{setting}: {why}"))
            }
            Err(other) => panic!("{other:?}"),
            Ok(_) => None,
        };
        let cases = [
            (
                ExplicitPaths {
                    clang: at("clang-cl.exe"),
                    ..Default::default()
                },
                "[toolchain.paths] clang: it is clang-cl, not clang",
            ),
            (
                ExplicitPaths {
                    clang_cl: at("clang.exe"),
                    ..Default::default()
                },
                "[toolchain.paths] clang-cl: it is clang, not clang-cl",
            ),
            (
                ExplicitPaths {
                    clang: at("clang-format.exe"),
                    ..Default::default()
                },
                "[toolchain.paths] clang: it is clang-format, not clang",
            ),
            (
                ExplicitPaths {
                    clang_format: at("clang-tidy.exe"),
                    ..Default::default()
                },
                "[toolchain.paths] clang-format: it is clang-tidy, not clang-format",
            ),
            (
                ExplicitPaths {
                    clang_tidy: at("other.exe"),
                    ..Default::default()
                },
                "[toolchain.paths] clang-tidy: it is not an LLVM tool cuno knows",
            ),
        ];
        for (explicit, want) in cases {
            assert_eq!(refused(explicit).as_deref(), Some(want));
        }
        let right = ExplicitPaths {
            clang: at("clang.exe"),
            clang_cl: at("clang-cl.exe"),
            clang_format: at("clang-format.exe"),
            clang_tidy: at("clang-tidy.exe"),
        };
        assert_eq!(refused(right), None);
        // A vendor's clang-tidy names itself `Ubuntu LLVM version` alone.
        let vendor = ExplicitPaths {
            clang_tidy: at("vendor-tidy.exe"),
            ..Default::default()
        };
        assert_eq!(refused(vendor), None);
    }

    /// A configured tool's `--version` is run once, for what it is and for
    /// its version, so a tool slow to answer is waited for once.
    ///
    /// Mutation: run `--version` again for the probe; this test fails.
    #[test]
    fn a_configured_tool_is_asked_its_version_once() {
        struct Counting(std::sync::atomic::AtomicUsize);
        impl Runner for Counting {
            fn run(&self, program: &Path, args: &[&OsStr], env: &Env) -> std::io::Result<Output> {
                if args.first().is_some_and(|a| *a == OsStr::new("--version")) {
                    self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
                ByName.run(program, args, env)
            }
        }
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        files(r, &[("a", "clang.exe")]);
        let explicit = ExplicitPaths {
            clang: Some(r.join("a/clang.exe")),
            ..Default::default()
        };
        let counting = Counting(std::sync::atomic::AtomicUsize::new(0));
        let found =
            find_llvm_tools(&env(r, &[]), HostOs::Windows, &explicit, &[], &counting).unwrap();
        assert_eq!(found.len(), 1);
        assert!(matches!(found[0].probe, Probe::Ran { .. }));
        assert_eq!(counting.0.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    /// A runner that answers every `--version` with the same output.
    struct Says(bool, &'static str);
    impl Runner for Says {
        fn run(&self, _p: &Path, _a: &[&OsStr], _e: &Env) -> std::io::Result<Output> {
            Ok(Output {
                success: self.0,
                stdout: self.1.as_bytes().to_vec(),
                stderr: Vec::new(),
            })
        }
    }

    /// A runner whose programs cannot be started.
    struct CannotStart;
    impl Runner for CannotStart {
        fn run(&self, _p: &Path, _a: &[&OsStr], _e: &Env) -> std::io::Result<Output> {
            Err(std::io::Error::other("cannot start"))
        }
    }

    fn one_clang(run: &dyn Runner) -> LlvmTool {
        let root = tempfile::tempdir().unwrap();
        files(root.path(), &[("bin", "clang.exe")]);
        find_llvm_tools(
            &env(root.path(), &["bin"]),
            HostOs::Windows,
            &ExplicitPaths::default(),
            &[],
            run,
        )
        .unwrap()
        .remove(0)
    }

    /// Mutation: keep empty or relative `PATH` entries; this test fails.
    #[test]
    fn only_absolute_path_entries_are_searched() {
        let root = tempfile::tempdir().unwrap();
        let absolute = root.path().join("bin");
        let path = std::env::join_paths([
            std::path::PathBuf::new(),
            ".".into(),
            "relative/bin".into(),
            absolute.clone(),
        ])
        .unwrap();
        let env = Env::from_pairs([("PATH".into(), path)], NameCase::Insensitive);
        assert_eq!(path_dirs(&env), [absolute]);
    }

    /// Mutation: read a version from a run that failed, or drop a tool that
    /// could not be started instead of recording it as failed; this test
    /// fails.
    #[test]
    fn failing_tools_are_recorded_as_failed() {
        assert_eq!(
            one_clang(&Says(false, "clang version 22.1.8\n")).probe,
            Probe::Failed
        );
        assert_eq!(one_clang(&CannotStart).probe, Probe::Failed);
        assert_eq!(one_clang(&Says(true, "garbage\n")).probe, Probe::NoVersion);
    }

    /// Mutation: search for the version past the line with the word
    /// `version`; this test fails (the install directory holds `9.9.9`).
    #[test]
    fn the_version_is_read_from_its_own_line() {
        let tool = one_clang(&Says(
            true,
            "clang version trunk\nInstalledDir: C:/llvm-9.9.9/bin\n",
        ));
        assert_eq!(tool.probe, Probe::NoVersion);
    }

    /// Mutation: ignore the text after the number, or drop the `Target:`
    /// line; this test fails.
    #[test]
    fn prereleases_and_targets_are_recorded() {
        let release = one_clang(&Says(
            true,
            "clang version 22.1.8 (https://x)\nTarget: x86_64-pc-windows-msvc\n",
        ));
        assert_eq!(
            release.probe,
            Probe::Ran {
                version: Version {
                    major: 22,
                    minor: 1,
                    patch: 8
                },
                prerelease: false,
                target: Some("x86_64-pc-windows-msvc".into()),
            }
        );
        for banner in [
            "clang version 19.1.0-rc1 (x)\n",
            "clang version 23.0.0git (x)\n",
        ] {
            let Probe::Ran {
                prerelease, target, ..
            } = one_clang(&Says(true, banner)).probe
            else {
                panic!("{banner}");
            };
            assert!(prerelease, "{banner}");
            assert_eq!(target, None);
        }
        let mingw = one_clang(&Says(
            true,
            "clang version 21.1.0\nTarget: x86_64-w64-windows-gnu\n",
        ));
        assert!(
            matches!(mingw.probe, Probe::Ran { target: Some(t), .. } if t == "x86_64-w64-windows-gnu")
        );
    }

    /// Mutation: map any `[toolchain.paths]` field to another tool; this test
    /// fails.
    #[test]
    fn each_configured_path_belongs_to_its_own_tool() {
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        files(
            r,
            &[
                ("a", "clang.exe"),
                ("b", "clang-cl.exe"),
                ("c", "clang-format.exe"),
                ("d", "clang-tidy.exe"),
            ],
        );
        let explicit = ExplicitPaths {
            clang: Some(r.join("a/clang.exe")),
            clang_cl: Some(r.join("b/clang-cl.exe")),
            clang_format: Some(r.join("c/clang-format.exe")),
            clang_tidy: Some(r.join("d/clang-tidy.exe")),
        };
        let found =
            find_llvm_tools(&env(r, &[]), HostOs::Windows, &explicit, &[], &ByName).unwrap();
        let got: Vec<_> = found
            .iter()
            .map(|t| {
                (
                    t.tool,
                    t.path
                        .parent()
                        .unwrap()
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    t.origin.clone(),
                )
            })
            .collect();
        assert_eq!(
            got,
            [
                (Tool::Clang, "a".to_owned(), Origin::Configured),
                (Tool::ClangCl, "b".to_owned(), Origin::Configured),
                (Tool::ClangFormat, "c".to_owned(), Origin::Configured),
                (Tool::ClangTidy, "d".to_owned(), Origin::Configured),
            ]
        );
    }

    /// Mutation: accept a configured directory (`exists` instead of
    /// `is_file`); this test fails.
    #[test]
    fn a_configured_directory_is_not_a_tool() {
        let root = tempfile::tempdir().unwrap();
        let explicit = ExplicitPaths {
            clang: Some(root.path().to_owned()),
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
            matches!(err, ToolchainError::ConfiguredToolMissing { .. }),
            "{err}"
        );
    }

    /// Mutation: compare paths as written instead of canonicalized; this test
    /// fails (`pf/../pf/LLVM/bin` and `pf/LLVM/bin` are one directory).
    #[test]
    fn the_same_file_spelled_differently_is_listed_once() {
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        files(r, &[("pf/LLVM/bin", "clang.exe")]);
        let found = find_llvm_tools(
            &env(r, &["pf/../pf/LLVM/bin"]),
            HostOs::Windows,
            &ExplicitPaths::default(),
            &[],
            &ByDir,
        )
        .unwrap();
        assert_eq!(clangs(&found).len(), 1, "{found:?}");
    }

    /// Mutation: search only the first Visual Studio, reverse them, or tag
    /// every bundled Clang with the first installation; this test fails.
    #[test]
    fn every_visual_studio_is_searched_in_order() {
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        files(
            r,
            &[
                ("VS1/VC/Tools/Llvm/x64/bin", "clang.exe"),
                ("VS2/VC/Tools/Llvm/x64/bin", "clang.exe"),
            ],
        );
        let found = find_llvm_tools(
            &env(r, &[]),
            HostOs::Windows,
            &ExplicitPaths::default(),
            &[vs(r, "VS1"), vs(r, "VS2")],
            &ByDir,
        )
        .unwrap();
        let origins: Vec<_> = clangs(&found).into_iter().map(|(o, _)| o).collect();
        assert_eq!(
            origins,
            [
                Origin::VisualStudio(r.join("VS1")),
                Origin::VisualStudio(r.join("VS2"))
            ]
        );
    }
}
