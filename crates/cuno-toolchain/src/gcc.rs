//! MinGW GCC: finding `gcc` (and the `g++` beside it). See
//! `docs/en/specs/toolchain.md`, "MinGW (GCC)".

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use cuno_core::Env;

use crate::{HostOs, Runner, ToolchainError, Version};

/// The MSYS2 directories searched on Windows after `PATH`, in order: the
/// UCRT64 environment, then MINGW64.
const MSYS2_DIRS: [&str; 2] = [r"C:\msys64\ucrt64\bin", r"C:\msys64\mingw64\bin"];

/// Where a copy of GCC was found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    /// The path written in `[toolchain.paths] gcc`.
    Configured,
    /// A directory on `PATH`.
    Path,
    /// One of MSYS2's standard directories.
    Msys2,
}

/// One copy of GCC.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gcc {
    /// `gcc`; its `g++` is the one in the same directory.
    pub path: PathBuf,
    /// What running it told.
    pub probe: Probe,
    /// Where it was found.
    pub origin: Origin,
}

impl Gcc {
    /// The version, when it gave one.
    pub fn version(&self) -> Option<Version> {
        match &self.probe {
            Probe::Ran { version, .. } => Some(*version),
            _ => None,
        }
    }

    /// The `g++` beside it (`docs/en/specs/toolchain.md`, "MinGW (GCC)").
    pub fn cxx(&self, host: HostOs) -> PathBuf {
        self.path.with_file_name(host.executable("g++"))
    }
}

/// What running a copy of GCC told.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Probe {
    /// It ran and gave its version.
    Ran {
        /// The version, the first `<n>.<n>.<n>` on `--version`'s first line
        /// (`gcc.exe (Rev2, Built by MSYS2 project) 14.2.0`).
        version: Version,
        /// Whether that line marks a snapshot (`experimental`, `prerelease`).
        prerelease: bool,
        /// What `-dumpmachine` printed (`x86_64-w64-mingw32`), if anything.
        target: Option<String>,
    },
    /// It ran but gave no version that could be read.
    NoVersion,
    /// It could not be started, or exited with an error.
    Failed,
}

/// Every copy of GCC, in the spec's search order: the configured path,
/// `PATH` (absolute entries only), then MSYS2's standard directories (on
/// Windows). The same file found twice is listed once. Not finding GCC is
/// not an error; a configured path with no file is.
pub fn find_gcc(
    env: &Env,
    host: HostOs,
    explicit: Option<&Path>,
    run: &dyn Runner,
) -> Result<Vec<Gcc>, ToolchainError> {
    let msys2: Vec<PathBuf> = MSYS2_DIRS.iter().map(PathBuf::from).collect();
    find_in(env, host, explicit, &msys2, run)
}

/// [`find_gcc`] with MSYS2's directories given, so a test can stand them in.
fn find_in(
    env: &Env,
    host: HostOs,
    explicit: Option<&Path>,
    msys2: &[PathBuf],
    run: &dyn Runner,
) -> Result<Vec<Gcc>, ToolchainError> {
    let mut candidates: Vec<(PathBuf, Origin)> = Vec::new();
    if let Some(path) = explicit {
        if !path.is_file() {
            return Err(ToolchainError::ConfiguredToolMissing {
                setting: "[toolchain.paths] gcc".to_owned(),
                path: path.to_owned(),
            });
        }
        candidates.push((path.to_owned(), Origin::Configured));
    }
    let file = host.executable("gcc");
    let mut dirs: Vec<(PathBuf, Origin)> = path_dirs(env)
        .into_iter()
        .map(|dir| (dir, Origin::Path))
        .collect();
    if host == HostOs::Windows {
        dirs.extend(msys2.iter().map(|d| (d.clone(), Origin::Msys2)));
    }
    candidates.extend(
        dirs.into_iter()
            .map(|(dir, origin)| (dir.join(&file), origin))
            .filter(|(path, _)| path.is_file()),
    );
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut found = Vec::new();
    for (path, origin) in candidates {
        let identity = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if seen.contains(&identity) {
            continue;
        }
        seen.push(identity);
        let probe = probe(&path, run, env);
        found.push(Gcc {
            path,
            probe,
            origin,
        });
    }
    Ok(found)
}

