//! What every command that compiles does before it compiles: find the
//! compilers and tools, then, for the selected compiler, resolve the
//! environment it runs in. `cuno doctor` and the build commands both call
//! these, so they diagnose and build with the same answers.

use cuno_core::Env;

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

/// The environment the selected compiler runs in.
#[derive(Clone, Debug)]
pub struct Prepared {
    /// The process environment with the MSVC toolset's and the Windows SDK's
    /// paths applied, or as given when no toolset is used.
    pub env: Env,
    /// The Windows SDK, when a toolset is used.
    pub sdk: Option<WindowsSdk>,
}

/// Resolves the environment for `selected`: with an MSVC toolset (every
/// compiler on Windows), the Windows SDK is found and both are applied to
/// `env` (`docs/en/specs/toolchain.md`, "Environment resolution on Windows").
pub fn prepare(
    selected: &Selected,
    env: &Env,
    registry: &dyn Registry,
) -> Result<Prepared, ToolchainError> {
    let Some(toolset) = &selected.msvc else {
        return Ok(Prepared {
            env: env.clone(),
            sdk: None,
        });
    };
    let sdk = msvc::find_windows_sdk(env, registry)?;
    Ok(Prepared {
        env: msvc::resolve(toolset, &sdk).apply(env),
        sdk: Some(sdk),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Version;
    use crate::compiler::CompilerKind;
    use crate::msvc::MsvcToolset;
    use crate::select::Source;
    use cuno_core::NameCase;
    use std::ffi::{OsStr, OsString};
    use std::path::{Path, PathBuf};

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
}
