//! `cuno doctor` run as a process, its whole stderr compared with what the
//! spec says it prints (`docs/en/specs/commands.md`, "What `cuno doctor`
//! prints"). The expected lines are built from cuno's own detection and
//! selection, which `tests/toolchain_detection.rs` already holds to the
//! independent reference; what is tested here is the command: what it
//! chooses, what it prints, and how it exits.
#![cfg(windows)]

mod support;

use std::process::Command;

use cuno_core::Env;
use cuno_toolchain::llvm::{ExplicitPaths, find_llvm_tools};
use cuno_toolchain::msvc::{self, SystemRegistry};
use cuno_toolchain::select::{self, Choices, Found, Selected};
use cuno_toolchain::{HostOs, SystemRunner};
use support::compilers::{Compiler, Language, require};
use support::{Project, Run};

/// The environment the tests run cuno in, without `CUNO_*` (as
/// `support::hermetic` leaves it).
fn env() -> Env {
    Env::from_process().without(&["CUNO_COMPILER", "CUNO_TARGET_DIR"])
}

/// What cuno's selection gives for `name` here.
fn selected(name: &str) -> Selected {
    let env = env();
    let runner = SystemRunner::new();
    let installs = msvc::find_installations(&env, &runner).unwrap();
    let llvm = find_llvm_tools(
        &env,
        HostOs::Windows,
        &ExplicitPaths::default(),
        &installs,
        &runner,
    )
    .unwrap();
    let choices = Choices {
        cli: Some(name.to_owned()),
        ..Default::default()
    };
    let found = Found {
        installs: &installs,
        llvm: &llvm,
        explicit_msvc: None,
    };
    select::select(&choices, found, &Default::default(), HostOs::Windows).unwrap()
}

fn sdk() -> String {
    msvc::find_windows_sdk(&env(), &SystemRegistry)
        .unwrap()
        .version
}

/// The lines for `selected`, chosen by `source`, with no problem.
fn healthy(selected: &Selected, source: &str) -> String {
    let mut out = format!(
        "    Selected {} {} ({source})\n             {}\n",
        selected.kind.name(),
        selected.version,
        selected.executable.display()
    );
    let toolset = selected.msvc.as_ref().unwrap();
    if selected.kind != select::CompilerKind::Msvc {
        out += &format!(
            "       Using MSVC {} ({})\n",
            toolset.cl_version,
            toolset.dir.ancestors().nth(4).unwrap().display()
        );
    }
    out += &format!("       Using Windows SDK {}\n", sdk());
    if selected.kind != select::CompilerKind::Msvc {
        out += "    Accepted by the MSVC standard library\n";
    }
    out + "    Finished no problems found\n"
}

/// Outside a project, each compiler named by `--compiler` is diagnosed with
/// its location, version, MSVC toolset, Windows SDK and, for Clang, the
/// standard library check.
///
/// Mutation: change any line's verb, order or text, leave out the probe, or
/// exit non-zero with no problem; this test fails.
#[test]
fn each_compiler_is_diagnosed_outside_a_project() {
    for (name, compiler) in [
        ("msvc", Compiler::Msvc),
        ("clang", Compiler::Clang),
        ("clang-cl", Compiler::ClangCl),
    ] {
        if require(compiler, Language::C, "x.c", "x.exe").is_none() {
            continue;
        }
        let expected = healthy(&selected(name), "--compiler");
        Project::new()
            .cuno(&["doctor", "--compiler", name])
            .assert(0, "", &expected);
    }
}

/// Inside a project, the compiler `cuno.toml` names is diagnosed, and the
/// source says so; with nothing named, MSVC by automatic selection.
///
/// Mutation: ignore the project's layers, or name another source; this test
/// fails.
#[test]
fn inside_a_project_its_choice_is_diagnosed() {
    let manifest = "[package]\nname = \"app\"\nversion = \"0.1.0\"\n";
    if require(Compiler::Clang, Language::C, "x.c", "x.exe").is_some() {
        let expected = healthy(&selected("clang"), "[toolchain] compiler in cuno.toml");
        Project::new()
            .file(
                "cuno.toml",
                &format!("{manifest}[toolchain]\ncompiler = \"clang\"\n"),
            )
            .cuno(&["doctor"])
            .assert(0, "", &expected);
    }
    if require(Compiler::Msvc, Language::C, "x.c", "x.exe").is_some() {
        let expected = healthy(&selected("msvc"), "automatic selection");
        Project::new()
            .file("cuno.toml", manifest)
            .cuno(&["doctor"])
            .assert(0, "", &expected);
    }
}

