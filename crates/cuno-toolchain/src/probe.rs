//! The standard library probe: whether the MSVC standard library accepts
//! the selected Clang, and clang-tidy, asked by compiling a file that
//! includes `<yvals_core.h>` (`docs/en/specs/toolchain.md`, "Minimum
//! compiler versions"). Selection decides which compiler; this asks
//! whether it can build here.

use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

use cuno_core::Env;

use crate::compiler::Family;
use crate::flags::{CompileInput, Mode, ToolContext, compile_args, for_clang_tools};
use crate::select::{LlvmChoice, Selected};
use crate::{Runner, ToolchainError, Version};

/// What a standard library probe compiles with, besides the probe file.
#[derive(Clone, Copy, Debug)]
pub struct ProbeInputs<'a> {
    /// A directory outside the project to write the probe file in.
    pub scratch: &'a Path,
    /// The compile input for a C++ source: the probe compiles C++, so a C
    /// package's is made with `Lang::Cxx` and a C++ edition, or a C
    /// standard switch would be rejected. [`check`] is given the build's and
    /// runs it in check mode, [`check_tidy`] compile_commands.json's and
    /// runs it compiled, as that file plans it; each sets the mode itself,
    /// whatever `input.mode` says.
    pub input: &'a CompileInput<'a>,
    /// Where the compiler runs: the project root, as for the build, so a
    /// relative path in the project's flags or include directories means
    /// the same (`toolchain.md`, "Minimum
    /// compiler versions").
    pub cwd: &'a Path,
}

/// Whether [`check`] asks anything of `selected`: only an LLVM compiler
/// using the MSVC standard library is checked; GCC brings its own. A caller asks first, so
/// that nothing is prepared (a scratch directory) for a check that does not
/// apply.
pub fn applies(selected: &Selected) -> bool {
    match selected.kind.family() {
        Family::Llvm => selected.msvc.is_some(),
        Family::Msvc | Family::Gnu => false,
    }
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
    if !applies(selected) {
        return Ok(());
    }
    stl_probe(
        selected.kind.name(),
        selected.version,
        inputs.scratch,
        |probe| {
            let args = check_command(&inputs, probe);
            run.run_in(&selected.executable, &as_args(&args), env, inputs.cwd)
        },
    )
}

/// The build's command for `probe` in check mode: the check switch, every
/// flag the build gives, the language switch and the source.
fn check_command(inputs: &ProbeInputs<'_>, probe: &Path) -> Vec<OsString> {
    let input = CompileInput {
        mode: Mode::Check,
        ..*inputs.input
    };
    compile_args(&input, probe, None)
}

