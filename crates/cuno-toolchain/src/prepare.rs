//! What every command that compiles does before it compiles: find the
//! compilers and tools, then, for the selected compiler, resolve the
//! environment it runs in. `cuno doctor` and the build commands both call
//! these, so they diagnose and build with the same answers.

use std::ffi::OsStr;
use std::path::PathBuf;

use cuno_core::Env;

use crate::compiler::CompilerKind;
use crate::llvm::{ExplicitPaths, LlvmTool, find_llvm_tools};
use crate::msvc::{self, Registry, VsInstallation, WindowsSdk};
use crate::select::Selected;
use crate::{HostOs, Runner, ToolchainError};

/// What detection found. Each part is a result of its own, so a caller that
/// reports problems (`cuno doctor`) keeps going after one fails.
#[derive(Debug)]
pub struct Detected {
    /// Every Visual Studio installation; empty on a host that has none.
    pub installs: Result<Vec<VsInstallation>, ToolchainError>,
    /// Every LLVM tool found, in search order.
    pub llvm: Result<Vec<LlvmTool>, ToolchainError>,
}

/// Finds Visual Studio (on Windows) and the LLVM tools, the copies
/// `explicit` names first. LLVM is searched even when Visual Studio is not
/// found, since its bundled LLVM is only one of the places looked in.
pub fn detect(env: &Env, host: HostOs, explicit: &ExplicitPaths, run: &dyn Runner) -> Detected {
    let installs = match host {
        HostOs::Windows => msvc::find_installations(env, run),
        HostOs::Other => Ok(Vec::new()),
    };
    let found = installs.as_deref().unwrap_or(&[]);
    let llvm = find_llvm_tools(env, host, explicit, found, run);
    Detected { installs, llvm }
}

/// Variables compilers and linkers read options or include paths from by
/// themselves, which would add flags no command line shows
/// (`toolchain.md`, "How it is resolved").
pub const TOOL_OPTION_VARIABLES: [&str; 7] = [
    "CL",
    "_CL_",
    "LINK",
    "_LINK_",
    "CPATH",
    "C_INCLUDE_PATH",
    "CPLUS_INCLUDE_PATH",
];

/// The environment the selected compiler runs in.
#[derive(Clone, Debug)]
pub struct Prepared {
    /// The process environment with the MSVC toolset's and the Windows SDK's
    /// paths applied, or as given when no toolset is used.
    pub env: Env,
    /// The Windows SDK, when a toolset is used.
    pub sdk: Option<WindowsSdk>,
    /// The [`TOOL_OPTION_VARIABLES`] that were set and are left out of
    /// `env`, for the build to warn about.
    pub ignored: Vec<&'static str>,
}

impl Prepared {
    /// The include directories the compiler searches for the system and
    /// standard headers: the resolved `INCLUDE`, in order, when the
    /// environment was prepared for an MSVC toolset, else none, whatever the
    /// process's own `INCLUDE` says. The one place an MSVC environment's
    /// include path is read, so a compiler with no toolset never takes it as
    /// its include path (compile_commands.json, the build records,
    /// clang-tidy).
    pub fn include_dirs(&self) -> Vec<PathBuf> {
        match self.sdk {
            Some(_) => crate::flags::include_dirs(self.env.get("INCLUDE")),
            None => Vec::new(),
        }
    }

    /// What besides each compile's arguments identifies its command: the
    /// compiler's `version`, and the variables of the environment that
    /// decide which headers and libraries it reads (`build.md`, "Deciding
    /// what changed").
    pub fn identity<'a>(&'a self, version: &'a str) -> [&'a OsStr; 4] {
        let var = |name| self.env.get(name).unwrap_or_default();
        [
            OsStr::new(version),
            var("INCLUDE"),
            var("LIB"),
            var("LIBPATH"),
        ]
    }
}