/// An unsupported compiler is a problem naming where it was chosen, and the
/// last line counts it.
///
/// Mutation: fall back to automatic selection for `gcc`, or exit 0; this
/// test fails.
#[test]
fn an_unsupported_compiler_is_a_problem() {
    Project::new()
        .cuno(&["doctor", "--compiler", "gcc"])
        .assert(
            1,
            "",
            "error: gcc is not supported yet (chosen by --compiler)\n\
         help: choose msvc, clang or clang-cl\n    \
         Finished 1 problem found\n",
        );
}

/// Without Visual Studio, the missing vswhere is one problem with its help,
/// not repeated as "no Visual Studio" by the selection, and doctor still
/// finishes.
///
/// Mutation: stop at the first problem, report the selection's
/// `NoVisualStudio` as well, or exit 0; this test fails.
#[test]
fn a_missing_visual_studio_is_reported_not_fatal() {
    let project = Project::new();
    let empty = project.path().join("empty");
    std::fs::create_dir(&empty).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_cuno"));
    cmd.args(["doctor", "--compiler", "msvc"])
        .env("ProgramFiles(x86)", &empty);
    let run: Run = project.run_command(cmd);
    let vswhere = empty.join(r"Microsoft Visual Studio\Installer\vswhere.exe");
    let help = "help: install Build Tools for Visual Studio with the \"Desktop development with C++\" workload: https://visualstudio.microsoft.com/visual-cpp-build-tools/\n";
    run.assert(
        1,
        "",
        &format!(
            "error: vswhere.exe was not found at {}\n{help}    \
             Finished 1 problem found\n",
            vswhere.display()
        ),
    );
}

/// A configuration file cuno cannot read is a problem with its place, and
/// the compiler is still diagnosed.
///
/// Mutation: stop at the configuration error; this test fails.
#[test]
fn a_broken_configuration_is_reported_and_diagnosis_goes_on() {
    if require(Compiler::Msvc, Language::C, "x.c", "x.exe").is_none() {
        return;
    }
    let run = Project::new()
        .file("cuno.toml", "[package]\nname = 1\n")
        .cuno(&["doctor", "--compiler", "msvc"]);
    let expected = healthy(&selected("msvc"), "--compiler").replace(
        "    Finished no problems found\n",
        "    Finished 1 problem found\n",
    );
    let first =
        "error: invalid type: integer `1`, expected a string in cuno.toml\n --> cuno.toml:2:8\n";
    assert_eq!(run.code, Some(1), "{}", run.stderr);
    assert_eq!(run.stderr, format!("{first}{expected}"));
}

/// A manifest for the tests' projects, with `extra` appended.
fn manifest(extra: &str) -> String {
    format!("[package]\nname = \"app\"\nversion = \"0.1.0\"\n{extra}")
}

/// The part of `healthy`'s output before the standard library check.
fn before_check(healthy: &str) -> &str {
    healthy.split_once("    Accepted").unwrap().0
}

