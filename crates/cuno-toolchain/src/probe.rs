//! The standard library probe: whether the MSVC standard library accepts
//! the selected Clang, and clang-tidy, asked by compiling a file that
//! includes `<yvals_core.h>` (`docs/en/specs/toolchain.md`, "Minimum
//! compiler versions"). Selection decides which compiler; this asks
//! whether it can build here.

use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

use cuno_core::Env;

use crate::compiler::{CompilerKind, Family, Syntax};
use crate::select::{LlvmChoice, Selected};
use crate::{Runner, ToolchainError, Version};

/// What the build's compile command would also get, for the standard library
/// probe.
#[derive(Clone, Copy, Debug)]
pub struct ProbeInputs<'a> {
    /// A directory outside the project to write the probe file in.
    pub scratch: &'a Path,
    /// The build's compile flags in this compiler's syntax, except those
    /// naming inputs and outputs: the language standard, RTTI and exceptions,
    /// include directories, defines and the user's `flags`, as
    /// [`crate::flags::compile_flags`] gives them for a C++ input: the probe
    /// compiles C++, so a C package's are built with `Lang::Cxx` and a C++
    /// edition, or a C standard switch would be rejected.
    pub flags: &'a [OsString],
    /// Where the compiler runs: the project root, as for the build, so a
    /// relative path in `flags` means the same (`toolchain.md`, "Minimum
    /// compiler versions").
    pub cwd: &'a Path,
}

/// Whether the selected compiler can build here: on Windows a Clang must be
/// accepted by the MSVC standard library, which is asked directly by
/// syntax-checking a file that includes `<yvals_core.h>` (`toolchain.md`,
/// "Minimum compiler versions"). `env` is the resolved Windows environment.
/// The one check `cuno doctor` and the build commands share.
pub fn check(
    selected: &Selected,
    env: &Env,
    inputs: ProbeInputs<'_>,
    run: &dyn Runner,
) -> Result<(), ToolchainError> {
    // Only an LLVM compiler using the MSVC standard library is checked.
    match selected.kind.family() {
        Family::Llvm if selected.msvc.is_some() => {}
        Family::Llvm | Family::Msvc => return Ok(()),
    }
    let switches: &[&str] = match selected.kind.syntax() {
        Syntax::Cl => &["/nologo", "/TP", "/Zs"],
        Syntax::Gnu => &["-x", "c++", "-fsyntax-only"],
    };
    stl_probe(
        selected.kind.name(),
        selected.version,
        inputs.scratch,
        |probe| {
            let mut args: Vec<OsString> = switches.iter().map(OsString::from).collect();
            args.push("--target=x86_64-pc-windows-msvc".into());
            args.extend(inputs.flags.iter().cloned());
            args.push(probe.into());
            run.run_in(&selected.executable, &as_args(&args), env, inputs.cwd)
        },
    )
}

/// Whether `tidy` is accepted by the MSVC standard library: the same probe
/// as [`check`], analysed by clang-tidy with one harmless check enabled
/// (with none it stops without analysing) and, after `--`, the build's
/// compile flags for `kind`, in `kind`'s syntax (`toolchain.md`, "Minimum
/// compiler versions"). The project's `.clang-tidy` is not read.
pub fn check_tidy(
    tidy: &LlvmChoice,
    kind: CompilerKind,
    env: &Env,
    inputs: ProbeInputs<'_>,
    run: &dyn Runner,
) -> Result<(), ToolchainError> {
    stl_probe("clang-tidy", tidy.version, inputs.scratch, |probe| {
        let mut args: Vec<OsString> = vec![
            "--checks=-*,readability-braces-around-statements".into(),
            "--config={}".into(),
            probe.into(),
            "--".into(),
        ];
        let cl_syntax = match kind.syntax() {
            Syntax::Cl => true,
            Syntax::Gnu => false,
        };
        if cl_syntax {
            // clang-tidy reads flags as the GNU-style driver unless told.
            args.push("--driver-mode=cl".into());
        }
        args.push("--target=x86_64-pc-windows-msvc".into());
        args.extend(inputs.flags.iter().cloned());
        run.run_in(&tidy.path, &as_args(&args), env, inputs.cwd)
    })
}

fn as_args(args: &[OsString]) -> Vec<&OsStr> {
    args.iter().map(|a| a.as_os_str()).collect()
}

