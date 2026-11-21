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

/// Without Visual Studio, both the missing vswhere and the missing Visual
/// Studio are problems with their help, and doctor still finishes.
///
/// Mutation: stop at the first problem, or exit 0; this test fails.
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
            "error: vswhere.exe was not found at {}\n{help}\
             error: no Visual Studio or Build Tools with the C++ x64 tools was found\n{help}    \
             Finished 2 problems found\n",
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