/// When a configuration file cannot be read and nothing outside the files
/// names a compiler, the automatic selection says it is not the
/// configuration's choice; `CUNO_COMPILER` still applies.
///
/// Mutation: print the plain `automatic selection`, or drop `CUNO_COMPILER`
/// from the fallback; this test fails.
#[test]
fn an_unreadable_configuration_does_not_pass_for_its_choice() {
    let broken = "error: key with no value, expected `=` in .cuno\\config.toml\n --> .cuno\\config.toml:1:4\n";
    let project = || {
        Project::new()
            .file(
                "cuno.toml",
                &manifest("[toolchain]\ncompiler = \"clang\"\n"),
            )
            .file(".cuno/config.toml", "bad[\n")
    };
    if require(Compiler::Msvc, Language::C, "x.c", "x.exe").is_some() {
        let expected = healthy(
            &selected("msvc"),
            "automatic selection; the configuration could not be read",
        )
        .replace("no problems found", "1 problem found");
        project()
            .cuno(&["doctor"])
            .assert(1, "", &format!("{broken}{expected}"));
    }
    if require(Compiler::ClangCl, Language::C, "x.c", "x.exe").is_some() {
        let expected = healthy(&selected("clang-cl"), "CUNO_COMPILER")
            .replace("no problems found", "1 problem found");
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_cuno"));
        cmd.arg("doctor");
        project()
            .run_command_with(cmd, &[("CUNO_COMPILER", "clang-cl")])
            .assert(1, "", &format!("{broken}{expected}"));
    }
}

/// A configuration warning is printed with its place before the diagnosis,
/// and is not a problem.
///
/// Mutation: drop the warnings, or count them as problems; this test fails.
#[test]
fn configuration_warnings_come_first() {
    if require(Compiler::Msvc, Language::C, "x.c", "x.exe").is_none() {
        return;
    }
    let expected = healthy(&selected("msvc"), "--compiler");
    Project::new()
        .file("cuno.toml", &manifest("foo = 1\n"))
        .cuno(&["doctor", "--compiler", "msvc"])
        .assert(
            0,
            "",
            &format!(
                "warning: unknown key `package.foo` in cuno.toml, ignored\n --> cuno.toml:4:1\n{expected}"
            ),
        );
}

/// `[toolchain.version]` and `[toolchain.paths]` decide what is diagnosed,
/// as they decide what a build uses.
///
/// Mutation: pass doctor's selection no constraints, no LLVM paths or no
/// MSVC path; this test fails.
#[test]
fn the_toolchain_settings_constrain_the_selection() {
    if require(Compiler::Clang, Language::C, "x.c", "x.exe").is_none() {
        return;
    }
    let version = selected("clang").version;
    Project::new()
        .file("cuno.toml", &manifest("[toolchain.version]\nclang = \">=99\"\n"))
        .cuno(&["doctor", "--compiler", "clang"])
        .assert(
            1,
            "",
            &format!(
                "error: no clang found satisfies >=17.0.0 (cuno's minimum) and >=99 ([toolchain.version] clang); found {version}\n\
                 help: install a clang that satisfies it, or change [toolchain.version] clang\n    \
                 Finished 1 problem found\n"
            ),
        );
    Project::new()
        .file("cuno.toml", &manifest(""))
        .file(
            ".cuno/config.toml",
            "[toolchain.paths]\nclang = \"C:/nope/clang.exe\"\nmsvc = \"C:/nope/VS\"\n",
        )
        .cuno(&["doctor", "--compiler", "clang"])
        .assert(
            1,
            "",
            "error: [toolchain.paths] clang is C:/nope/clang.exe, but there is no file there\n\
             help: correct [toolchain.paths] clang, or remove it to let cuno find the tool\n\
             error: [toolchain.paths] msvc is C:/nope/VS, but no Visual Studio with the C++ x64 tools is installed there\n\
             help: correct [toolchain.paths] msvc, or remove it to let cuno find Visual Studio\n    \
             Finished 2 problems found\n",
        );
}

/// A C++23 project is a problem on a cl older than 19.52 and fine on a newer
/// one: what the build would refuse, doctor reports.
///
/// Mutation: check another edition than the project's, or drop the check;
/// this test fails on a cl older than 19.52 (as on CI).
#[test]
fn an_edition_the_compiler_lacks_is_a_problem() {
    use cuno_core::CunoError;
    use cuno_toolchain::flags::{Edition, check_edition};
    if require(Compiler::Msvc, Language::C, "x.c", "x.exe").is_none() {
        return;
    }
    let msvc = selected("msvc");
    let healthy = healthy(&msvc, "--compiler");
    let (code, expected) = match check_edition(msvc.kind, msvc.version, Edition::Cxx23) {
        Ok(()) => (0, healthy),
        Err(error) => {
            // After the Selected line and the executable's line.
            let at = healthy.match_indices('\n').nth(1).unwrap().0 + 1;
            let problem = format!("error: {error}\nhelp: {}\n", error.help().unwrap());
            let out = format!("{}{problem}{}", &healthy[..at], &healthy[at..]);
            (1, out.replace("no problems found", "1 problem found"))
        }
    };
    Project::new()
        .file("cuno.toml", &manifest("edition = \"c++23\"\n"))
        .cuno(&["doctor", "--compiler", "msvc"])
        .assert(code, "", &expected);
}