/// Writes the probe file to `scratch`, runs `invoke` on it, and reads the
/// standard library's verdict: `STL1000:` on stdout (clang-tidy) or stderr
/// (the compilers) is a rejection; any other failure decides nothing.
fn stl_probe(
    name: &'static str,
    version: Version,
    scratch: &Path,
    invoke: impl FnOnce(&Path) -> std::io::Result<crate::Output>,
) -> Result<(), ToolchainError> {
    let failed = |output: String| ToolchainError::ProbeFailed {
        compiler: name,
        output,
    };
    // A name of its own, so that two cuno processes sharing `scratch` do not
    // write or remove each other's file.
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let probe = scratch.join(format!(
        "cuno-stl-probe-{}-{}.cpp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&probe, "#include <yvals_core.h>\n")
        .map_err(|e| failed(format!("could not write {}: {e}", probe.display())))?;
    let output = invoke(&probe);
    let _ = std::fs::remove_file(&probe);
    let output = output.map_err(|e| failed(e.to_string()))?;
    if output.success {
        return Ok(());
    }
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    match text.lines().find(|line| line.contains("STL1000:")) {
        Some(line) => Err(ToolchainError::RejectedByStl {
            compiler: name,
            version,
            needed: needed_clang(line),
            message: line.trim().to_owned(),
        }),
        None => Err(failed(text.trim().to_owned())),
    }
}

/// `<version>` in the standard library's `expected Clang <version> or newer`,
/// as written.
fn needed_clang(message: &str) -> Option<String> {
    let rest = &message[message.find("expected Clang ")? + "expected Clang ".len()..];
    let version = &rest[..rest.find(" or newer")?];
    (!version.is_empty()).then(|| version.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Output;
    use crate::msvc::MsvcToolset;
    use crate::select::Source;
    use std::cell::RefCell;

    fn v(major: u32, minor: u32, patch: u32) -> Version {
        Version {
            major,
            minor,
            patch,
        }
    }

    /// An MSVC toolset of cl `version` in the Visual Studio at `path`.
    fn toolset(path: &str, version: Version) -> MsvcToolset {
        MsvcToolset {
            dir: Path::new(path).join(format!("VC/Tools/MSVC/{version}")),
            cl_version: version,
        }
    }

    /// A runner that answers the probe and records the arguments.
    struct Answers {
        ok: bool,
        stdout: &'static str,
        stderr: &'static str,
        args: RefCell<Vec<String>>,
        calls: RefCell<u32>,
    }

    fn answers(ok: bool, stdout: &'static str, stderr: &'static str) -> Answers {
        Answers {
            ok,
            stdout,
            stderr,
            args: Default::default(),
            calls: Default::default(),
        }
    }

    impl Runner for Answers {
        fn run(&self, _p: &Path, args: &[&OsStr], _e: &Env) -> std::io::Result<Output> {
            *self.calls.borrow_mut() += 1;
            *self.args.borrow_mut() = args
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            Ok(Output {
                success: self.ok,
                stdout: self.stdout.as_bytes().to_vec(),
                stderr: self.stderr.as_bytes().to_vec(),
            })
        }
    }

    struct CannotStart;
    impl Runner for CannotStart {
        fn run(&self, _p: &Path, _a: &[&OsStr], _e: &Env) -> std::io::Result<Output> {
            Err(std::io::Error::other("cannot start"))
        }
    }

    /// A runner whose tool runs past the limit, as `SystemRunner` reports it.
    struct TimesOut;
    impl Runner for TimesOut {
        fn run(&self, _p: &Path, _a: &[&OsStr], _e: &Env) -> std::io::Result<Output> {
            Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "did not finish within 30 seconds and was stopped",
            ))
        }
    }

    fn selected(kind: CompilerKind) -> Selected {
        Selected {
            kind,
            executable: "clang".into(),
            version: v(20, 1, 8),
            msvc: Some(toolset("VS", v(19, 52, 1))),
            source: Source::CommandLine,
        }
    }

    /// Mutation: treat a failed or timed-out probe as passing, call any
    /// failure STL1000,
    /// read only stdout, lose the version the library asks for, the
    /// compiler, its version or the library's message, keep the probe file,
    /// or swallow why it could not run; this test fails.
    #[test]
    fn the_probe_verdict_follows_the_standard_library() {
        let root = tempfile::tempdir().unwrap();
        let inputs = ProbeInputs {
            scratch: root.path(),
            flags: &[],
            cwd: root.path(),
        };
        let clang = selected(CompilerKind::Clang);
        let stl = "x\\yvals_core.h(922,1): error: STL1000: Unexpected compiler version, expected Clang 22 or newer.\n";
        let err = check(&clang, &Env::default(), inputs, &answers(false, "", stl)).unwrap_err();
        assert_eq!(
            err.to_string(),
            "clang 20.1.8 is not accepted by the MSVC standard library in use (STL1000)"
        );
        let ToolchainError::RejectedByStl {
            needed, message, ..
        } = &err
        else {
            panic!("{err:?}");
        };
        assert_eq!(
            (needed.as_deref(), message.as_str()),
            (Some("22"), stl.trim())
        );

        let err = check(
            &clang,
            &Env::default(),
            inputs,
            &answers(false, "fatal error: 'yvals_core.h' file not found", ""),
        )
        .unwrap_err();
        assert!(
            matches!(&err, ToolchainError::ProbeFailed { compiler: "clang", output } if output == "fatal error: 'yvals_core.h' file not found"),
            "{err:?}"
        );
        let err = check(&clang, &Env::default(), inputs, &TimesOut).unwrap_err();
        assert!(
            matches!(&err, ToolchainError::ProbeFailed { output, .. } if output.contains("within 30 seconds")),
            "{err:?}"
        );
        let err = check(&clang, &Env::default(), inputs, &CannotStart).unwrap_err();
        assert!(
            matches!(&err, ToolchainError::ProbeFailed { output, .. } if output == "cannot start"),
            "{err:?}"
        );
        check(&clang, &Env::default(), inputs, &answers(true, "", "")).unwrap();
        assert_eq!(
            std::fs::read_dir(root.path()).unwrap().count(),
            0,
            "probe file left behind"
        );

        assert_eq!(
            needed_clang("expected Clang 17.0.0 or newer."),
            Some("17.0.0".into())
        );
        assert_eq!(needed_clang("expected Clang  or newer."), None);
        assert_eq!(needed_clang("STL1000: Unexpected compiler version"), None);
    }

    /// Mutation: probe as C, without the target, without the build's flags
    /// or with the other driver's mode, probe MSVC or a Clang without an
    /// MSVC toolset, or name every probe file alike; this test fails.
    #[test]
    fn the_probe_compiles_as_cpp_under_the_builds_flags() {
        let root = tempfile::tempdir().unwrap();
        let flags: Vec<OsString> = vec!["-fno-rtti".into(), "-D_HAS_STATIC_RTTI=0".into()];
        let inputs = ProbeInputs {
            scratch: root.path(),
            flags: &flags,
            cwd: root.path(),
        };
        let mut probes = Vec::new();
        for (kind, mode) in [
            (CompilerKind::Clang, ["-x", "c++", "-fsyntax-only"]),
            (CompilerKind::ClangCl, ["/nologo", "/TP", "/Zs"]),
        ] {
            let run = answers(true, "", "");
            check(&selected(kind), &Env::default(), inputs, &run).unwrap();
            let args = run.args.borrow().clone();
            let (probe, rest) = args.split_last().unwrap();
            assert_eq!(
                rest,
                [
                    &mode[..],
                    &[
                        "--target=x86_64-pc-windows-msvc",
                        "-fno-rtti",
                        "-D_HAS_STATIC_RTTI=0"
                    ]
                ]
                .concat(),
                "{kind:?}"
            );
            assert!(probe.ends_with(".cpp"), "{probe}");
            probes.push(probe.clone());
        }
        assert_ne!(probes[0], probes[1]);

        let never = answers(false, "STL1000:", "");
        check(
            &selected(CompilerKind::Msvc),
            &Env::default(),
            inputs,
            &never,
        )
        .unwrap();
        let mut elsewhere = selected(CompilerKind::Clang);
        elsewhere.msvc = None;
        check(&elsewhere, &Env::default(), inputs, &never).unwrap();
        assert_eq!(*never.calls.borrow(), 0);
    }

    /// Mutation: read only stderr (clang-tidy reports STL1000 on stdout),
    /// name the compiler instead of clang-tidy, leave out the one check or
    /// `--`, give the build's flags before `--`, or put cl-style flags
    /// through clang-tidy without `--driver-mode=cl` (or GNU-style ones
    /// with it); this test fails.
    #[test]
    fn clang_tidy_is_asked_the_standard_librarys_verdict() {
        let root = tempfile::tempdir().unwrap();
        let flags: Vec<OsString> = vec!["/GR-".into()];
        let inputs = ProbeInputs {
            scratch: root.path(),
            flags: &flags,
            cwd: root.path(),
        };
        let tidy = LlvmChoice {
            path: "clang-tidy".into(),
            version: v(20, 1, 8),
        };
        let stl = "x\\yvals_core.h(922,1): error: STL1000: Unexpected compiler version, expected Clang 22 or newer. [clang-diagnostic-error]\n";
        let err = check_tidy(
            &tidy,
            CompilerKind::Msvc,
            &Env::default(),
            inputs,
            &answers(false, stl, ""),
        )
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "clang-tidy 20.1.8 is not accepted by the MSVC standard library in use (STL1000)"
        );
        assert_eq!(
            cuno_core::CunoError::help(&err).unwrap(),
            "install clang-tidy 22 or newer"
        );

        for (kind, mode) in [
            (CompilerKind::Msvc, &["--driver-mode=cl"][..]),
            (CompilerKind::ClangCl, &["--driver-mode=cl"][..]),
            (CompilerKind::Clang, &[][..]),
        ] {
            let run = answers(true, "", "");
            check_tidy(&tidy, kind, &Env::default(), inputs, &run).unwrap();
            let args = run.args.borrow().clone();
            assert_eq!(args[0], "--checks=-*,readability-braces-around-statements");
            assert!(args[2].ends_with(".cpp"), "{args:?}");
            assert_eq!(
                args[3..],
                [
                    &["--"][..],
                    mode,
                    &["--target=x86_64-pc-windows-msvc", "/GR-"]
                ]
                .concat(),
                "{kind:?}"
            );
        }
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }
}