/// The absolute directories on `PATH`, as LLVM's detection reads them: an
/// empty or relative entry would name cuno's working directory.
fn path_dirs(env: &Env) -> Vec<PathBuf> {
    env.get("PATH")
        .map(|path| {
            std::env::split_paths(path)
                .filter(|d| d.is_absolute())
                .collect()
        })
        .unwrap_or_default()
}

/// Runs `gcc --version` for the version, and `gcc -dumpmachine` for the
/// target.
fn probe(path: &Path, run: &dyn Runner, env: &Env) -> Probe {
    let Some(output) = run
        .run(path, &[OsStr::new("--version")], env)
        .ok()
        .filter(|o| o.success)
    else {
        return Probe::Failed;
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let first = text.lines().next().unwrap_or_default();
    let Some(version) = Version::find_in(first) else {
        return Probe::NoVersion;
    };
    let prerelease = first.contains("experimental") || first.contains("prerelease");
    let target = run
        .run(path, &[OsStr::new("-dumpmachine")], env)
        .ok()
        .filter(|o| o.success)
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .filter(|t| !t.is_empty());
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

    /// Answers as the GCC in each directory would: `--version` with the
    /// version in the directory's name (`.../v14/gcc.exe` is 14.2.0, `exp`
    /// an experimental 15.0.0), and `-dumpmachine` with MinGW's target, or
    /// another for a directory named `linux`.
    struct ByDir;

    impl Runner for ByDir {
        fn run(&self, program: &Path, args: &[&OsStr], _env: &Env) -> std::io::Result<Output> {
            let dir = program
                .parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let out = if args == [OsStr::new("-dumpmachine")] {
                if dir == "linux" {
                    "x86_64-linux-gnu\n".to_owned()
                } else {
                    "x86_64-w64-mingw32\n".to_owned()
                }
            } else if dir == "linux" {
                "gcc (Ubuntu 13.2.0-23ubuntu4) 13.2.0\n".to_owned()
            } else if dir == "exp" {
                "gcc.exe (GCC) 15.0.0 20260101 (experimental)\nCopyright\n".to_owned()
            } else if dir == "broken" {
                return Ok(Output {
                    success: false,
                    stdout: Vec::new(),
                    stderr: b"boom".to_vec(),
                });
            } else {
                let major = dir.trim_start_matches('v');
                format!("gcc.exe (Rev2, Built by MSYS2 project) {major}.2.0\nCopyright 3.1.4\n")
            };
            Ok(Output {
                success: true,
                stdout: out.into_bytes(),
                stderr: Vec::new(),
            })
        }
    }

    fn gcc_in(dir: &Path) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join("gcc.exe");
        std::fs::write(&path, "").unwrap();
        path
    }

    fn env_with_path(dirs: &[&Path]) -> Env {
        let path = std::env::join_paths(dirs).unwrap();
        Env::from_pairs([("PATH".into(), path)], NameCase::Insensitive)
    }

    /// The configured path comes first, then `PATH` in order, and a file
    /// reached twice is listed once; the version is read from `--version`'s
    /// first line, never a later one, and the target from `-dumpmachine`.
    ///
    /// Mutation: search `PATH` before the configured path, list a file
    /// twice, or read the version from a later line; this test fails.
    #[test]
    fn copies_are_found_in_order_once_each() {
        let root = tempfile::tempdir().unwrap();
        let configured = gcc_in(&root.path().join("v13"));
        let on_path = gcc_in(&root.path().join("v14"));
        let env = env_with_path(&[
            on_path.parent().unwrap(),
            configured.parent().unwrap(),
            Path::new("relative"),
        ]);
        let found = find_in(&env, HostOs::Windows, Some(&configured), &[], &ByDir).unwrap();
        let listed: Vec<(&Path, &Origin, Option<Version>)> = found
            .iter()
            .map(|g| (g.path.as_path(), &g.origin, g.version()))
            .collect();
        assert_eq!(
            listed,
            [
                (
                    configured.as_path(),
                    &Origin::Configured,
                    Version::find_in("13.2.0")
                ),
                (on_path.as_path(), &Origin::Path, Version::find_in("14.2.0")),
            ]
        );
        assert!(matches!(
            &found[0].probe,
            Probe::Ran { target: Some(t), prerelease: false, .. } if t == "x86_64-w64-mingw32"
        ));
    }

    /// A snapshot is a pre-release, another target is recorded as such,
    /// and one that fails is recorded, not dropped.
    ///
    /// Mutation: miss `experimental`, drop the target, or leave out a copy
    /// that fails; this test fails.
    #[test]
    fn what_each_copy_is_is_recorded() {
        let root = tempfile::tempdir().unwrap();
        let exp = gcc_in(&root.path().join("exp"));
        let linux = gcc_in(&root.path().join("linux"));
        let broken = gcc_in(&root.path().join("broken"));
        let env = env_with_path(&[
            exp.parent().unwrap(),
            linux.parent().unwrap(),
            broken.parent().unwrap(),
        ]);
        let found = find_in(&env, HostOs::Windows, None, &[], &ByDir).unwrap();
        assert_eq!(found.len(), 3);
        assert!(matches!(
            found[0].probe,
            Probe::Ran {
                prerelease: true,
                ..
            }
        ));
        assert!(matches!(
            &found[1].probe,
            Probe::Ran { target: Some(t), .. } if t == "x86_64-linux-gnu"
        ));
        assert_eq!(found[2].probe, Probe::Failed);
    }

    /// A configured path with no file is an error naming the setting.
    ///
    /// Mutation: skip a missing configured file; this test fails.
    #[test]
    fn a_configured_path_with_no_file_is_an_error() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("nowhere").join("gcc.exe");
        let env = env_with_path(&[]);
        let err = find_in(&env, HostOs::Windows, Some(&missing), &[], &ByDir).unwrap_err();
        assert!(
            matches!(&err, ToolchainError::ConfiguredToolMissing { setting, .. } if setting == "[toolchain.paths] gcc"),
            "{err:?}"
        );
    }

    /// On Windows MSYS2's directories come after `PATH`, in the order
    /// given, UCRT64 first; elsewhere they are not searched. `g++` is the
    /// one beside the `gcc`.
    ///
    /// Mutation: search MSYS2 before `PATH`, swap the two MSYS2
    /// directories, search them off Windows, or look for `g++` elsewhere;
    /// this test fails.
    #[test]
    fn msys2_comes_after_path_and_g_plus_plus_is_beside_gcc() {
        assert_eq!(
            MSYS2_DIRS,
            [r"C:\msys64\ucrt64\bin", r"C:\msys64\mingw64\bin"]
        );
        let root = tempfile::tempdir().unwrap();
        let ucrt = gcc_in(&root.path().join("v14"));
        let mingw = gcc_in(&root.path().join("v12"));
        let on_path = gcc_in(&root.path().join("v13"));
        let env = env_with_path(&[on_path.parent().unwrap()]);
        let msys2 = [
            ucrt.parent().unwrap().to_owned(),
            mingw.parent().unwrap().to_owned(),
        ];
        let found = find_in(&env, HostOs::Windows, None, &msys2, &ByDir).unwrap();
        let order: Vec<(&Path, &Origin)> = found
            .iter()
            .map(|g| (g.path.as_path(), &g.origin))
            .collect();
        assert_eq!(
            order,
            [
                (on_path.as_path(), &Origin::Path),
                (ucrt.as_path(), &Origin::Msys2),
                (mingw.as_path(), &Origin::Msys2),
            ]
        );
        // Off Windows the file is `gcc`, and MSYS2 is not searched.
        std::fs::write(on_path.with_file_name("gcc"), "").unwrap();
        let elsewhere = find_in(&env, HostOs::Other, None, &msys2, &ByDir).unwrap();
        assert_eq!(elsewhere.len(), 1);
        assert_eq!(
            found[1].cxx(HostOs::Windows),
            ucrt.with_file_name("g++.exe")
        );
    }
}
