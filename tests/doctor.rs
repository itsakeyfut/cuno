//! `cuno doctor` run as a process, its whole stderr compared with what the
//! spec says it prints (`docs/en/specs/commands.md`, "What `cuno doctor`
//! prints"). The expected lines are built from cuno's own detection and
//! selection, which `tests/toolchain_detection.rs` already holds to the
//! independent reference; what is tested here is the command: what it
//! chooses, what it prints, and how it exits.
#![cfg(windows)]

mod support;

use std::path::Path;
use std::process::Command;

use cuno_core::Env;
use cuno_toolchain::llvm::{ExplicitPaths, LlvmTool, Tool, find_llvm_tools};
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

/// What cuno detects here.
fn detected() -> (Vec<msvc::VsInstallation>, Vec<LlvmTool>) {
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
    (installs, llvm)
}

/// What cuno's selection gives for `name` here.
fn selected(name: &str) -> Selected {
    let (installs, llvm) = detected();
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
    out + &found_tools() + &others(Some(selected.kind)) + "    Finished no problems found\n"
}

/// The `Found` lines for clang-format and clang-tidy, as found here.
fn found_tools() -> String {
    let (_, llvm) = detected();
    let mut out = String::new();
    for tool in [Tool::ClangFormat, Tool::ClangTidy] {
        let chosen = select::select_tool(tool, &llvm, HostOs::Windows).unwrap();
        out += &format!("       Found {} {}\n", tool.name(), chosen.version);
    }
    out
}

/// The `Info` line for `kind`: MSVC as selected with Visual Studio, a Clang
/// by the copy detected.
fn info(kind: select::CompilerKind) -> String {
    let (_, llvm) = detected();
    let version = match kind {
        select::CompilerKind::Msvc => selected("msvc").version,
        select::CompilerKind::Clang => {
            select::select_tool(Tool::Clang, &llvm, HostOs::Windows)
                .unwrap()
                .version
        }
        select::CompilerKind::ClangCl => {
            select::select_tool(Tool::ClangCl, &llvm, HostOs::Windows)
                .unwrap()
                .version
        }
    };
    format!("        Info {} {version} (not selected)\n", kind.name())
}

/// The `Info` lines for the compilers other than `selected`.
fn others(selected: Option<select::CompilerKind>) -> String {
    [
        select::CompilerKind::Msvc,
        select::CompilerKind::Clang,
        select::CompilerKind::ClangCl,
    ]
    .into_iter()
    .filter(|k| Some(*k) != selected)
    .map(info)
    .collect()
}