/// Whether `tidy` is accepted by the MSVC standard library: the same probe
/// as [`check`], analysed by clang-tidy with one harmless check enabled
/// (with none it stops without analysing) and, after `--`, the command
/// compile_commands.json would hold for it without the source, which
/// clang-tidy is given as its file: `inputs.input` compiled, as the file
/// is planned (`dev`, `CUNO_TEST`), and read as Clang-based tools read it,
/// with `context` ([`for_clang_tools`]). That is what `cuno lint` runs
/// clang-tidy with (`toolchain.md`, "Minimum compiler versions"). The
/// project's `.clang-tidy` is not read.
pub fn check_tidy(
    tidy: &LlvmChoice,
    env: &Env,
    context: ToolContext<'_>,
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
        // Compiled, as compile_commands.json plans its commands.
        let input = CompileInput {
            mode: Mode::Compile,
            ..*inputs.input
        };
        let command = for_clang_tools(context, &compile_args(&input, probe, None));
        args.extend(
            command
                .after_double_dash()
                .into_iter()
                .filter(|arg| arg.as_os_str() != probe.as_os_str()),
        );
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
    use crate::compiler::CompilerKind;
    use crate::flags::{
        BuildSettings, Edition, Lang, OptLevel, ProfileSettings, Runtime, TARGET_SWITCH, Warnings,
    };
    use crate::msvc::MsvcToolset;
    use crate::select::Source;
    use std::cell::RefCell;
    use std::path::PathBuf;

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

    /// A C++ project's settings with RTTI off and `_HAS_STATIC_RTTI=0`
    /// defined, as one built without RTTI writes them.
    fn no_rtti() -> (BuildSettings, ProfileSettings) {
        let build = BuildSettings {
            warnings: Warnings::Default,
            warnings_as_errors: false,
            defines: vec!["_HAS_STATIC_RTTI=0".into()],
            include_dirs: vec![],
            links: vec![],
            lib_dirs: vec![],
            link_flags: vec![],
            flags: vec![],
            runtime: Runtime::Dynamic,
            exceptions: true,
            rtti: false,
        };
        let profile = ProfileSettings {
            opt_level: OptLevel::O0,
            debug_info: false,
            debug_assertions: true,
            lto: false,
        };
        (build, profile)
    }

    /// What clang-tidy is told of a build with no `INCLUDE` and no toolset.
    fn no_context(kind: CompilerKind) -> ToolContext<'static> {
        ToolContext {
            kind,
            include: &[],
            msvc: None,
            gnuc: None,
        }
    }

    /// A C++20 input of `settings` for `kind`, in `mode`.
    fn input(
        kind: CompilerKind,
        settings: &(BuildSettings, ProfileSettings),
        mode: Mode,
    ) -> CompileInput<'_> {
        CompileInput {
            kind,
            lang: Lang::Cxx,
            edition: Edition::Cxx20,
            build: &settings.0,
            profile: &settings.1,
            include_dirs: &[],
            test: false,
            mode,
        }
    }

    fn strings(args: Vec<OsString>) -> Vec<String> {
        args.into_iter().map(|a| a.into_string().unwrap()).collect()
    }

    /// Mutation: treat a failed or timed-out probe as passing, call any
    /// failure STL1000, read only stdout, lose the version the library asks
    /// for, the compiler, its version or the library's message, keep the
    /// probe file, or swallow why it could not run; this test fails.
    #[test]
    fn the_probe_verdict_follows_the_standard_library() {
        let root = tempfile::tempdir().unwrap();
        let settings = no_rtti();
        let compile = input(CompilerKind::Clang, &settings, Mode::Check);
        let inputs = ProbeInputs {
            scratch: root.path(),
            input: &compile,
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

    /// The probe runs the build's own command for the probe file in check
    /// mode, whatever mode it is handed: the check switch, every flag the
    /// build gives (the project's RTTI setting here), the C++ switch and the
    /// file, with the target named once. MSVC, and a Clang with no MSVC
    /// toolset, are not probed, and every probe file has its own name.
    ///
    /// Mutation: add a switch or the target of the probe's own, drop the
    /// build's flags, probe in the mode it is handed, probe MSVC or a Clang
    /// without an MSVC toolset, or name every probe file alike; this test
    /// fails.
    #[test]
    fn the_probe_runs_the_builds_command_in_check_mode() {
        let root = tempfile::tempdir().unwrap();
        let settings = no_rtti();
        let mut probes = Vec::new();
        for (kind, switch, cxx, rtti) in [
            (CompilerKind::Clang, "-fsyntax-only", "c++", "-fno-rtti"),
            (CompilerKind::ClangCl, "/Zs", "/TP", "/GR-"),
        ] {
            let handed = input(kind, &settings, Mode::Compile);
            let inputs = ProbeInputs {
                scratch: root.path(),
                input: &handed,
                cwd: root.path(),
            };
            let run = answers(true, "", "");
            check(&selected(kind), &Env::default(), inputs, &run).unwrap();
            let args = run.args.borrow().clone();
            let probe = args.last().unwrap().clone();
            assert!(probe.ends_with(".cpp"), "{probe}");
            let expected = strings(compile_args(
                &input(kind, &settings, Mode::Check),
                Path::new(&probe),
                None,
            ));
            assert_eq!(args, expected, "{kind:?}");
            assert_eq!(args[0], switch, "{kind:?}");
            for needed in [cxx, rtti, "-D_HAS_STATIC_RTTI=0"] {
                let needed = match kind {
                    CompilerKind::ClangCl if needed.starts_with("-D") => "/D_HAS_STATIC_RTTI=0",
                    _ => needed,
                };
                assert!(args.iter().any(|a| a == needed), "{kind:?}: {args:?}");
            }
            assert_eq!(
                args.iter().filter(|a| a.starts_with("--target")).count(),
                1,
                "{kind:?}: {args:?}"
            );
            probes.push(probe);
        }
        assert_ne!(probes[0], probes[1]);

        let compile = input(CompilerKind::Msvc, &settings, Mode::Check);
        let inputs = ProbeInputs {
            scratch: root.path(),
            input: &compile,
            cwd: root.path(),
        };
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

    /// clang-tidy is asked about the probe file, with one check, no
    /// configuration, and after `--` the same check-mode command (whatever
    /// mode it is handed) without the file: in cl's mode for a cl-syntax compiler, with the target
    /// named once (an MSVC compile's flags carry none).
    ///
    /// Mutation: read only stderr (clang-tidy reports STL1000 on stdout),
    /// name the compiler instead of clang-tidy, leave out the one check or
    /// `--`, give the command before `--` or with the file again, put
    /// cl-style flags through clang-tidy without `--driver-mode=cl` (or
    /// GNU-style ones with it), name the target twice or not at all for
    /// MSVC, run the command in the mode it is handed rather than compiled
    /// as compile_commands.json plans it, or leave out the include
    /// directories or the MSVC version it is given; this test fails.
    #[test]
    fn clang_tidy_is_asked_the_standard_librarys_verdict() {
        let root = tempfile::tempdir().unwrap();
        let settings = no_rtti();
        let tidy = LlvmChoice {
            path: "clang-tidy".into(),
            version: v(20, 1, 8),
        };
        let stl = "x\\yvals_core.h(922,1): error: STL1000: Unexpected compiler version, expected Clang 22 or newer. [clang-diagnostic-error]\n";
        let include = [PathBuf::from("C:/inc")];
        let version = v(19, 52, 1);
        let context = |kind| ToolContext {
            kind,
            include: &include,
            msvc: Some(&version),
            gnuc: None,
        };
        let msvc = input(CompilerKind::Msvc, &settings, Mode::Check);
        let err = check_tidy(
            &tidy,
            &Env::default(),
            context(CompilerKind::Msvc),
            ProbeInputs {
                scratch: root.path(),
                input: &msvc,
                cwd: root.path(),
            },
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
            (CompilerKind::Msvc, &["--driver-mode=cl", TARGET_SWITCH][..]),
            (CompilerKind::ClangCl, &["--driver-mode=cl"][..]),
            (CompilerKind::Clang, &[][..]),
        ] {
            let handed = input(kind, &settings, Mode::Check);
            let inputs = ProbeInputs {
                scratch: root.path(),
                input: &handed,
                cwd: root.path(),
            };
            let run = answers(true, "", "");
            check_tidy(&tidy, &Env::default(), context(kind), inputs, &run).unwrap();
            let compile = input(kind, &settings, Mode::Compile);
            let args = run.args.borrow().clone();
            assert_eq!(args[0], "--checks=-*,readability-braces-around-statements");
            let probe = &args[2];
            assert!(probe.ends_with(".cpp"), "{args:?}");
            let command: Vec<String> = strings(
                for_clang_tools(
                    context(kind),
                    &compile_args(&compile, Path::new(probe), None),
                )
                .args()
                .to_vec(),
            )
            .into_iter()
            .filter(|a| a != probe)
            .collect();
            for each in ["-fms-compatibility-version=19.52.1", "C:/inc"] {
                assert!(args.iter().any(|a| a.contains(each)), "{kind:?}: {args:?}");
            }
            let expected: Vec<String> = ["--"]
                .iter()
                .chain(mode)
                .map(|s| (*s).to_owned())
                .chain(command)
                .collect();
            assert_eq!(args[3..], expected, "{kind:?}");
            assert_eq!(
                args.iter().filter(|a| a.starts_with("--target")).count(),
                1,
                "{kind:?}: {args:?}"
            );
        }
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }

    /// With MSVC selected, clang-tidy reads the build's command as
    /// clang-cl's, so a standard switch clang-cl does not know is given as
    /// clang-cl's own: `edition = "c++23"` reaches clang-tidy as C++23
    /// (`docs/en/specs/toolchain.md`, "Minimum compiler versions").
    ///
    /// Mutation: give clang-tidy the command without the clang-cl reading;
    /// this test fails.
    #[test]
    fn clang_tidy_reads_an_msvc_standard_as_clang_cl_does() {
        let root = tempfile::tempdir().unwrap();
        let settings = no_rtti();
        let mut compile = input(CompilerKind::Msvc, &settings, Mode::Check);
        compile.edition = Edition::Cxx23;
        let run = answers(true, "", "");
        let tidy = LlvmChoice {
            path: "clang-tidy".into(),
            version: v(22, 1, 8),
        };
        let inputs = ProbeInputs {
            scratch: root.path(),
            input: &compile,
            cwd: root.path(),
        };
        check_tidy(
            &tidy,
            &Env::default(),
            no_context(CompilerKind::Msvc),
            inputs,
            &run,
        )
        .unwrap();
        let args = run.args.borrow().clone();
        assert!(args.iter().any(|a| a == "/clang:-std=c++23"), "{args:?}");
        assert!(!args.iter().any(|a| a == "/std:c++23"), "{args:?}");
    }

    /// With MSVC selected and `lto` on, clang-tidy is not given `/GL`,
    /// which clang-cl reports as unused when it only analyses.
    ///
    /// Mutation: keep `/GL` in the clang-cl reading; this test fails.
    #[test]
    fn clang_tidy_is_not_given_link_time_optimization() {
        let root = tempfile::tempdir().unwrap();
        let mut settings = no_rtti();
        settings.1.lto = true;
        let compile = input(CompilerKind::Msvc, &settings, Mode::Check);
        let built = strings(compile_args(&compile, Path::new("x.cpp"), None));
        assert!(built.iter().any(|a| a == "/GL"), "{built:?}");
        let run = answers(true, "", "");
        let tidy = LlvmChoice {
            path: "clang-tidy".into(),
            version: v(22, 1, 8),
        };
        let inputs = ProbeInputs {
            scratch: root.path(),
            input: &compile,
            cwd: root.path(),
        };
        check_tidy(
            &tidy,
            &Env::default(),
            no_context(CompilerKind::Msvc),
            inputs,
            &run,
        )
        .unwrap();
        let args = run.args.borrow().clone();
        assert!(!args.iter().any(|a| a == "/GL"), "{args:?}");
    }
}