/// Resolves the environment for `selected`: with an MSVC toolset (every
/// compiler on Windows but GCC), the Windows SDK is found and both are
/// applied to `env` (`docs/en/specs/toolchain.md`, "Environment resolution
/// on Windows"); GCC gets its own directory first on `PATH`, which its
/// compiler proper loads DLLs from (`toolchain.md`, "MinGW (GCC)").
pub fn prepare(
    selected: &Selected,
    env: &Env,
    registry: &dyn Registry,
) -> Result<Prepared, ToolchainError> {
    let ignored: Vec<&'static str> = TOOL_OPTION_VARIABLES
        .into_iter()
        .filter(|name| env.get(name).is_some())
        .collect();
    let clean = env.without(&TOOL_OPTION_VARIABLES);
    let Some(toolset) = &selected.msvc else {
        let env = match selected.kind {
            CompilerKind::Gcc => crate::gcc::env_for(&selected.executable, &clean),
            CompilerKind::Msvc | CompilerKind::Clang | CompilerKind::ClangCl => clean,
        };
        return Ok(Prepared {
            env,
            sdk: None,
            ignored,
        });
    };
    let sdk = msvc::find_windows_sdk(env, registry)?;
    Ok(Prepared {
        env: msvc::resolve(toolset, &sdk).apply(&clean),
        sdk: Some(sdk),
        ignored,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Version;
    use crate::msvc::MsvcToolset;

    use crate::select::Source;
    use cuno_core::NameCase;
    use std::ffi::{OsStr, OsString};
    use std::path::{Path, PathBuf};

    /// The include directories are the prepared `INCLUDE`, in order, only
    /// when the environment was prepared for a toolset; without one, an
    /// `INCLUDE` the process had is not the compiler's.
    ///
    /// Mutation: read `INCLUDE` whether or not a toolset was used, or lose
    /// the order; this test fails.
    #[test]
    fn include_dirs_are_the_toolset_s_only() {
        let env = Env::from_pairs(
            [("INCLUDE".into(), "C:/msvc/include;;C:/sdk/ucrt".into())],
            NameCase::Insensitive,
        );
        let with_toolset = Prepared {
            env: env.clone(),
            sdk: Some(WindowsSdk {
                dir: "C:/sdk".into(),
                version: "10.0.28000.0".to_owned(),
            }),
            ignored: Vec::new(),
        };
        assert_eq!(
            with_toolset.include_dirs(),
            [
                PathBuf::from("C:/msvc/include"),
                PathBuf::from("C:/sdk/ucrt")
            ]
        );
        let without = Prepared {
            env,
            sdk: None,
            ignored: Vec::new(),
        };
        assert!(without.include_dirs().is_empty());
    }

    /// A step's command is identified by the compiler's version and the
    /// `INCLUDE`, `LIB` and `LIBPATH` it runs with, so an updated compiler
    /// or SDK rebuilds.
    ///
    /// Mutation: leave out the version or any of the variables; this test
    /// fails.
    #[test]
    fn the_compiler_and_its_environment_identify_commands() {
        let prepared = |env| Prepared {
            env,
            sdk: None,
            ignored: Vec::new(),
        };
        let env = Env::from_pairs(
            [
                ("include".into(), "C:/inc".into()),
                ("LIB".into(), "C:/lib".into()),
                ("LIBPATH".into(), "C:/libpath".into()),
                ("PATH".into(), "C:/bin".into()),
            ],
            NameCase::Insensitive,
        );
        assert_eq!(
            prepared(env).identity("19.52.1"),
            ["19.52.1", "C:/inc", "C:/lib", "C:/libpath"].map(OsStr::new)
        );
        assert_eq!(
            prepared(Env::default()).identity("1"),
            ["1", "", "", ""].map(OsStr::new)
        );
    }

    /// A registry that names `dir` as the Windows Kits root.
    struct Kits(PathBuf);
    impl Registry for Kits {
        fn local_machine_string(&self, _key: &str, _value: &str) -> Option<OsString> {
            Some(self.0.clone().into())
        }
    }

    /// A runner that runs nothing.
    struct Nothing;
    impl Runner for Nothing {
        fn run(&self, _p: &Path, _a: &[&OsStr], _e: &Env) -> std::io::Result<crate::Output> {
            Err(std::io::Error::other("not run"))
        }
    }

    fn complete_sdk(kits: &Path, version: &str) {
        let include = kits.join("Include").join(version);
        let lib = kits.join("Lib").join(version);
        for (dir, file) in [("um", "windows.h"), ("ucrt", "stdio.h")] {
            std::fs::create_dir_all(include.join(dir)).unwrap();
            std::fs::write(include.join(dir).join(file), "").unwrap();
            std::fs::create_dir_all(lib.join(dir).join("x64")).unwrap();
        }
    }

    fn selected(msvc: Option<MsvcToolset>) -> Selected {
        Selected {
            kind: CompilerKind::ClangCl,
            executable: "clang-cl".into(),
            version: Version {
                major: 22,
                minor: 1,
                patch: 8,
            },
            msvc,
            source: Source::CommandLine,
        }
    }

    fn env() -> Env {
        Env::from_pairs([("KEEP".into(), "1".into())], NameCase::Insensitive)
    }

    /// With a toolset, the SDK is found and both are applied to the
    /// environment; without one, the environment is as given and there is
    /// no SDK; with no SDK, the error says so.
    ///
    /// Mutation: skip the SDK, return the environment unresolved, or require
    /// a toolset; this test fails.
    #[test]
    fn a_toolset_gets_the_sdk_and_its_environment() {
        let root = tempfile::tempdir().unwrap();
        let kits = root.path().join("Kits");
        complete_sdk(&kits, "10.0.28000.0");
        let toolset = MsvcToolset {
            dir: root.path().join("VS/VC/Tools/MSVC/14.52.1"),
            cl_version: Version {
                major: 19,
                minor: 52,
                patch: 1,
            },
        };
        let prepared = prepare(
            &selected(Some(toolset.clone())),
            &env(),
            &Kits(kits.clone()),
        )
        .unwrap();
        assert_eq!(prepared.sdk.as_ref().unwrap().version, "10.0.28000.0");
        let include = prepared
            .env
            .get("INCLUDE")
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(
            include.contains(&toolset.dir.join("include").display().to_string()),
            "{include}"
        );
        assert!(include.contains("10.0.28000.0"), "{include}");
        assert_eq!(prepared.env.get("KEEP").unwrap(), "1");

        let bare = prepare(&selected(None), &env(), &Kits(kits)).unwrap();
        assert!(bare.sdk.is_none());
        assert_eq!(bare.env.get("INCLUDE"), None);
        assert_eq!(bare.env.get("KEEP").unwrap(), "1");

        let nowhere = root.path().join("none");
        assert!(matches!(
            prepare(&selected(Some(toolset)), &env(), &Kits(nowhere)),
            Err(ToolchainError::NoWindowsSdk { .. })
        ));
    }

    /// GCC runs with its own directory first on `PATH` and the rest kept,
    /// and with no MSVC environment; no other compiler's `PATH` changes.
    ///
    /// Mutation: give GCC the environment unchanged, or put its directory
    /// on another compiler's `PATH`; this test fails.
    #[test]
    fn gcc_finds_its_own_dlls() {
        let path = |dirs: &[&str]| std::env::join_paths(dirs).unwrap();
        let env = Env::from_pairs(
            [("PATH".into(), path(&["C:/Windows"]))],
            NameCase::Insensitive,
        );
        let gcc = Selected {
            kind: CompilerKind::Gcc,
            executable: "C:/msys64/ucrt64/bin/gcc.exe".into(),
            ..selected(None)
        };
        let prepared = prepare(&gcc, &env, &Kits("C:/nowhere".into())).unwrap();
        assert_eq!(
            prepared.env.get("PATH"),
            Some(path(&["C:/msys64/ucrt64/bin", "C:/Windows"]).as_os_str())
        );
        assert!(prepared.sdk.is_none());
        for kind in [
            CompilerKind::Msvc,
            CompilerKind::Clang,
            CompilerKind::ClangCl,
        ] {
            let other = Selected {
                kind,
                ..gcc.clone()
            };
            let prepared = prepare(&other, &env, &Kits("C:/nowhere".into())).unwrap();
            assert_eq!(prepared.env.get("PATH"), env.get("PATH"), "{kind:?}");
        }
    }

    /// Off Windows nothing looks for Visual Studio.
    ///
    /// Mutation: look for Visual Studio on every host; this test fails.
    #[test]
    fn other_hosts_look_for_no_visual_studio() {
        let detected = detect(&env(), HostOs::Other, &ExplicitPaths::default(), &Nothing);
        assert!(
            matches!(detected.installs, Ok(ref v) if v.is_empty()),
            "{detected:?}"
        );
        assert!(
            matches!(detected.llvm, Ok(ref v) if v.is_empty()),
            "{detected:?}"
        );
    }

    /// Answers vswhere, cl.exe and clang from canned output.
    struct Canned(Vec<(&'static str, Vec<u8>)>);
    impl Runner for Canned {
        fn run(&self, program: &Path, _a: &[&OsStr], _e: &Env) -> std::io::Result<crate::Output> {
            let program = program.to_string_lossy().replace('\\', "/");
            self.0
                .iter()
                .find(|(suffix, _)| program.ends_with(suffix))
                .map(|(_, stdout)| crate::Output {
                    success: true,
                    stdout: stdout.clone(),
                    stderr: Vec::new(),
                })
                .ok_or_else(|| std::io::Error::other("not answered"))
        }
    }

    /// On Windows, the Visual Studio found is searched for its bundled LLVM:
    /// with nothing on `PATH` and no `%ProgramFiles%\LLVM`, the clang
    /// Visual Studio ships is found.
    ///
    /// Mutation: search LLVM without the installations found; this test
    /// fails.
    #[test]
    fn the_llvm_visual_studio_bundles_is_found() {
        let root = tempfile::tempdir().unwrap();
        let installer = root.path().join("x86/Microsoft Visual Studio/Installer");
        std::fs::create_dir_all(&installer).unwrap();
        std::fs::write(installer.join("vswhere.exe"), "").unwrap();
        let vs = root.path().join("VS");
        let cl_dir = vs.join("VC/Tools/MSVC/14.52.1/bin/Hostx64/x64");
        let llvm_dir = vs.join("VC/Tools/Llvm/x64/bin");
        for dir in [&cl_dir, &llvm_dir] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(cl_dir.join("cl.exe"), "").unwrap();
        std::fs::write(llvm_dir.join("clang.exe"), "").unwrap();
        let vswhere = serde_json::to_vec(&serde_json::json!([{
            "installationPath": vs,
            "isPrerelease": false,
            "isComplete": true,
            "isLaunchable": true,
        }]))
        .unwrap();
        let run = Canned(vec![
            ("vswhere.exe", vswhere),
            (
                "cl.exe",
                b"Microsoft (R) C/C++ Optimizing Compiler Version 19.52.1 for x64".to_vec(),
            ),
            (
                "clang.exe",
                b"clang version 22.1.3\nTarget: x86_64-pc-windows-msvc\n".to_vec(),
            ),
        ]);
        let env = Env::from_pairs(
            [(
                "ProgramFiles(x86)".into(),
                root.path().join("x86").into_os_string(),
            )],
            NameCase::Insensitive,
        );
        let detected = detect(&env, HostOs::Windows, &ExplicitPaths::default(), &run);
        assert_eq!(detected.installs.unwrap().len(), 1);
        let llvm = detected.llvm.unwrap();
        assert_eq!(llvm.len(), 1, "{llvm:?}");
        assert_eq!(llvm[0].path, llvm_dir.join("clang.exe"));
    }

    /// Variables that add compiler or linker options unseen are left out of
    /// the build's environment, with or without a toolset, and named so the
    /// build can warn; others are kept.
    ///
    /// Mutation: keep any of them, or name one that was not set; this test
    /// fails.
    #[test]
    fn option_variables_are_left_out_and_named() {
        let env = Env::from_pairs(
            [
                ("cl".into(), "/DX".into()),
                ("_LINK_".into(), "/STACK:1".into()),
                ("CPATH".into(), "C:/x".into()),
                ("KEEP".into(), "1".into()),
            ],
            NameCase::Insensitive,
        );
        let prepared = prepare(&selected(None), &env, &Kits(PathBuf::new())).unwrap();
        assert_eq!(prepared.ignored, ["CL", "_LINK_", "CPATH"]);
        for name in TOOL_OPTION_VARIABLES {
            assert_eq!(prepared.env.get(name), None, "{name}");
        }
        assert_eq!(prepared.env.get("KEEP").unwrap(), "1");
    }
}