/// In a C project the standard library check is not made (toolchain.md,
/// "Minimum compiler versions").
///
/// Mutation: make the check in C projects too; this test fails.
#[test]
fn a_c_project_is_not_checked_against_the_standard_library() {
    if require(Compiler::Clang, Language::C, "x.c", "x.exe").is_none() {
        return;
    }
    let expected = healthy(&selected("clang"), "--compiler")
        .replace("    Accepted by the MSVC standard library\n", "");
    Project::new()
        .file("cuno.toml", &manifest("edition = \"c11\"\nlang = \"c\"\n"))
        .cuno(&["doctor", "--compiler", "clang"])
        .assert(0, "", &expected);
}

/// The standard library check compiles with the project's dev-profile flags
/// and `src` on the include path: a `src/yvals_core.h` failing with a
/// message that tells the profiles apart is what doctor reports, as a
/// problem whose help points at the flags.
///
/// Mutation: probe with the release profile, without `src`, or treat the
/// probe's failure as passing; this test fails.
#[test]
fn the_check_uses_the_builds_dev_flags_and_reports_its_failure() {
    if require(Compiler::ClangCl, Language::C, "x.c", "x.exe").is_none() {
        return;
    }
    let run = Project::new()
        .file("cuno.toml", &manifest(""))
        .file(
            "src/yvals_core.h",
            "#ifdef NDEBUG\n#error release flags reached the probe\n#else\n#error dev flags reached the probe\n#endif\n",
        )
        .cuno(&["doctor", "--compiler", "clang-cl"]);
    let healthy = healthy(&selected("clang-cl"), "--compiler");
    assert_eq!(run.code, Some(1), "{}", run.stderr);
    let rest = run
        .stderr
        .strip_prefix(before_check(&healthy))
        .unwrap_or_else(|| panic!("{}", run.stderr));
    assert!(
        rest.starts_with(
            "error: could not tell whether clang-cl works with the MSVC standard library:\n"
        ),
        "{rest}"
    );
    assert!(
        rest.contains("error: dev flags reached the probe"),
        "{rest}"
    );
    assert!(
        rest.ends_with(
            "help: check `[build.<compiler>] flags` in cuno.toml, and the Visual Studio and LLVM installations\n    \
             Finished 1 problem found\n"
        ),
        "{rest}"
    );
}

/// A temporary directory that does not exist is a problem saying so, with
/// its own help, and is not created.
///
/// Mutation: create the directory with its parents, or report it with the
/// probe's help; this test fails.
#[test]
fn a_missing_temporary_directory_is_reported_not_created() {
    if require(Compiler::Clang, Language::C, "x.c", "x.exe").is_none() {
        return;
    }
    let project = Project::new();
    let missing = project.home().join("missing");
    let temp = missing.join("temp");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_cuno"));
    cmd.args(["doctor", "--compiler", "clang"])
        .env("TEMP", &temp)
        .env("TMP", &temp);
    let run = project.run_command(cmd);
    let healthy = healthy(&selected("clang"), "--compiler");
    assert_eq!(run.code, Some(1), "{}", run.stderr);
    let rest = run
        .stderr
        .strip_prefix(before_check(&healthy))
        .unwrap_or_else(|| panic!("{}", run.stderr));
    let error = format!("error: cannot create {}\\cuno-doctor-", temp.display());
    assert!(rest.starts_with(&error), "{rest}");
    assert!(
        rest.ends_with(
            "help: set TEMP to a directory that exists and can be written\n    \
             Finished 1 problem found\n"
        ),
        "{rest}"
    );
    assert!(!missing.exists(), "the temporary directory was created");
}
