//! MinGW GCC: finding `gcc` (and the `g++` beside it). See
//! `docs/en/specs/toolchain.md`, "MinGW (GCC)".
//!
//! Nothing here runs unless GCC is wanted: the detection of every build
//! (`prepare::detect`) does not call it, so an MSVC or Clang build, `cuno
//! fmt` and `cuno lint` never start a GCC.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use cuno_core::Env;

use crate::{HostOs, Runner, ToolchainError, Version};

/// The MSYS2 directories searched on Windows after `PATH`, in order: the
/// UCRT64 environment, then MINGW64 (whose GCC is found, and refused for
/// its C runtime, so the error can say why).
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

/// What running a copy of GCC told, each from output no locale changes
/// (`docs/en/specs/toolchain.md`, "MinGW (GCC)").
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Probe {
    /// It ran and gave its version.
    Ran {
        /// What `-dumpfullversion` printed (`14.2.0`).
        version: Version,
        /// Whether `--version`'s first line marks a snapshot
        /// (`experimental`, `prerelease`).
        prerelease: bool,
        /// What `-dumpmachine` printed (`x86_64-w64-mingw32`), if anything.
        target: Option<String>,
        /// Whether its C runtime is UCRT: its `_mingw.h` defines `_UCRT`.
        ucrt: bool,
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
        let probe = probe(&path, host, run, env);
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

/// What `gcc` prints for `args`, when it ran and succeeded.
/// It runs in its own directory, which holds no headers: `-include` looks
/// in the working directory first, and cuno's is the project's.
fn ask(path: &Path, args: &[&str], run: &dyn Runner, env: &Env) -> Option<String> {
    let args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    let cwd = path.parent().unwrap_or(Path::new("."));
    run.run_in(path, &args, env, cwd)
        .ok()
        .filter(|o| o.success)
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
}

/// `env` with the directory holding `gcc` first on `PATH`: its compiler
/// proper loads DLLs from there, and fails silently when it is not on
/// `PATH` (`docs/en/specs/toolchain.md`, "MinGW (GCC)").
pub fn env_for(gcc: &Path, env: &Env) -> Env {
    let Some(dir) = gcc.parent() else {
        return env.clone();
    };
    let rest = env
        .get("PATH")
        .map(|p| std::env::split_paths(p).collect::<Vec<_>>())
        .unwrap_or_default();
    match std::env::join_paths(std::iter::once(dir.to_owned()).chain(rest)) {
        Ok(path) => env.with("PATH", path),
        Err(_) => env.clone(),
    }
}

/// Runs `gcc` for its version, whether it is a snapshot, its target and
/// its C runtime, with its own directory first on `PATH`.
fn probe(path: &Path, host: HostOs, run: &dyn Runner, env: &Env) -> Probe {
    let env = &env_for(path, env);
    let Some(full) = ask(path, &["-dumpfullversion"], run, env) else {
        return Probe::Failed;
    };
    let Some(version) = Version::find_in(full.trim()) else {
        return Probe::NoVersion;
    };
    let banner = ask(path, &["--version"], run, env).unwrap_or_default();
    let first = banner.lines().next().unwrap_or_default();
    let prerelease = first.contains("experimental") || first.contains("prerelease");
    let target = ask(path, &["-dumpmachine"], run, env)
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty());
    let empty_input = match host {
        HostOs::Windows => "NUL",
        HostOs::Other => "/dev/null",
    };
    let args = ["-dM", "-E", "-x", "c", "-include", "_mingw.h", empty_input];
    // A GCC that cannot preprocess cannot build either; calling it MSVCRT
    // would send its user to install the GCC they already have.
    let Some(macros) = ask(path, &args, run, env) else {
        return Probe::Failed;
    };
    let ucrt = macros.lines().any(|l| {
        let mut words = l.split_whitespace();
        words.next() == Some("#define") && words.next() == Some("_UCRT")
    });
    Probe::Ran {
        version,
        prerelease,
        target,
        ucrt,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Output;
    use cuno_core::NameCase;

    /// Answers as the GCC in each directory would, by the directory's name:
    /// `v<major>` is `<major>.2.0`, UCRT, MinGW; `msvcrt` lacks `_UCRT`;
    /// as a real one, any of them gives `_UCRT` only from `-include
    /// _mingw.h`, run with its own directory first on `PATH`;
    /// `exp` and `pre` are snapshots; `linux` targets Linux; `nover` gives
    /// no version; `nomachine` fails `-dumpmachine`; `blank` prints an empty
    /// target; `nocc1` cannot preprocess; `broken` fails everything.
    struct ByDir;

    impl Runner for ByDir {
        /// As if in a project whose root holds a `_mingw.h` of its own.
        fn run(&self, program: &Path, args: &[&OsStr], env: &Env) -> std::io::Result<Output> {
            self.run_in(program, args, env, Path::new("project"))
        }

        fn run_in(
            &self,
            program: &Path,
            args: &[&OsStr],
            env: &Env,
            cwd: &Path,
        ) -> std::io::Result<Output> {
            let dir = program
                .parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let arg = args[0].to_string_lossy().into_owned();
            let ok = |text: &str| Output {
                success: true,
                stdout: text.as_bytes().to_vec(),
                stderr: Vec::new(),
            };
            let failed = Output {
                success: false,
                stdout: Vec::new(),
                stderr: b"boom".to_vec(),
            };
            if dir == "broken" {
                return Ok(failed);
            }
            Ok(match arg.as_str() {
                "-dumpfullversion" => match dir.as_str() {
                    "nover" => ok("unknown\n"),
                    "exp" | "pre" => ok("15.0.1\n"),
                    "linux" | "msvcrt" | "nomachine" | "blank" | "nocc1" => ok("13.2.0\n"),
                    _ => ok(&format!("{}.2.0\n", dir.trim_start_matches('v'))),
                },
                "--version" => match dir.as_str() {
                    "exp" => ok("gcc.exe (GCC) 15.0.1 20260101 (experimental)\n"),
                    "pre" => ok("gcc.exe (GCC) 15.0.1 20260101 (prerelease)\n"),
                    // A vendor name with a number of its own, which is not
                    // the version.
                    _ => ok("gcc.exe (Buildroot 2023.02.1) 99.9.9\nCopyright 3.1.4\n"),
                },
                "-dumpmachine" => match dir.as_str() {
                    "linux" => ok("x86_64-linux-gnu\n"),
                    "nomachine" => failed,
                    "blank" => ok("\n"),
                    _ => ok("x86_64-w64-mingw32\n"),
                },
                "-dM" => {
                    let first_on_path = env
                        .get("PATH")
                        .and_then(|p| std::env::split_paths(p).next());
                    let header = args.windows(2).any(|w| w == ["-include", "_mingw.h"]);
                    if dir == "nocc1" || first_on_path.as_deref() != program.parent() {
                        // Its compiler proper finds no DLLs: exit 1, silent.
                        Output {
                            success: false,
                            stdout: Vec::new(),
                            stderr: Vec::new(),
                        }
                    } else if cwd != program.parent().unwrap() {
                        // The project's `_mingw.h` was read instead.
                        ok("#define __MINGW64__ 1\n")
                    } else if !header || matches!(dir.as_str(), "msvcrt" | "linux") {
                        ok("#define __MINGW64__ 1\n#define _UCRTX 1\n")
                    } else {
                        // No trailing space, which the real one prints.
                        ok("#define __MINGW64__ 1\n#define _UCRT\n")
                    }
                }
                _ => failed,
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

    fn find(env: &Env, explicit: Option<&Path>) -> Vec<Gcc> {
        find_in(env, HostOs::Windows, explicit, &[], &ByDir).unwrap()
    }

    /// The configured path comes first, then `PATH` in order; a file
    /// reached twice, even by another spelling, is listed once; the version
    /// is `-dumpfullversion`'s, never a number in the banner; a UCRT GCC
    /// whose directory is not first on `PATH` is still found to be UCRT.
    ///
    /// Mutation: search `PATH` before the configured path, compare paths as
    /// written, read the version from the banner, check `_UCRT` without
    /// `-include _mingw.h`, run GCC with the caller's `PATH`, or run it in
    /// the project (whose `_mingw.h` shadows GCC's); this test fails.
    #[test]
    fn copies_are_found_in_order_once_each() {
        let root = tempfile::tempdir().unwrap();
        let configured = gcc_in(&root.path().join("v13"));
        let on_path = gcc_in(&root.path().join("v14"));
        let respelled = root.path().join("v14").join("..").join("v14");
        let env = env_with_path(&[
            on_path.parent().unwrap(),
            &respelled,
            configured.parent().unwrap(),
        ]);
        let found = find(&env, Some(&configured));
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
        assert_eq!(
            found[0].probe,
            Probe::Ran {
                version: Version::find_in("13.2.0").unwrap(),
                prerelease: false,
                target: Some("x86_64-w64-mingw32".to_owned()),
                ucrt: true,
            }
        );
    }

    /// Snapshots by either word, another target, no version, a run that
    /// fails, a failed or empty `-dumpmachine`, a C runtime other than UCRT,
    /// and a GCC that cannot preprocess (failed, not MSVCRT) are each
    /// recorded as such, not dropped.
    ///
    /// Mutation: miss `experimental` or `prerelease`, keep an empty target,
    /// ignore `-dumpmachine` failing, record no version as failed, take
    /// `_UCRTX` for `_UCRT`, require a space after `_UCRT`, record a GCC that
    /// cannot preprocess as MSVCRT, or leave a copy out; this test fails.
    #[test]
    fn what_each_copy_is_is_recorded() {
        let root = tempfile::tempdir().unwrap();
        let names = [
            "exp",
            "pre",
            "linux",
            "nover",
            "broken",
            "nomachine",
            "blank",
            "msvcrt",
            "nocc1",
            "v15",
        ];
        let dirs: Vec<PathBuf> = names
            .iter()
            .map(|n| gcc_in(&root.path().join(n)).parent().unwrap().to_owned())
            .collect();
        let env = env_with_path(&dirs.iter().map(PathBuf::as_path).collect::<Vec<_>>());
        let found = find(&env, None);
        assert_eq!(found.len(), names.len());
        let ran = |i: usize| match &found[i].probe {
            Probe::Ran {
                prerelease,
                target,
                ucrt,
                ..
            } => (*prerelease, target.clone(), *ucrt),
            other => panic!("{}: {other:?}", names[i]),
        };
        assert!(ran(0).0, "experimental");
        assert!(ran(1).0, "prerelease");
        assert_eq!(ran(2).1.as_deref(), Some("x86_64-linux-gnu"));
        assert_eq!(found[3].probe, Probe::NoVersion);
        assert_eq!(found[4].probe, Probe::Failed);
        assert_eq!(ran(5).1, None, "a failed -dumpmachine");
        assert_eq!(ran(6).1, None, "an empty -dumpmachine");
        assert!(!ran(7).2, "MSVCRT");
        assert_eq!(found[8].probe, Probe::Failed, "cannot preprocess");
        assert!(ran(9).2, "`#define _UCRT` with nothing after it");
    }

    /// GCC's directory goes in front of `PATH`, and the rest stays, in order.
    ///
    /// Mutation: drop the rest of `PATH`, or put the directory last; this
    /// test fails.
    #[test]
    fn gcc_s_directory_goes_in_front_of_path() {
        let root = tempfile::tempdir().unwrap();
        let (a, b) = (root.path().join("a"), root.path().join("b"));
        let gcc = root.path().join("gcc").join("gcc.exe");
        let env = env_for(&gcc, &env_with_path(&[&a, &b]));
        let path: Vec<PathBuf> = std::env::split_paths(env.get("PATH").unwrap()).collect();
        assert_eq!(path, [root.path().join("gcc"), a, b]);
    }

    /// A configured path with no file is an error naming the setting.
    ///
    /// Mutation: skip a missing configured file; this test fails.
    #[test]
    fn a_configured_path_with_no_file_is_an_error() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("nowhere").join("gcc.exe");
        let err = find_in(
            &env_with_path(&[]),
            HostOs::Windows,
            Some(&missing),
            &[],
            &ByDir,
        )
        .unwrap_err();
        assert!(
            matches!(&err, ToolchainError::ConfiguredToolMissing { setting, .. } if setting == "[toolchain.paths] gcc"),
            "{err:?}"
        );
    }

    /// Only absolute `PATH` entries are searched.
    ///
    /// Mutation: keep a relative entry; this test fails.
    #[test]
    fn only_absolute_path_entries_are_searched() {
        let root = tempfile::tempdir().unwrap();
        let env = env_with_path(&[Path::new("relative"), root.path()]);
        assert_eq!(path_dirs(&env), [root.path().to_owned()]);
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
        // Off Windows the file is `gcc`, and MSYS2 is not searched, though
        // its directories hold one.
        for gcc in [&on_path, &ucrt, &mingw] {
            std::fs::write(gcc.with_file_name("gcc"), "").unwrap();
        }
        let elsewhere = find_in(&env, HostOs::Other, None, &msys2, &ByDir).unwrap();
        assert_eq!(elsewhere.len(), 1);
        assert_eq!(
            found[1].cxx(HostOs::Windows),
            ucrt.with_file_name("g++.exe")
        );
    }
}