/// Whether clang-format and clang-tidy are here; the tests whose output
/// lists them are skipped without.
fn llvm_tools_here() -> bool {
    let (_, llvm) = detected();
    [Tool::ClangFormat, Tool::ClangTidy]
        .into_iter()
        .all(|t| select::select_tool(t, &llvm, HostOs::Windows).is_ok())
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
    if !llvm_tools_here() || require(Compiler::Msvc, Language::C, "x.c", "x.exe").is_none() {
        return;
    }
    Project::new()
        .cuno(&["doctor", "--compiler", "gcc"])
        .assert(
            1,
            "",
            &format!(
                "error: gcc is not supported yet (chosen by --compiler)\n\
                 help: choose msvc, clang or clang-cl\n{}{}    \
                 Finished 1 problem found\n",
                found_tools(),
                others(None)
            ),
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
    let help = "help: install Build Tools for Visual Studio with the \"Desktop development with C++\" workload: https://visualstudio.microsoft.com/visual-cpp-build-tools/
";

    // Without Visual Studio its bundled LLVM is not found either (on CI it
    // is the only one), so what is expected is what is found without it.
    let llvm = find_llvm_tools(
        &env(),
        HostOs::Windows,
        &ExplicitPaths::default(),
        &[],
        &SystemRunner::new(),
    )
    .unwrap();
    let mut problems = 1;
    let mut tools = String::new();
    for tool in [Tool::ClangFormat, Tool::ClangTidy] {
        tools += &match select::select_tool(tool, &llvm, HostOs::Windows) {
            Ok(chosen) => format!("       Found {} {}\n", tool.name(), chosen.version),
            Err(error) => {
                use cuno_core::CunoError;
                problems += 1;
                format!("error: {error}\nhelp: {}\n", error.help().unwrap())
            }
        };
    }
    let mut info = String::new();
    for tool in [Tool::Clang, Tool::ClangCl] {
        info += &match select::select_tool(tool, &llvm, HostOs::Windows) {
            Ok(chosen) => format!(
                "        Info {} {} (not selected)\n",
                tool.name(),
                chosen.version
            ),
            Err(_) => format!("        Info {} not found\n", tool.name()),
        };
    }
    let finished = if problems == 1 {
        "1 problem found".to_owned()
    } else {
        format!("{problems} problems found")
    };
    run.assert(
        1,
        "",
        &format!(
            // MSVC is the compiler asked for, so it has no Info line.
            "error: vswhere.exe was not found at {}\n{help}{tools}{info}    \
             Finished {finished}\n",
            vswhere.display(),
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
/// as they decide what a build uses. When a configured LLVM path has no
/// file, nothing was looked for, so no Clang is said to be missing.
///
/// Mutation: pass doctor's selection no constraints, no LLVM paths or no
/// MSVC path, or print `Info clang not found` after the configured path
/// failed; this test fails.
#[test]
fn the_toolchain_settings_constrain_the_selection() {
    if !llvm_tools_here() || require(Compiler::Clang, Language::C, "x.c", "x.exe").is_none() {
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
                 help: install a clang that satisfies it, or change [toolchain.version] clang\n{}{}    \
                 Finished 1 problem found\n",
                found_tools(),
                // clang is the compiler asked for, so it has no Info line.
                others(Some(select::CompilerKind::Clang))
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

/// In a C project the standard library check is not made, for the compiler
/// or for clang-tidy (toolchain.md, "Minimum compiler versions"): a
/// `src/yvals_core.h` that would reject both is never read.
///
/// Mutation: make either check in C projects too; this test fails.
#[test]
fn a_c_project_is_not_checked_against_the_standard_library() {
    if require(Compiler::Clang, Language::C, "x.c", "x.exe").is_none() {
        return;
    }
    let expected = healthy(&selected("clang"), "--compiler")
        .replace("    Accepted by the MSVC standard library\n", "");
    Project::new()
        .file("cuno.toml", &manifest("edition = \"c11\"\nlang = \"c\"\n"))
        .file("src/yvals_core.h", STL1000)
        .cuno(&["doctor", "--compiler", "clang"])
        .assert(0, "", &expected);
}

/// The standard library check compiles with the project's dev-profile flags
/// and `src` on the include path: a `src/yvals_core.h` failing with a
/// message that tells the profiles apart is what doctor reports, as a
/// problem whose help points at the flags.
///
/// Mutation: probe with the release profile, without `src`, or treat the
/// probe's failure as passing, for the compiler or for clang-tidy; this
/// test fails.
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
        rest.contains(
            "error: could not tell whether clang-tidy works with the MSVC standard library:\n"
        ),
        "{rest}"
    );
    assert_eq!(
        rest.matches("dev flags reached the probe").count(),
        4,
        "{rest}"
    );
    assert_eq!(
        rest.matches("help: check `[build.<compiler>] flags` in cuno.toml, and the Visual Studio and LLVM installations\n").count(),
        2,
        "{rest}"
    );
    assert!(rest.ends_with("    Finished 2 problems found\n"), "{rest}");
}

/// A temporary directory that does not exist is a problem saying so, with
/// its own help, and is not created.
///
/// Mutation: create the directory with its parents, report it with the
/// probe's help, or name the wrong check; this test fails.
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
    // Once for the compiler's check, once for clang-tidy's, each named.
    assert_eq!(rest.matches(&error).count(), 2, "{rest}");
    assert_eq!(rest.matches(" to check clang: ").count(), 1, "{rest}");
    assert_eq!(rest.matches(" to check clang-tidy: ").count(), 1, "{rest}");
    assert_eq!(
        rest.matches("help: set TEMP to a directory that exists and can be written\n")
            .count(),
        2,
        "{rest}"
    );
    assert!(rest.ends_with("    Finished 2 problems found\n"), "{rest}");
    assert!(!missing.exists(), "the temporary directory was created");
}

/// A `src/yvals_core.h` that rejects every compiler as the MSVC standard
/// library does, for a project's checks to read.
const STL1000: &str =
    "#error STL1000: Unexpected compiler version, expected Clang 99.0.0 or newer.\n";

/// clang-tidy is checked against the standard library with the build's
/// flags even when MSVC is selected, and a rejection is a problem with the
/// version to install.
///
/// Mutation: skip clang-tidy's check, run it only for Clang, give it no
/// `src`, or report it as the compiler; this test fails.
#[test]
fn clang_tidy_is_checked_against_the_standard_library() {
    if !llvm_tools_here() || require(Compiler::Msvc, Language::C, "x.c", "x.exe").is_none() {
        return;
    }
    let (_, llvm) = detected();
    let tidy = select::select_tool(Tool::ClangTidy, &llvm, HostOs::Windows)
        .unwrap()
        .version;
    let expected = healthy(&selected("msvc"), "--compiler")
        .replace(
            &format!("       Found clang-tidy {tidy}\n"),
            &format!(
                "       Found clang-tidy {tidy}\n\
                 error: clang-tidy {tidy} is not accepted by the MSVC standard library in use (STL1000)\n\
                 help: install clang-tidy 99.0.0 or newer\n"
            ),
        )
        .replace("no problems found", "1 problem found");
    Project::new()
        .file("cuno.toml", &manifest(""))
        .file("src/yvals_core.h", STL1000)
        .cuno(&["doctor", "--compiler", "msvc"])
        .assert(1, "", &expected);
}

/// Without LLVM anywhere cuno looks, clang-format and clang-tidy missing
/// are problems with their help, and the Clangs not selected are only
/// information.
///
/// Mutation: make a missing clang-format or clang-tidy information, or a
/// missing unselected compiler a problem; this test fails.
#[test]
fn missing_llvm_tools_are_problems_and_missing_compilers_are_not() {
    let project = Project::new();
    let empty = project.home().join("empty");
    std::fs::create_dir(&empty).unwrap();
    let mut cmd = without_llvm(&empty, None);
    cmd.args(["doctor", "--compiler", "msvc"]);
    let run = project.run_command(cmd);
    let vswhere = empty.join(r"Microsoft Visual Studio\Installer\vswhere.exe");
    let llvm_help = |tool: &str| {
        format!(
            "help: install LLVM (https://github.com/llvm/llvm-project/releases), or write where {tool} is in [toolchain.paths] {tool}\n"
        )
    };
    run.assert(
        1,
        "",
        &format!(
            "error: vswhere.exe was not found at {}\n\
             help: install Build Tools for Visual Studio with the \"Desktop development with C++\" workload: https://visualstudio.microsoft.com/visual-cpp-build-tools/\n\
             error: no clang-format was found\n{}\
             error: no clang-tidy was found\n{}        \
             Info clang not found\n        \
             Info clang-cl not found\n    \
             Finished 3 problems found\n",
            vswhere.display(),
            llvm_help("clang-format"),
            llvm_help("clang-tidy"),
        ),
    );
}

/// A batch file standing in for an LLVM tool: it prints `version` (the
/// lines `--version` would) and exits with `code`, whatever it is asked.
fn fake_tool(version: &str, code: i32) -> String {
    let lines: String = version.lines().map(|l| format!("echo {l}\r\n")).collect();
    format!("@echo off\r\n{lines}exit /b {code}\r\n")
}

/// `[toolchain.paths]` entries for `tools` in `project`, each a batch file.
fn configured(project: &Project, tools: &[(&str, &str)]) -> String {
    let mut out = String::from("[toolchain.paths]\n");
    for (tool, _) in tools {
        let path = project.path().join(format!("fake/{tool}.cmd"));
        out += &format!("{tool} = '{}'\n", path.display());
    }
    out
}

/// Each `Found` and `Info` line names the copy that tool's own selection
/// picks: a configured one, even when older than the copy installed, with
/// its version, and never another tool's.
///
/// Mutation: report one tool's copy for another (clang-cl's for clang, or
/// clang-tidy's for clang-format), or let a newer copy found win over the
/// configured one; this test fails.
#[test]
fn each_tool_line_names_its_own_configured_copy() {
    if require(Compiler::Msvc, Language::C, "x.c", "x.exe").is_none() {
        return;
    }
    let tools = [
        ("clang-format", "clang-format version 19.1.1"),
        ("clang-tidy", "LLVM version 19.1.2"),
        (
            "clang",
            "clang version 18.1.3\nTarget: x86_64-pc-windows-msvc",
        ),
        (
            "clang-cl",
            "clang version 18.1.4\nTarget: x86_64-pc-windows-msvc",
        ),
    ];
    let mut project = Project::new().file("cuno.toml", &manifest(""));
    let config = configured(&project, &tools);
    for (tool, version) in tools {
        project = project.file(&format!("fake/{tool}.cmd"), &fake_tool(version, 0));
    }
    let msvc = selected("msvc");
    let expected = format!(
        "    Selected msvc {} (--compiler)\n             {}\n       \
         Using Windows SDK {}\n       \
         Found clang-format 19.1.1\n       \
         Found clang-tidy 19.1.2\n        \
         Info clang 18.1.3 (not selected)\n        \
         Info clang-cl 18.1.4 (not selected)\n    \
         Finished no problems found\n",
        msvc.version,
        msvc.executable.display(),
        sdk()
    );
    project
        .file(".cuno/config.toml", &config)
        .cuno(&["doctor", "--compiler", "msvc"])
        .assert(0, "", &expected);
}

/// A configured clang-tidy that does not run is a problem naming the
/// setting; the clang-tidy installed is not used in its place.
///
/// Mutation: fall back to another copy when the configured one did not
/// run; this test fails.
#[test]
fn a_configured_tool_that_does_not_run_is_a_problem() {
    if !llvm_tools_here() || require(Compiler::Msvc, Language::C, "x.c", "x.exe").is_none() {
        return;
    }
    let project = Project::new().file("cuno.toml", &manifest(""));
    let config = configured(&project, &[("clang-tidy", "")]);
    let fake = project.path().join("fake/clang-tidy.cmd");
    let project = project
        .file("fake/clang-tidy.cmd", &fake_tool("", 1))
        .file(".cuno/config.toml", &config);
    let (_, llvm) = detected();
    let format = select::select_tool(Tool::ClangFormat, &llvm, HostOs::Windows)
        .unwrap()
        .version;
    let healthy = healthy(&selected("msvc"), "--compiler");
    let (before, _) = healthy.split_once("       Found").unwrap();
    let expected = format!(
        "{before}       Found clang-format {format}\n\
         error: [toolchain.paths] clang-tidy is {}, but it did not run\n\
         help: correct [toolchain.paths] clang-tidy, or remove it to let cuno find the tool\n{}    \
         Finished 1 problem found\n",
        fake.display(),
        others(Some(select::CompilerKind::Msvc))
    );
    project
        .cuno(&["doctor", "--compiler", "msvc"])
        .assert(1, "", &expected);
}

/// The standard library checks compile as the project's C++ edition: a
/// `src/yvals_core.h` that fails naming the `__cplusplus` it sees tells
/// which edition reached them.
///
/// Mutation: probe every project as one fixed edition; this test fails.
#[test]
fn the_checks_use_the_projects_edition() {
    if !llvm_tools_here() || require(Compiler::Clang, Language::C, "x.c", "x.exe").is_none() {
        return;
    }
    let header = "#if __cplusplus == 201703L\n#error probed as c++17\n\
                  #elif __cplusplus == 202002L\n#error probed as c++20\n\
                  #else\n#error probed as another edition\n#endif\n";
    for (edition, seen) in [("c++17", "probed as c++17"), ("c++20", "probed as c++20")] {
        let run = Project::new()
            .file(
                "cuno.toml",
                &manifest(&format!("edition = \"{edition}\"\n")),
            )
            .file("src/yvals_core.h", header)
            .cuno(&["doctor", "--compiler", "clang"]);
        assert_eq!(run.code, Some(1), "{}", run.stderr);
        // Once from clang's check, once from clang-tidy's.
        assert_eq!(
            run.stderr.matches(seen).count(),
            4,
            "{edition}: {}",
            run.stderr
        );
        assert!(!run.stderr.contains("another edition"), "{}", run.stderr);
    }
}

/// A configured clang is the compiler diagnosed even when an installed
/// one is newer, and one that does not run is a problem naming the setting,
/// not a reason to diagnose another.
///
/// Mutation: let the installed clang win over the configured one, or fall
/// back to it when the configured one does not run; this test fails.
#[test]
fn a_configured_compiler_is_the_one_diagnosed() {
    if require(Compiler::Clang, Language::C, "x.c", "x.exe").is_none() {
        return;
    }
    let project = Project::new().file("cuno.toml", &manifest(""));
    let config = configured(&project, &[("clang", "")]);
    let fake = project.path().join("fake/clang.cmd");
    let run = project
        .file(
            "fake/clang.cmd",
            &fake_tool("clang version 18.1.3\nTarget: x86_64-pc-windows-msvc", 0),
        )
        .file(".cuno/config.toml", &config)
        .cuno(&["doctor", "--compiler", "clang"]);
    let selected = format!(
        "    Selected clang 18.1.3 (--compiler)\n             {}\n",
        fake.display()
    );
    assert!(run.stderr.starts_with(&selected), "{}", run.stderr);

    let project = Project::new().file("cuno.toml", &manifest(""));
    let config = configured(&project, &[("clang", "")]);
    let fake = project.path().join("fake/clang.cmd");
    let run = project
        .file("fake/clang.cmd", &fake_tool("", 1))
        .file(".cuno/config.toml", &config)
        .cuno(&["doctor", "--compiler", "clang"]);
    let problem = format!(
        "error: [toolchain.paths] clang is {}, but it did not run\n\
         help: correct [toolchain.paths] clang, or remove it to let cuno find the tool\n",
        fake.display()
    );
    assert_eq!(run.code, Some(1), "{}", run.stderr);
    assert!(run.stderr.starts_with(&problem), "{}", run.stderr);
    assert!(!run.stderr.contains("Selected"), "{}", run.stderr);
    // Found by selection and by the Info lines alike, reported once.
    assert_eq!(run.stderr.matches(&problem).count(), 1, "{}", run.stderr);
}

/// A configured clang that cannot be used is a problem even while MSVC is
/// selected, in place of its `Info` line, so it is not first found when
/// switching to it (commands.md, "`cuno doctor`").
///
/// Mutation: list it as `Info clang not found`, or leave it out; this test
/// fails.
#[test]
fn an_unusable_configured_compiler_is_a_problem_even_unselected() {
    if !llvm_tools_here() || require(Compiler::Msvc, Language::C, "x.c", "x.exe").is_none() {
        return;
    }
    let project = Project::new().file("cuno.toml", &manifest(""));
    let config = configured(&project, &[("clang", "")]);
    let fake = project.path().join("fake/clang.cmd");
    let expected = healthy(&selected("msvc"), "--compiler")
        .replace(
            &info(select::CompilerKind::Clang),
            &format!(
                "error: [toolchain.paths] clang is {}, but it did not run\n\
                 help: correct [toolchain.paths] clang, or remove it to let cuno find the tool\n",
                fake.display()
            ),
        )
        .replace("no problems found", "1 problem found");
    project
        .file("fake/clang.cmd", &fake_tool("", 1))
        .file(".cuno/config.toml", &config)
        .cuno(&["doctor", "--compiler", "msvc"])
        .assert(1, "", &expected);
}

/// A configured clang below cuno's minimum is the same problem whether or
/// not clang is the compiler asked for, reported once, and never also
/// listed as an `Info` line.
///
/// Mutation: judge the `Info` lines without cuno's minimum, or give the
/// compiler asked for an `Info` line too; this test fails.
#[test]
fn a_configured_clang_below_the_minimum_is_one_problem_either_way() {
    if require(Compiler::Msvc, Language::C, "x.c", "x.exe").is_none() {
        return;
    }
    let project = || {
        let project = Project::new().file("cuno.toml", &manifest(""));
        let config = configured(&project, &[("clang", "")]);
        let fake = project.path().join("fake/clang.cmd");
        let project = project
            .file(
                "fake/clang.cmd",
                &fake_tool("clang version 16.0.6\nTarget: x86_64-pc-windows-msvc", 0),
            )
            .file(".cuno/config.toml", &config);
        (project, fake)
    };
    let problem = |fake: &Path| {
        format!(
            "error: [toolchain.paths] clang is {}, but 16.0.6 does not satisfy >=17.0.0 (cuno's minimum)\n\
             help: correct [toolchain.paths] clang, or remove it to let cuno find the tool\n",
            fake.display()
        )
    };
    for args in [&["doctor"][..], &["doctor", "--compiler", "clang"][..]] {
        let (project, fake) = project();
        let run = project.cuno(args);
        assert_eq!(run.code, Some(1), "{args:?}: {}", run.stderr);
        assert_eq!(
            run.stderr.matches(&problem(&fake)).count(),
            1,
            "{args:?}: {}",
            run.stderr
        );
        assert!(
            !run.stderr.contains("Info clang "),
            "{args:?}: {}",
            run.stderr
        );
    }
}

/// A `cuno` command that finds no LLVM tool and no Visual Studio: `PATH`
/// without any directory holding one (plus `extra`, first, when given), and
/// the install directories pointed at `empty`.
fn without_llvm(empty: &Path, extra: Option<&Path>) -> Command {
    let path: Vec<_> = extra
        .map(Path::to_path_buf)
        .into_iter()
        .chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap()).filter(|dir| {
                ![
                    "clang.exe",
                    "clang-cl.exe",
                    "clang-format.exe",
                    "clang-tidy.exe",
                ]
                .iter()
                .any(|tool| dir.join(tool).is_file())
            }),
        )
        .collect();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_cuno"));
    // Removed under the name it is inherited by (`Path` on Windows), or the
    // child would see the original beside the one set here.
    for (name, _) in std::env::vars_os() {
        if name.eq_ignore_ascii_case("PATH") {
            cmd.env_remove(name);
        }
    }
    cmd.env("PATH", std::env::join_paths(path).unwrap())
        .env("ProgramFiles", empty)
        // Windows gives a 64-bit process `ProgramFiles` from this one.
        .env("ProgramW6432", empty)
        .env("ProgramFiles(x86)", empty);
    cmd
}

/// A compiler that is found but cannot be used, and is not the one asked
/// for, is listed as `not usable` with the reason, not as `not found`, and
/// is no problem.
///
/// Mutation: list it as `not found`, or count it as a problem; this test
/// fails.
#[test]
fn a_found_but_unusable_compiler_is_listed_with_its_reason() {
    let project = Project::new();
    let empty = project.home().join("empty");
    let bin = project.home().join("old-llvm");
    std::fs::create_dir_all(&empty).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(
        bin.join("fake.c"),
        "#include <stdio.h>
int main(void) { puts(\"clang version 16.0.6\");          puts(\"Target: x86_64-pc-windows-msvc\"); return 0; }
",
    )
    .unwrap();
    let Some(mut compile) = require(Compiler::Msvc, Language::C, "fake.c", "clang.exe") else {
        return;
    };
    let built = compile.current_dir(&bin).output().unwrap();
    assert!(built.status.success(), "{built:?}");

    let mut cmd = without_llvm(&empty, Some(&bin));
    cmd.args(["doctor", "--compiler", "msvc"]);
    let run = project.run_command(cmd);
    assert!(
        run.stderr.contains(
            "        Info clang not usable: no clang found satisfies >=17.0.0 (cuno's minimum); found 16.0.6
"
        ),
        "{}",
        run.stderr
    );
    assert!(
        run.stderr.contains(
            "        Info clang-cl not found
"
        ),
        "{}",
        run.stderr
    );
    // vswhere, clang-format and clang-tidy, not the old clang.
    assert!(
        run.stderr.ends_with(
            "    Finished 3 problems found
"
        ),
        "{}",
        run.stderr
    );
}

/// When clang is asked for and Visual Studio cannot be used, selection
/// stops before it looks at clang, yet a configured clang that cannot be
/// used is still reported; with no Visual Studio at all the MSVC `Info`
/// line says `not found`, and after a wrong `[toolchain.paths] msvc` there
/// is no MSVC line to contradict the error.
///
/// Mutation: skip the configured clang when selection fails at Visual
/// Studio, list a missing Visual Studio as `not usable`, or print an MSVC
/// `Info` line after its configured path was reported; this test fails.
#[test]
fn a_configured_clang_is_reported_when_visual_studio_stops_selection() {
    let fake_clang = "clang version 16.0.6\nTarget: x86_64-pc-windows-msvc";
    let below = |fake: &Path| {
        format!(
            "error: [toolchain.paths] clang is {}, but 16.0.6 does not satisfy >=17.0.0 (cuno's minimum)\n",
            fake.display()
        )
    };

    // No Visual Studio at all.
    let project = Project::new().file("cuno.toml", &manifest(""));
    let config = configured(&project, &[("clang", "")]);
    let fake = project.path().join("fake/clang.cmd");
    let empty = project.home().join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let project = project
        .file("fake/clang.cmd", &fake_tool(fake_clang, 0))
        .file(".cuno/config.toml", &config);
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_cuno"));
    cmd.args(["doctor", "--compiler", "clang"])
        .env("ProgramFiles(x86)", &empty);
    let run = project.run_command(cmd);
    assert_eq!(run.code, Some(1), "{}", run.stderr);
    assert_eq!(
        run.stderr.matches(&below(&fake)).count(),
        1,
        "{}",
        run.stderr
    );
    assert!(
        run.stderr.contains("        Info msvc not found\n"),
        "{}",
        run.stderr
    );

    // A wrong `[toolchain.paths] msvc`.
    let project = Project::new().file("cuno.toml", &manifest(""));
    let config = format!(
        "{}msvc = 'C:/nope/VS'\n",
        configured(&project, &[("clang", "")])
    );
    let fake = project.path().join("fake/clang.cmd");
    let run = project
        .file("fake/clang.cmd", &fake_tool(fake_clang, 0))
        .file(".cuno/config.toml", &config)
        .cuno(&["doctor", "--compiler", "clang"]);
    assert_eq!(run.code, Some(1), "{}", run.stderr);
    assert!(
        run.stderr.starts_with(
            "error: [toolchain.paths] msvc is C:/nope/VS, but no Visual Studio with the C++ x64 tools is installed there\n"
        ),
        "{}",
        run.stderr
    );
    assert_eq!(
        run.stderr.matches(&below(&fake)).count(),
        1,
        "{}",
        run.stderr
    );
    assert_eq!(
        run.stderr.matches("[toolchain.paths] msvc is").count(),
        1,
        "{}",
        run.stderr
    );
    assert!(!run.stderr.contains("Info msvc"), "{}", run.stderr);
}

/// `[toolchain.version]` holds only the compiler used: a requirement no
/// installed clang meets changes nothing while MSVC is asked for.
///
/// Mutation: apply `[toolchain.version]` to the `Info` lines; this test
/// fails.
#[test]
fn the_info_lines_ignore_toolchain_version() {
    if !llvm_tools_here() || require(Compiler::Msvc, Language::C, "x.c", "x.exe").is_none() {
        return;
    }
    let expected = healthy(&selected("msvc"), "--compiler");
    Project::new()
        .file(
            "cuno.toml",
            &manifest("[toolchain.version]\nclang = \">=99\"\nclang-cl = \">=99\"\n"),
        )
        .cuno(&["doctor", "--compiler", "msvc"])
        .assert(0, "", &expected);
}

/// The clang-cl asked for is checked like clang when Visual Studio stops
/// selection: a configured clang-cl that cannot be used is reported.
///
/// Mutation: check only clang there, not every LLVM compiler; this test
/// fails.
#[test]
fn a_configured_clang_cl_is_reported_when_visual_studio_stops_selection() {
    let project = Project::new().file("cuno.toml", &manifest(""));
    let config = configured(&project, &[("clang-cl", "")]);
    let fake = project.path().join("fake/clang-cl.cmd");
    let empty = project.home().join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let project = project
        .file(
            "fake/clang-cl.cmd",
            &fake_tool("clang version 16.0.6\nTarget: x86_64-pc-windows-msvc", 0),
        )
        .file(".cuno/config.toml", &config);
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_cuno"));
    cmd.args(["doctor", "--compiler", "clang-cl"])
        .env("ProgramFiles(x86)", &empty);
    let run = project.run_command(cmd);
    let below = format!(
        "error: [toolchain.paths] clang-cl is {}, but 16.0.6 does not satisfy >=17.0.0 (cuno's minimum)\n",
        fake.display()
    );
    assert_eq!(run.code, Some(1), "{}", run.stderr);
    assert_eq!(run.stderr.matches(&below).count(), 1, "{}", run.stderr);
}

/// The standard library check takes the selected compiler's own
/// `[build.<compiler>] flags`: a define written only there reaches a
/// `src/yvals_core.h` that fails on it, and one written for another
/// compiler does not.
///
/// Mutation: build the probe's flags from another compiler's table; this
/// test fails.
#[test]
fn the_check_takes_the_selected_compilers_own_flags() {
    let header = "#ifdef OWN_FLAGS\n#error own flags reached the probe\n#endif\n\
                  #ifdef OTHER_FLAGS\n#error another compiler's flags reached the probe\n#endif\n";
    for (name, compiler, own, other) in [
        ("clang", Compiler::Clang, "-DOWN_FLAGS", "/DOTHER_FLAGS"),
        (
            "clang-cl",
            Compiler::ClangCl,
            "/DOWN_FLAGS",
            "-DOTHER_FLAGS",
        ),
    ] {
        if require(compiler, Language::C, "x.c", "x.exe").is_none() {
            continue;
        }
        let others: String = ["msvc", "clang", "clang-cl"]
            .into_iter()
            .filter(|n| *n != name)
            .map(|n| format!("[build.{n}]\nflags = [\"{other}\"]\n"))
            .collect();
        let run = Project::new()
            .file(
                "cuno.toml",
                &manifest(&format!("[build.{name}]\nflags = [\"{own}\"]\n{others}")),
            )
            .file("src/yvals_core.h", header)
            .cuno(&["doctor", "--compiler", name]);
        assert_eq!(run.code, Some(1), "{name}: {}", run.stderr);
        assert!(
            run.stderr.contains("own flags reached the probe"),
            "{name}: {}",
            run.stderr
        );
        assert!(
            !run.stderr.contains("another compiler's flags"),
            "{name}: {}",
            run.stderr
        );
    }
}

/// The standard library check takes the host's own `[platform.<os>.build]`
/// layer: an include directory written for Windows, holding a
/// `yvals_core.h` that fails, is what the check reports, and the same
/// written for Linux is not read.
///
/// Mutation: build doctor's check for another host or OS than the one it
/// runs on; this test fails.
#[test]
fn the_check_takes_the_hosts_platform_layer() {
    if require(Compiler::ClangCl, Language::C, "x.c", "x.exe").is_none() {
        return;
    }
    let header = "#error poisoned yvals_core.h reached the probe\n";
    let windows = Project::new()
        .file(
            "cuno.toml",
            &manifest("[platform.windows.build]\ninclude-dirs = [\"poison\"]\n"),
        )
        .file("poison/yvals_core.h", header)
        .cuno(&["doctor", "--compiler", "clang-cl"]);
    assert_eq!(windows.code, Some(1), "{}", windows.stderr);
    assert!(
        windows
            .stderr
            .contains("poisoned yvals_core.h reached the probe"),
        "{}",
        windows.stderr
    );

    let linux = Project::new()
        .file(
            "cuno.toml",
            &manifest("[platform.linux.build]\ninclude-dirs = [\"poison\"]\n"),
        )
        .file("poison/yvals_core.h", header)
        .cuno(&["doctor", "--compiler", "clang-cl"]);
    assert!(!linux.stderr.contains("poisoned"), "{}", linux.stderr);
    assert!(
        linux
            .stderr
            .contains("    Accepted by the MSVC standard library\n"),
        "{}",
        linux.stderr
    );
}

/// Started below the project root, doctor probes the standard library,
/// with the compiler and with clang-tidy, where the build runs, the root,
/// so a relative path in the project's flags means what it means to the
/// build.
///
/// Mutation: probe, or run clang-tidy's check, in the directory doctor was
/// started in or in the scratch directory; this test fails.
#[test]
fn the_probe_runs_in_the_project_root() {
    if require(Compiler::ClangCl, Language::C, "x.c", "x.exe").is_none() {
        return;
    }
    let project = Project::new()
        .file(
            "cuno.toml",
            &manifest("[build.clang-cl]\nflags = [\"/FIsrc/cfg.h\"]\n"),
        )
        .file("src/cfg.h", "#define CFG 1\n")
        .file("src/main.cpp", "int main() {}\n");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_cuno"));
    cmd.args(["doctor", "--compiler", "clang-cl"])
        .current_dir(project.path().join("src"));
    let run = project.run_command(cmd);
    assert!(
        run.stderr
            .contains("    Accepted by the MSVC standard library\n"),
        "{}",
        run.stderr
    );
    // clang-tidy's check reads the same flags in the same place.
    if llvm_tools_here() {
        assert!(
            run.stderr.ends_with("Finished no problems found\n"),
            "{}",
            run.stderr
        );
    }
}

/// With MSVC selected, clang-tidy's standard library check reads the
/// build's flags as clang-cl does: a C++23 project built with link-time
/// optimization and errors for warnings reports no problem, where cl's
/// `/std:c++23` or `/GL` would be an unused argument to clang-cl and so an
/// error.
///
/// Mutation: give clang-tidy the cl command without the clang-cl reading,
/// or keep `/GL` in it; this test fails.
#[test]
fn clang_tidy_reads_an_msvc_projects_flags_as_clang_cl() {
    if !llvm_tools_here() || require(Compiler::Msvc, Language::C, "x.c", "x.exe").is_none() {
        return;
    }
    let project = Project::new()
        .file(
            "cuno.toml",
            &manifest(
                "edition = \"c++23\"\n[build]\nwarnings-as-errors = true\n[profile.dev]\nlto = true\n",
            ),
        )
        .file("src/main.cpp", "int main() {}\n");
    let run = project.cuno(&["doctor", "--compiler", "msvc"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert!(
        run.stderr.ends_with("Finished no problems found\n"),
        "{}",
        run.stderr
    );
}
