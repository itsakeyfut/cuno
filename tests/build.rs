//! `cuno build` run as a process on the real compilers: what it builds, what
//! it prints and how it exits (`docs/en/specs/commands.md`, "`cuno build` /
//! `cuno check`").
#![cfg(windows)]

mod support;

use support::compilers::{Compiler, Language, require};
use support::{Project, Run};

const MANIFEST: &str = "[package]\nname = \"app\"\nversion = \"0.1.0\"\n";

/// A package with a body source, a header, the default executable and one
/// in `src/bin/`, each printing what the body computes.
fn package(extra: &str) -> Project {
    Project::new()
        .file("cuno.toml", &format!("{MANIFEST}{extra}"))
        // C linkage, so the C executable links with the C++ body.
        .file(
            "src/util.h",
            "#pragma once\n#ifdef __cplusplus\nextern \"C\"\n#endif\nint add(int, int);\n",
        )
        .file(
            "src/util.cpp",
            "#include \"util.h\"\nint add(int a, int b) { return a + b; }\n",
        )
        .file(
            "src/main.cpp",
            "#include <cstdio>\n#include \"util.h\"\nint main() { std::printf(\"app %d\\n\", add(2, 3)); }\n",
        )
        .file(
            "src/bin/tool.c",
            "#include <stdio.h>\n#include \"../util.h\"\nint main(void) { printf(\"tool %d\\n\", add(4, 5)); return 0; }\n",
        )
}

const COMPILERS: [(&str, Compiler); 3] = [
    ("msvc", Compiler::Msvc),
    ("clang", Compiler::Clang),
    ("clang-cl", Compiler::ClangCl),
];

fn here(compiler: Compiler) -> bool {
    require(compiler, Language::C, "x.c", "x.exe").is_some()
}

/// `stderr` is exactly a `Compiling` line for `compiler` and a `Finished`
/// line for `profile` described as `optimization`.
fn assert_built(run: &Run, compiler: &str, profile: &str, optimization: &str) {
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    let lines: Vec<&str> = run.stderr.lines().collect();
    assert_eq!(lines.len(), 2, "{}", run.stderr);
    assert!(
        lines[0].starts_with(&format!("   Compiling app v0.1.0 ({compiler} "))
            && lines[0].ends_with(')'),
        "{}",
        run.stderr
    );
    let finished = format!("    Finished `{profile}` profile [{optimization}] in ");
    assert!(
        lines[1].starts_with(&finished) && lines[1].ends_with('s'),
        "{}",
        run.stderr
    );
}

/// On every compiler, a package with a body, a header and two executables,
/// one of them C in a C++ package, builds; each executable links the body
/// and runs; and the output is the two lines and nothing else, cl's
/// file-name echo included.
///
/// Mutation: link a bin without the body, compile the C source as C++,
/// show cl's file-name line, or print any line the spec does not; this
/// test fails.
#[test]
fn each_compiler_builds_and_links_every_executable() {
    for (name, compiler) in COMPILERS {
        if !here(compiler) {
            continue;
        }
        let project = package("");
        let run = project.cuno(&["build", "--compiler", name]);
        assert_built(&run, name, "dev", "unoptimized + debuginfo");
        let debug = project.path().join("target/debug");
        for (exe, printed) in [("app.exe", "app 5\n"), ("tool.exe", "tool 9\n")] {
            let run = project.run(debug.join(exe), &[]);
            // The C runtime writes `\n` as `\r\n`.
            assert_eq!(
                (run.code, run.stdout.replace("\r\n", "\n")),
                (Some(0), printed.to_owned()),
                "{name} {exe}"
            );
        }
    }
}

/// `--release` builds into `target/release` and says so; `-q` prints
/// nothing on success.
///
/// Mutation: build release into `debug`, describe it as unoptimized, or
/// print status lines under `-q`; this test fails.
#[test]
fn release_and_quiet() {
    if !here(Compiler::Msvc) {
        return;
    }
    let project = package("");
    let run = project.cuno(&["build", "--release", "--compiler", "msvc"]);
    assert_built(&run, "msvc", "release", "optimized");
    assert!(project.path().join("target/release/app.exe").is_file());
    project
        .cuno(&["-q", "build", "--compiler", "msvc"])
        .assert(0, "", "");
}

/// A compile error shows the compiler's message, then `could not compile`,
/// and exits 1; a missing definition fails the link, `could not link`.
///
/// Mutation: hide the compiler's output, report a link failure as a
/// compile failure, or exit with another code; this test fails.
#[test]
fn failures_show_the_compiler_and_exit_1() {
    for (name, compiler) in COMPILERS {
        if !here(compiler) {
            continue;
        }
        let run = Project::new()
            .file("cuno.toml", MANIFEST)
            .file("src/main.cpp", "int main() { return undeclared; }\n")
            .cuno(&["build", "--compiler", name]);
        assert_eq!(run.code, Some(1), "{name}: {}", run.stderr);
        assert!(run.stderr.contains("undeclared"), "{name}: {}", run.stderr);
        assert!(
            run.stderr.ends_with("error: could not compile `app`\n"),
            "{name}: {}",
            run.stderr
        );

        let run = Project::new()
            .file("cuno.toml", MANIFEST)
            .file(
                "src/main.cpp",
                "int missing();\nint main() { return missing(); }\n",
            )
            .cuno(&["build", "--compiler", name]);
        assert_eq!(run.code, Some(1), "{name}: {}", run.stderr);
        assert!(
            run.stderr.ends_with("error: could not link `app`\n"),
            "{name}: {}",
            run.stderr
        );
    }
}

/// What cannot be built is refused before anything runs, exit 3.
///
/// Mutation: accept another profile or `--target`, or build outside a
/// project; this test fails.
#[test]
fn what_cannot_be_built_exits_3() {
    let project = package("");
    project.cuno(&["build", "--profile", "bench"]).assert(
        3,
        "",
        "error: no profile named `bench`\nhelp: use dev or release\n",
    );
    project
        .cuno(&["build", "--target", "aarch64-pc-windows-msvc"])
        .assert(3, "", "error: --target is not supported yet\n");
    let outside = Project::new();
    outside.cuno(&["build"]).assert(
        3,
        "",
        &format!(
            "error: could not find cuno.toml in {} or any parent directory\n\
             help: run it in a directory that holds a cuno.toml, or below one\n",
            outside.path().display()
        ),
    );
}

/// With `-v`, each step's command is shown before it runs.
///
/// Mutation: show nothing, or only the links; this test fails.
#[test]
fn verbose_shows_each_command() {
    if !here(Compiler::Clang) {
        return;
    }
    let run = package("").cuno(&["-v", "build", "--compiler", "clang"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    let running = run
        .stderr
        .lines()
        .filter(|l| l.starts_with("     Running `"))
        .count();
    // Three compiles and two links.
    assert_eq!(running, 5, "{}", run.stderr);
}

/// A setting in `cuno.toml` reaches `cuno doctor`'s standard library check
/// and the build's compiles alike, since both take their flags from one
/// place: a define written in `[build]` trips a `src/yvals_core.h` in both.
///
/// Mutation: build doctor's check or the build's compiles from other
/// settings; this test fails.
#[test]
fn a_setting_reaches_doctor_and_the_build_alike() {
    if !here(Compiler::ClangCl) {
        return;
    }
    let project = package("[build]\ndefines = [\"FROM_SETTINGS\"]\n").file(
        "src/yvals_core.h",
        "#ifdef FROM_SETTINGS\n#error FROM_SETTINGS reached the compile\n#endif\n#include_next <yvals_core.h>\n",
    );
    let doctor = project.cuno(&["doctor", "--compiler", "clang-cl"]);
    assert!(
        doctor.stderr.contains("FROM_SETTINGS reached the compile"),
        "{}",
        doctor.stderr
    );
    let build = project.cuno(&["build", "--compiler", "clang-cl"]);
    assert_eq!(build.code, Some(3), "{}", build.stderr);
    assert!(
        build.stderr.contains("FROM_SETTINGS reached the compile"),
        "{}",
        build.stderr
    );
}

/// A program that prints which of `NDEBUG`, `SNEAKED` and `CFG` it was
/// built with.
const REPORTS_DEFINES: &str = r#"#include <cstdio>
int main() {
#ifdef NDEBUG
    std::puts("NDEBUG");
#endif
#ifdef SNEAKED
    std::puts("SNEAKED");
#endif
#ifdef CFG
    std::puts("CFG");
#endif
    std::puts("ran");
}
"#;

/// The C runtime writes `\n` as `\r\n`.
fn printed(run: &Run) -> String {
    run.stdout.replace("\r\n", "\n")
}

/// `CL`, `_CL_`, `LINK` and the like would add flags no command line
/// shows; the build leaves them out and warns about each, so a dev build
/// stays without `NDEBUG` and a define from the environment does not
/// arrive.
///
/// Mutation: pass them through, or leave them out without a warning; this
/// test fails.
#[test]
fn compiler_option_variables_are_left_out_with_a_warning() {
    for (name, compiler) in [("msvc", Compiler::Msvc), ("clang-cl", Compiler::ClangCl)] {
        if !here(compiler) {
            continue;
        }
        let project = Project::new()
            .file("cuno.toml", MANIFEST)
            .file("src/main.cpp", REPORTS_DEFINES);
        let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_cuno"));
        cmd.args(["build", "--compiler", name])
            .env("CL", "/DSNEAKED")
            .env("_CL_", "/DNDEBUG");
        let run = project.run_command(cmd);
        assert_eq!(run.code, Some(0), "{name}: {}", run.stderr);
        assert!(
            run.stderr.contains(
                "warning: CL is set in the environment and is not used\nhelp: write the flags in cuno.toml\n\
                 warning: _CL_ is set in the environment and is not used\nhelp: write the flags in cuno.toml\n"
            ),
            "{name}: {}",
            run.stderr
        );
        let ran = project.run(project.path().join("target/debug/app.exe"), &[]);
        assert_eq!(printed(&ran), "ran\n", "{name}");
    }
}

/// The build runs the same wherever in the project it is started: a
/// relative path in `[build.<compiler>] flags` means the same to the
/// standard library check and to the compiles.
///
/// Mutation: run the check, or the compiles, in the directory cuno was
/// started in; this test fails.
#[test]
fn a_build_started_below_the_root_runs_in_the_root() {
    for (name, compiler, flags) in [
        ("clang", Compiler::Clang, r#"["-include", "src/cfg.h"]"#),
        ("clang-cl", Compiler::ClangCl, r#"["/FIsrc/cfg.h"]"#),
    ] {
        if !here(compiler) {
            continue;
        }
        let project = Project::new()
            .file(
                "cuno.toml",
                &format!("{MANIFEST}[build.{name}]\nflags = {flags}\n"),
            )
            .file("src/cfg.h", "#define CFG 1\n")
            .file("src/main.cpp", REPORTS_DEFINES);
        let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_cuno"));
        cmd.args(["build", "--compiler", name])
            .current_dir(project.path().join("src"));
        let run = project.run_command(cmd);
        assert_eq!(run.code, Some(0), "{name}: {}", run.stderr);
        let ran = project.run(project.path().join("target/debug/app.exe"), &[]);
        assert_eq!(printed(&ran), "CFG\nran\n", "{name}");
    }
}

/// `--target-dir` puts the build where it says, and `[term] verbose`
/// shows the commands as `-v` does.
///
/// Mutation: ignore `--target-dir`, or `[term] verbose`; this test fails.
#[test]
fn target_dir_and_configured_verbosity_apply() {
    if !here(Compiler::Msvc) {
        return;
    }
    let project = package("").file(".cuno/config.toml", "[term]\nverbose = true\n");
    let run = project.cuno(&["build", "--compiler", "msvc", "--target-dir", "out"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert!(project.path().join("out/debug/app.exe").is_file());
    assert!(!project.path().join("target").exists());
    assert!(run.stderr.contains("     Running `"), "{}", run.stderr);
}

/// `-v` shows the command each step runs: the compiler, the source's
/// language switch and the source.
///
/// Mutation: show another command, or none; this test fails.
#[test]
fn verbose_shows_the_command_run() {
    if !here(Compiler::Clang) {
        return;
    }
    let project = package("");
    let run = project.cuno(&["-v", "build", "--compiler", "clang"]);
    let source = project.path().join("src").join("util.cpp");
    let line = run
        .stderr
        .lines()
        .find(|l| l.contains(&source.display().to_string()))
        .unwrap_or_else(|| panic!("{}", run.stderr));
    assert!(line.starts_with("     Running `\""), "{line}");
    assert!(line.contains("clang.exe\" -c "), "{line}");
    assert!(line.contains(" -x c++ "), "{line}");
}

/// A C package builds and runs on every compiler, with no standard library
/// check (it uses no C++ standard library).
///
/// Mutation: link or check a C package as C++; this test fails.
#[test]
fn a_c_package_builds_on_every_compiler() {
    for (name, compiler) in COMPILERS {
        if !here(compiler) {
            continue;
        }
        let project = Project::new()
            .file(
                "cuno.toml",
                &format!("{MANIFEST}lang = \"c\"\n"),
            )
            .file("src/yvals_core.h", "#error the C++ check ran\n")
            .file(
                "src/main.c",
                "#include <stdio.h>\nint main(void) { printf(\"c %ld\\n\", __STDC_VERSION__); return 0; }\n",
            );
        let run = project.cuno(&["build", "--compiler", name]);
        assert_built(&run, name, "dev", "unoptimized + debuginfo");
        let ran = project.run(project.path().join("target/debug/app.exe"), &[]);
        assert_eq!(printed(&ran), "c 201710\n", "{name}");
    }
}

/// `-q` keeps what a failure needs: the compiler's message and the error.
///
/// Mutation: hide the compiler's output under `-q`; this test fails.
#[test]
fn quiet_still_shows_a_failure() {
    if !here(Compiler::Clang) {
        return;
    }
    let run = Project::new()
        .file("cuno.toml", MANIFEST)
        .file("src/main.cpp", "int main() { return undeclared; }\n")
        .cuno(&["-q", "build", "--compiler", "clang"]);
    assert_eq!(run.code, Some(1), "{}", run.stderr);
    assert!(run.stderr.contains("undeclared"), "{}", run.stderr);
    assert!(
        run.stderr.ends_with("error: could not compile `app`\n"),
        "{}",
        run.stderr
    );
}

/// Warnings from reading `cuno.toml` and from classifying the sources are
/// shown, and the build goes on.
///
/// Mutation: drop either kind of warning; this test fails.
#[test]
fn warnings_are_shown_and_the_build_goes_on() {
    if !here(Compiler::Msvc) {
        return;
    }
    let run = package("unknown = 1\n[[bin]]\nname = \"app\"\npath = \"src/main.cpp\"\n").cuno(&[
        "build",
        "--compiler",
        "msvc",
    ]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert!(
        run.stderr
            .contains("warning: unknown key `package.unknown` in cuno.toml, ignored"),
        "{}",
        run.stderr
    );
    assert!(
        run.stderr.contains(
            "warning: src/bin/tool.c is not built: [[bin]] is written and does not list it"
        ),
        "{}",
        run.stderr
    );
}

/// `--profile release` is the release profile, as `--release` is.
///
/// Mutation: build `--profile release` as dev; this test fails.
#[test]
fn profile_release_is_release() {
    if !here(Compiler::Msvc) {
        return;
    }
    let project = package("");
    let run = project.cuno(&["build", "--profile", "release", "--compiler", "msvc"]);
    assert_built(&run, "msvc", "release", "optimized");
}

/// An edition the cl lacks is refused before anything is built (on a cl
/// older than 19.52, as on CI; elsewhere it builds).
///
/// Mutation: build without checking the edition; this test fails on a cl
/// older than 19.52.
#[test]
fn an_edition_the_compiler_lacks_is_refused_before_building() {
    if !here(Compiler::Msvc) {
        return;
    }
    let run = package("edition = \"c++23\"\n").cuno(&["build", "--compiler", "msvc"]);
    if run.code == Some(3) {
        assert!(run.stderr.starts_with("error: "), "{}", run.stderr);
        assert!(!run.stderr.contains("Compiling"), "{}", run.stderr);
    } else {
        assert_built(&run, "msvc", "dev", "unoptimized + debuginfo");
    }
}

/// A source whose name is not ASCII builds with cl, and cl's echo of its
/// name, in the console's code page, is not shown.
///
/// Mutation: compare cl's first line as UTF-8; this test fails.
#[test]
fn a_non_ascii_source_name_shows_no_echo() {
    if !here(Compiler::Msvc) {
        return;
    }
    let project = package("").file("src/日本語.cpp", "int japanese() { return 1; }\n");
    let run = project.cuno(&["build", "--compiler", "msvc"]);
    assert_built(&run, "msvc", "dev", "unoptimized + debuginfo");
}

/// An output directory that cannot be made is named, not blamed on the
/// compiler.
///
/// Mutation: report it as the compiler failing to run; this test fails.
#[test]
fn an_output_directory_that_cannot_be_made_is_named() {
    if !here(Compiler::Msvc) {
        return;
    }
    let project = package("").file("target", "a file\n");
    let run = project.cuno(&["build", "--compiler", "msvc"]);
    assert_eq!(run.code, Some(1), "{}", run.stderr);
    assert!(run.stderr.contains("could not create "), "{}", run.stderr);
    assert!(!run.stderr.contains("could not run"), "{}", run.stderr);
}

/// A program that prints each argument in brackets and then its working
/// directory, and exits 7.
const REPORTS_ITS_RUN: &str = r#"#include <cstdio>
#include <filesystem>
int main(int argc, char** argv) {
    for (int i = 1; i < argc; i++) std::printf("[%s]\n", argv[i]);
    std::printf("%s\n", std::filesystem::current_path().string().c_str());
    return 7;
}
"#;

/// On every compiler, `cuno run` started below the root builds, says what
/// it runs, and runs the program in the project root with the arguments
/// after `--`; the program's exit code is cuno's.
///
/// Mutation: drop the arguments, run in the target directory or where cuno
/// was started, or return 0; this test fails.
#[test]
fn run_passes_the_arguments_root_and_exit_code() {
    for (name, compiler) in COMPILERS {
        if !here(compiler) {
            continue;
        }
        let project = Project::new()
            .file("cuno.toml", MANIFEST)
            .file("src/main.cpp", REPORTS_ITS_RUN);
        let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_cuno"));
        cmd.args(["run", "--compiler", name, "--", "a", "b c"])
            .current_dir(project.path().join("src"));
        let run = project.run_command(cmd);
        assert_eq!(run.code, Some(7), "{name}: {}", run.stderr);
        let root = std::fs::canonicalize(project.path()).unwrap();
        let shown = run.stdout.replace("\r\n", "\n");
        let (args, cwd) = shown
            .split_once("]\n[b c]\n")
            .unwrap_or_else(|| panic!("{shown}"));
        assert_eq!(args, "[a", "{name}");
        assert_eq!(
            std::fs::canonicalize(cwd.trim_end()).unwrap(),
            root,
            "{name}"
        );
        let exe = project.path().join("target").join("debug").join("app.exe");
        assert!(
            run.stderr
                .ends_with(&format!("     Running `{}`\n", exe.display())),
            "{name}: {}",
            run.stderr
        );
    }
}

/// Started in the root, the `Running` line names the executable relative
/// to it; `-q` prints nothing of cuno's own.
///
/// Mutation: always show the full path, or show it under `-q`; this test
/// fails.
#[test]
fn run_shows_the_executable_relative_and_nothing_when_quiet() {
    if !here(Compiler::Clang) {
        return;
    }
    let project = Project::new()
        .file("cuno.toml", MANIFEST)
        .file("src/main.cpp", "int main() { return 0; }\n");
    let run = project.cuno(&["run", "--compiler", "clang"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert!(
        run.stderr
            .ends_with("     Running `target\\debug\\app.exe`\n"),
        "{}",
        run.stderr
    );
    project
        .cuno(&["-q", "run", "--compiler", "clang"])
        .assert(0, "", "");
}

/// With several executables, `cuno run` needs `--bin`, and refuses a name
/// that is not one, both with the candidates, exit 2, and before building
/// anything; `--bin` runs the one it names.
///
/// Mutation: run the first executable, exit with another code, or build
/// before choosing; this test fails.
#[test]
fn run_chooses_with_bin() {
    let Some((name, _)) = COMPILERS.into_iter().find(|&(_, c)| here(c)) else {
        return;
    };
    let project = package("");
    project.cuno(&["run", "--compiler", name]).assert(
        2,
        "",
        "error: several executables can be run; choose one with --bin\n\
         help: available: app, tool\n",
    );
    project
        .cuno(&["run", "--compiler", name, "--bin", "nope"])
        .assert(
            2,
            "",
            "error: no executable named `nope`\nhelp: available: app, tool\n",
        );
    assert!(!project.path().join("target").exists());
    let run = project.cuno(&["run", "--compiler", name, "--bin", "tool"]);
    assert_eq!(
        (run.code, run.stdout.replace("\r\n", "\n")),
        (Some(0), "tool 9\n".to_owned()),
        "{}",
        run.stderr
    );
}

/// On every compiler, `cuno check` reports a compile error with the
/// compiler's message and exit 1; a clean package prints `Checking` and
/// `Finished`, writes no executable, and no object but on MSVC, which checks
/// by compiling (`build.md`, "Always-on flags"); and a second check with
/// nothing changed checks nothing.
///
/// Mutation: link in check mode, write objects on clang or clang-cl, report
/// success on an error, or check everything again; this test fails.
#[test]
fn check_writes_no_object_and_checks_only_what_changed() {
    for (name, compiler) in COMPILERS {
        if !here(compiler) {
            continue;
        }
        let broken = Project::new()
            .file("cuno.toml", MANIFEST)
            .file("src/main.cpp", "int main() { return undeclared; }\n");
        let run = broken.cuno(&["check", "--compiler", name]);
        assert_eq!(run.code, Some(1), "{name}: {}", run.stderr);
        assert!(run.stderr.contains("undeclared"), "{name}: {}", run.stderr);
        assert!(
            run.stderr.ends_with("error: could not compile `app`\n"),
            "{name}: {}",
            run.stderr
        );

        let project = package("");
        let run = project.cuno(&["check", "--compiler", name]);
        assert_eq!(run.code, Some(0), "{name}: {}", run.stderr);
        let lines: Vec<&str> = run.stderr.lines().collect();
        assert_eq!(lines.len(), 2, "{name}: {}", run.stderr);
        assert!(
            lines[0].starts_with(&format!("    Checking app v0.1.0 ({name} ")),
            "{}",
            run.stderr
        );
        assert!(
            lines[1].starts_with("    Finished `dev` profile"),
            "{}",
            run.stderr
        );
        for project in [&broken, &project] {
            let mut stack = vec![project.path().join("target")];
            while let Some(dir) = stack.pop() {
                for entry in std::fs::read_dir(&dir).unwrap() {
                    let path = entry.unwrap().path();
                    let ext = path.extension().and_then(|e| e.to_str());
                    let object = ext == Some("obj") && compiler != Compiler::Msvc;
                    assert!(!object && ext != Some("exe"), "{name}: {}", path.display());
                    if path.is_dir() {
                        stack.push(path);
                    }
                }
            }
        }

        let again = project.cuno(&["check", "--compiler", name]);
        assert_eq!(again.code, Some(0), "{name}: {}", again.stderr);
        assert_eq!(again.stderr.lines().count(), 1, "{name}: {}", again.stderr);
        assert!(
            again.stderr.starts_with("    Finished "),
            "{}",
            again.stderr
        );
    }
}

/// A `cuno run` whose build fails runs nothing, not even the executable an
/// earlier build left behind.
///
/// Mutation: carry on to the run when the build of `cuno run` fails; this
/// test fails.
#[test]
fn run_runs_nothing_when_the_build_fails() {
    for (name, compiler) in COMPILERS {
        if !here(compiler) {
            continue;
        }
        let project = Project::new().file("cuno.toml", MANIFEST).file(
            "src/main.cpp",
            "#include <cstdio>\nint main() { std::puts(\"old\"); }\n",
        );
        let built = project.cuno(&["build", "--compiler", name]);
        assert_eq!(built.code, Some(0), "{name}: {}", built.stderr);
        std::fs::write(
            project.path().join("src/main.cpp"),
            "int main() { return undeclared; }\n",
        )
        .unwrap();
        let run = project.cuno(&["run", "--compiler", name]);
        assert_eq!(run.code, Some(1), "{name}: {}", run.stderr);
        assert_eq!(run.stdout, "", "{name}");
        assert!(!run.stderr.contains("Running"), "{name}: {}", run.stderr);
    }
}

/// On every compiler, a check reads the headers its compiler reports, so
/// breaking a header after a clean check makes the next check fail.
///
/// Mutation: record a check without its headers; this test fails.
#[test]
fn check_sees_a_changed_header() {
    for (name, compiler) in COMPILERS {
        if !here(compiler) {
            continue;
        }
        let project = Project::new()
            .file("cuno.toml", MANIFEST)
            .file("src/h.h", "#define VALUE 0\n")
            .file(
                "src/main.cpp",
                "#include \"h.h\"\nint main() { return VALUE; }\n",
            );
        let run = project.cuno(&["check", "--compiler", name]);
        assert_eq!(run.code, Some(0), "{name}: {}", run.stderr);
        std::fs::write(project.path().join("src/h.h"), "#define VALUE undeclared\n").unwrap();
        let run = project.cuno(&["check", "--compiler", name]);
        assert_eq!(run.code, Some(1), "{name}: {}", run.stderr);
        assert!(run.stderr.contains("undeclared"), "{name}: {}", run.stderr);
    }
}

/// The program `cuno run` runs gets the environment cuno was given, not
/// the one cuno resolved for the compiler.
///
/// Mutation: run it with no environment, or with the compiler's; this test
/// fails.
#[test]
fn run_gives_the_program_cunos_environment() {
    if !here(Compiler::Msvc) {
        return;
    }
    let project = Project::new().file("cuno.toml", MANIFEST).file(
        "src/main.cpp",
        r#"#include <cstdio>
#include <cstdlib>
int main() {
    std::printf("probe=%s\n", std::getenv("RUN_PROBE") ? std::getenv("RUN_PROBE") : "-");
    std::printf("include=%s\n", std::getenv("INCLUDE") ? "set" : "-");
}
"#,
    );
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_cuno"));
    cmd.args(["run", "--compiler", "msvc"])
        .env("RUN_PROBE", "given")
        .env_remove("INCLUDE");
    let run = project.run_command(cmd);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(printed(&run), "probe=given\ninclude=-\n", "{}", run.stderr);
}

/// A build marks the output directory it makes, so `cuno clean` removes
/// it afterwards.
///
/// Mutation: build without marking the output directory; this test fails.
#[test]
fn a_build_marks_its_output_directory_for_clean() {
    let Some((name, _)) = COMPILERS.into_iter().find(|&(_, c)| here(c)) else {
        return;
    };
    let project = package("");
    let run = project.cuno(&["build", "--compiler", name]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    let run = project.cuno(&["clean"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert!(!project.path().join("target").exists());
}

/// The compile commands as `compile_commands.json` holds them: each entry's
/// source and arguments.
fn compile_commands(project: &Project) -> Vec<(String, Vec<String>)> {
    let text = std::fs::read_to_string(project.path().join("compile_commands.json"))
        .expect("compile_commands.json should be written");
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            let args = entry["arguments"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| a.as_str().unwrap().to_owned())
                .collect();
            (entry["file"].as_str().unwrap().to_owned(), args)
        })
        .collect()
}

/// A build writes `.clang-format` from `[fmt]` at the root and clang-format
/// reads it, `[fmt.clang-format]` included; a hand-written one is kept and
/// warned about, and one that cannot be written is a warning, the build
/// going on (docs/en/specs/dev-tools.md, "Configuration").
///
/// Mutation: stop writing it, overwrite a hand-written one, write YAML
/// clang-format refuses, or fail the build on a write error; this test
/// fails.
#[test]
fn a_build_writes_the_clang_format_editors_read() {
    let Some(clang_format) = support::compilers::require_llvm_tool("clang-format") else {
        return;
    };
    let Some((name, _)) = COMPILERS.iter().find(|(_, c)| here(*c)) else {
        return;
    };
    let project =
        package("[fmt]\ncolumn-limit = 90\n[fmt.clang-format]\nIndentCaseLabels = true\n")
            .file(".clang-format", "BasedOnStyle: Google\n");
    let file = project.path().join(".clang-format");
    let run = project.cuno(&["build", "--compiler", name]);
    assert_eq!(run.code, Some(0), "{name}: {}", run.stderr);
    assert!(
        run.stderr
            .contains("warning: kept the hand-written `.clang-format`; "),
        "{}",
        run.stderr
    );
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "BasedOnStyle: Google\n"
    );

    std::fs::remove_file(&file).unwrap();
    let run = project.cuno(&["build", "--compiler", name]);
    assert_eq!(run.code, Some(0), "{name}: {}", run.stderr);
    assert!(!run.stderr.contains("warning"), "{}", run.stderr);
    let dumped = std::process::Command::new(&clang_format)
        .arg("--dump-config")
        .arg(format!("--style=file:{}", file.display()))
        .output()
        .unwrap();
    let config = String::from_utf8_lossy(&dumped.stdout);
    assert!(
        dumped.status.success(),
        "{}",
        String::from_utf8_lossy(&dumped.stderr)
    );
    assert!(config.contains("ColumnLimit:     90"), "{config}");
    assert!(config.contains("IndentWidth:     4"), "{config}");
    assert!(config.contains("IndentCaseLabels: true"), "{config}");

    std::fs::remove_file(&file).unwrap();
    std::fs::create_dir(&file).unwrap();
    let run = project.cuno(&["build", "--compiler", name]);
    assert_eq!(run.code, Some(0), "{name}: {}", run.stderr);
    assert!(
        run.stderr
            .contains("warning: could not write .clang-format: "),
        "{}",
        run.stderr
    );
}

/// On every compiler, `compile_commands.json` lists every source with the
/// `dev` command and `CUNO_TEST` defined, even after a `--release` build,
/// each naming the test build's object as its `output`, and is written when
/// a compile fails (docs/en/specs/build.md, "Output").
///
/// Mutation: write it from the built profile's graph, without the test
/// define, with the program's objects as the outputs, leave a source out,
/// or only after the build succeeds; this test fails.
#[test]
fn compile_commands_hold_the_dev_commands_whatever_is_built() {
    for (name, compiler) in COMPILERS {
        if !here(compiler) {
            continue;
        }
        let project = package("");
        let run = project.cuno(&["build", "--release", "--compiler", name]);
        assert_eq!(run.code, Some(0), "{name}: {}", run.stderr);
        let commands = compile_commands(&project);
        let mut files: Vec<&str> = commands
            .iter()
            .map(|(file, _)| file.rsplit(['/', '\\']).next().unwrap())
            .collect();
        files.sort();
        assert_eq!(files, ["main.cpp", "tool.c", "util.cpp"], "{name}");
        let text = std::fs::read_to_string(project.path().join("compile_commands.json")).unwrap();
        let entries: serde_json::Value = serde_json::from_str(&text).unwrap();
        let test_objects = project
            .path()
            .join("target")
            .join("debug")
            .join("test")
            .join("app");
        for entry in entries.as_array().unwrap() {
            let output = std::path::Path::new(entry["output"].as_str().unwrap());
            assert!(output.starts_with(&test_objects), "{name}: {output:?}");
        }
        for (file, args) in &commands {
            assert!(
                args.iter().any(|a| a.ends_with("DCUNO_TEST")),
                "{name} {file}: {args:?}"
            );
            assert!(
                !args.iter().any(|a| a.contains("NDEBUG")),
                "{name} {file}: {args:?}"
            );
        }

        let broken = Project::new()
            .file("cuno.toml", MANIFEST)
            .file("src/main.cpp", "int main() { return undeclared; }\n");
        let run = broken.cuno(&["build", "--compiler", name]);
        assert_eq!(run.code, Some(1), "{name}: {}", run.stderr);
        assert_eq!(compile_commands(&broken).len(), 1, "{name}");
    }
}

/// A C++23 program built with errors for warnings, which stops at an
/// `#error` unless it is analysed as C++23 against MSVC 19.50 or newer,
/// with the standard library and the Windows headers.
const ANALYSED_AS_BUILT_CXX: &str = r#"#include <vector>
#include <windows.h>
#if __cplusplus < 202302L
#error analysed below C++23
#endif
#if _MSC_VER < 1950
#error analysed as an older MSVC
#endif
int main() { std::vector<int> v; return (int)v.size(); }
"#;

/// A C23 program that needs clang's own `stddef.h` and `limits.h`, not
/// the Windows SDK's or MSVC's of the same name.
const ANALYSED_AS_BUILT_C: &str = r#"#include <stddef.h>
#include <limits.h>
int main(void) {
    max_align_t m;
    (void)m;
    return LLONG_WIDTH == 64 ? 0 : 1;
}
"#;

/// On every compiler, clang-tidy analyses through `compile_commands.json`
/// alone, with no environment but `SystemRoot` (which Windows programs
/// need to start), the program the build compiled: C++23 against the same
/// MSVC, with the same headers in the same order, and errors for warnings
/// without failing on an argument it does not use. The exit status alone
/// is not enough: a missing header is reported as a diagnostic.
///
/// Mutation: leave out the include directories or the MSVC version, write
/// `INCLUDE` with `-isystem`, leave cl's `/std:c++23`, or write `/c`; this
/// test fails.
#[test]
fn compile_commands_analyse_the_program_that_was_built() {
    let Some(tidy) = support::compilers::require_llvm_tool("clang-tidy") else {
        return;
    };
    for (name, compiler) in COMPILERS {
        if !here(compiler) {
            continue;
        }
        for (manifest, source, text) in [
            (
                format!("{MANIFEST}edition = \"c++23\"\n[build]\nwarnings-as-errors = true\n"),
                "src/main.cpp",
                ANALYSED_AS_BUILT_CXX,
            ),
            (
                format!("{MANIFEST}lang = \"c\"\nedition = \"c23\"\n"),
                "src/main.c",
                ANALYSED_AS_BUILT_C,
            ),
        ] {
            // MSVC's own C library has neither name; the C case is for
            // the compilers that bring clang's headers.
            if compiler == Compiler::Msvc && source.ends_with(".c") {
                continue;
            }
            let project = Project::new()
                .file("cuno.toml", &manifest)
                .file(source, text);
            let run = project.cuno(&["build", "--compiler", name]);
            assert_eq!(run.code, Some(0), "{name} {source}: {}", run.stderr);
            // One check, so clang-tidy parses each source and reports
            // little but the compiler's own diagnostics.
            let mut cmd = std::process::Command::new(&tidy);
            cmd.args([
                "-p",
                ".",
                source,
                "--checks=-*,readability-braces-around-statements",
            ])
            .env_clear()
            .current_dir(project.path());
            if let Some(root) = std::env::var_os("SystemRoot") {
                cmd.env("SystemRoot", root);
            }
            let out = cmd.output().unwrap();
            let shown = String::from_utf8_lossy(&out.stdout).into_owned()
                + &String::from_utf8_lossy(&out.stderr);
            assert!(out.status.success(), "{name} {source}: {shown}");
            assert!(!shown.contains("error:"), "{name} {source}: {shown}");
        }
    }
}

/// `cuno check` and `cuno run` write the file as `cuno build` does, from
/// the `dev` graph, with each object where the `dev` test build puts it;
/// one that cannot be written is a warning and the build goes on.
///
/// Mutation: write it only for `cuno build`, lay the entries out for the
/// built profile, or drop the warning; this test fails.
#[test]
fn compile_commands_are_written_by_every_build_command() {
    let Some((name, _)) = COMPILERS.into_iter().find(|&(_, c)| here(c)) else {
        return;
    };
    for command in ["check", "run"] {
        let project = Project::new()
            .file("cuno.toml", MANIFEST)
            .file("src/main.cpp", "int main() {}\n");
        let run = project.cuno(&[command, "--release", "--compiler", name]);
        assert_eq!(run.code, Some(0), "{command}: {}", run.stderr);
        let text = std::fs::read_to_string(project.path().join("compile_commands.json"))
            .unwrap_or_else(|e| panic!("{command}: {e}"));
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        let output = value[0]["output"].as_str().unwrap().replace('\\', "/");
        assert!(
            output.contains("/target/debug/test/"),
            "{command}: {output}"
        );
    }

    let project = Project::new()
        .file("cuno.toml", MANIFEST)
        .file("src/main.cpp", "int main() {}\n")
        .file("compile_commands.json/keep", "");
    let run = project.cuno(&["build", "--compiler", name]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert!(
        run.stderr
            .starts_with("warning: could not write compile_commands.json: "),
        "{}",
        run.stderr
    );
}

/// A header the standard library probe includes first that stops when the
/// probe's flags are not the dev build's: release flags (`NDEBUG`) or the
/// test build's (`CUNO_TEST`). The program itself includes nothing.
const PROBE_FLAGS: &str = "#ifdef NDEBUG\n#error probed with release flags\n#endif\n#ifdef CUNO_TEST\n#error probed with test flags\n#endif\n#include_next <yvals_core.h>\n";

/// The build probes the standard library with the flags of the profile
/// it builds, and never with the test build's: a dev build passes the
/// header above, a release build stops at its `NDEBUG` line, exit 3.
///
/// Mutation: probe with the dev profile whatever is built, or with
/// `CUNO_TEST` defined; this test fails.
#[test]
fn the_probe_uses_the_built_profiles_flags() {
    if !here(Compiler::ClangCl) {
        return;
    }
    let project = Project::new()
        .file("cuno.toml", MANIFEST)
        .file("src/yvals_core.h", PROBE_FLAGS)
        .file("src/main.cpp", "int main() {}\n");
    let run = project.cuno(&["build", "--compiler", "clang-cl"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    let run = project.cuno(&["build", "--release", "--compiler", "clang-cl"]);
    assert_eq!(run.code, Some(3), "{}", run.stderr);
    assert!(
        run.stderr.contains("probed with release flags"),
        "{}",
        run.stderr
    );
}

/// The build selects the copies `[toolchain.paths]` names: one that is
/// not there is an error naming the setting, exit 3, not another copy.
///
/// Mutation: detect without the settings' paths, or select without the
/// configured Visual Studio; this test fails.
#[test]
fn the_build_uses_the_configured_toolchain_paths() {
    if !here(Compiler::Clang) {
        return;
    }
    for (setting, compiler, says) in [
        (
            "clang = \"C:/nope/clang.exe\"",
            "clang",
            "error: [toolchain.paths] clang is C:/nope/clang.exe, but there is no file there\n",
        ),
        (
            "msvc = \"C:/nope/VS\"",
            "msvc",
            "error: [toolchain.paths] msvc is C:/nope/VS, but no Visual Studio with the C++ x64 tools is installed there\n",
        ),
    ] {
        let project = Project::new()
            .file("cuno.toml", MANIFEST)
            .file(
                ".cuno/config.toml",
                &format!("[toolchain.paths]\n{setting}\n"),
            )
            .file("src/main.cpp", "int main() {}\n");
        let run = project.cuno(&["build", "--compiler", compiler]);
        assert_eq!(run.code, Some(3), "{compiler}: {}", run.stderr);
        assert!(run.stderr.starts_with(says), "{compiler}: {}", run.stderr);
    }
}

/// When cuno colors its output, Clang's diagnostics are colored too, and
/// with color off nothing is (docs/en/specs/commands.md, "Format").
///
/// Mutation: pass Clang no color flags, or only `-fcolor-diagnostics`,
/// which writes no escape codes through a pipe on Windows, put them after
/// the arguments, or leave them out of what `-v` shows; this test fails.
#[test]
fn clangs_diagnostics_are_colored_when_cunos_output_is() {
    for (name, compiler) in [("clang", Compiler::Clang), ("clang-cl", Compiler::ClangCl)] {
        if !here(compiler) {
            continue;
        }
        let project = Project::new()
            .file("cuno.toml", MANIFEST)
            .file("src/main.cpp", "int main() { return undeclared; }\n");
        let colored = project.cuno(&["--color", "always", "build", "--compiler", name]);
        let diagnostic = colored
            .stderr
            .lines()
            .find(|line| line.contains("undeclared"))
            .unwrap_or_default();
        assert!(diagnostic.contains('\x1b'), "{name}: {}", colored.stderr);
        let plain = project.cuno(&["--color", "never", "build", "--compiler", name]);
        assert!(!plain.stderr.contains('\x1b'), "{name}: {}", plain.stderr);

        // `-v` shows the flags as run, first.
        let shown = project.cuno(&["-v", "--color", "always", "build", "--compiler", name]);
        let running = shown
            .stderr
            .lines()
            .find(|line| line.contains("Running") && line.contains("main.cpp"))
            .unwrap_or_default();
        assert!(
            running.contains(" -fcolor-diagnostics -fansi-escape-codes "),
            "{name}: {running}"
        );

        // A user's own `-fno-color-diagnostics` comes later and wins.
        let opted_out = Project::new()
            .file(
                "cuno.toml",
                &format!("{MANIFEST}[build.{name}]\nflags = [\"-fno-color-diagnostics\"]\n"),
            )
            .file("src/main.cpp", "int main() { return undeclared; }\n")
            .cuno(&["--color", "always", "build", "--compiler", name]);
        let diagnostic = opted_out
            .stderr
            .lines()
            .find(|line| line.contains("undeclared"))
            .unwrap_or_default();
        assert!(!diagnostic.contains('\x1b'), "{name}: {}", opted_out.stderr);
    }
}

/// A build whose temporary directory does not exist stops before compiling
/// with a setup problem naming the directory and the check, as doctor
/// reports it, and does not create the directory (`toolchain.md`, "Minimum
/// compiler versions").
///
/// Mutation: hand the build another temporary directory than the process's
/// in `src/main.rs`, or create the missing one with its parents; this test
/// fails.
#[test]
fn a_missing_temporary_directory_stops_the_build() {
    if !here(Compiler::Clang) {
        return;
    }
    let project = Project::new()
        .file("cuno.toml", MANIFEST)
        .file("src/main.cpp", "int main() { return 0; }\n");
    let missing = project.home().join("missing");
    let temp = missing.join("temp");
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_cuno"));
    cmd.args(["build", "--compiler", "clang"])
        .env("TEMP", &temp)
        .env("TMP", &temp);
    let run = project.run_command(cmd);
    assert_eq!(run.code, Some(3), "{}", run.stderr);
    let error = format!("error: cannot create {}\\cuno-build-", temp.display());
    assert!(run.stderr.contains(&error), "{}", run.stderr);
    assert!(run.stderr.contains(" to check clang: "), "{}", run.stderr);
    assert!(
        run.stderr
            .contains("help: set TEMP to a directory that exists and can be written\n"),
        "{}",
        run.stderr
    );
    assert!(!missing.exists(), "the temporary directory was created");
    assert!(!project.path().join("target/debug/app.exe").exists());
}

/// compile_commands.json's clang command names the headers itself: run in
/// an environment whose `INCLUDE` holds a `<vector>` that is an `#error`,
/// it still reads the selected toolset's. clang searches the environment's
/// headers before `-idirafter`, so without `-nostdlibinc` it would read the
/// decoy (`build.md`, "Output").
///
/// Mutation: leave `-nostdlibinc` out of clang's form; this test fails.
#[test]
fn compile_commands_for_clang_ignore_the_environments_headers() {
    if !here(Compiler::Clang) {
        return;
    }
    let project = Project::new()
        .file("cuno.toml", MANIFEST)
        .file(
            "src/main.cpp",
            "#include <vector>\nint main() { return std::vector<int>{1}.size() == 1 ? 0 : 1; }\n",
        )
        .file(
            "decoy/vector",
            "#error the environment's headers were read\n",
        );
    let run = project.cuno(&["build", "--compiler", "clang"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    let text = std::fs::read_to_string(project.path().join("compile_commands.json")).unwrap();
    let entries: serde_json::Value = serde_json::from_str(&text).unwrap();
    let arguments: Vec<&str> = entries[0]["arguments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap())
        .collect();
    // The command as written, analysing only, in the decoy environment.
    let mut cmd = std::process::Command::new(arguments[0]);
    cmd.args(
        arguments[1..]
            .iter()
            .map(|&a| if a == "-c" { "-fsyntax-only" } else { a }),
    )
    .env_clear()
    .env("INCLUDE", project.path().join("decoy"))
    .current_dir(project.path());
    if let Some(root) = std::env::var_os("SystemRoot") {
        cmd.env("SystemRoot", root);
    }
    let out = cmd.output().unwrap();
    let shown =
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{arguments:?}\n{shown}");
}

/// `-q` wins over `[term] verbose = true`: no step's command and no status
/// line is shown (`commands.md`, "Available for all commands").
///
/// Mutation: let `[term] verbose` override `-q`; this test fails.
#[test]
fn quiet_wins_over_configured_verbose() {
    if !here(Compiler::Msvc) {
        return;
    }
    let project = package("").file(".cuno/config.toml", "[term]\nverbose = true\n");
    let run = project.cuno(&["-q", "build", "--compiler", "msvc"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    for line in ["Running", "Compiling", "Finished"] {
        assert!(!run.stderr.contains(line), "{line} in:\n{}", run.stderr);
    }
}

/// The compiler's warning lines in what a build printed.
fn warnings(run: &Run) -> Vec<String> {
    run.stderr
        .lines()
        .filter(|l| l.contains("warning") && l.contains("unused"))
        .map(str::to_owned)
        .collect()
}

/// A second build with nothing changed shows the warnings the first build
/// showed, as the compiler printed them, on every compiler; once the source
/// no longer warns, nothing is shown again (`commands.md`, "Format").
///
/// Mutation: show nothing for an up-to-date step, keep cl's file-name line
/// in what is shown again, or keep the old output when a rebuild prints
/// nothing; this test fails.
#[test]
fn an_up_to_date_step_shows_its_last_warnings_again() {
    for (name, compiler) in COMPILERS {
        if !here(compiler) {
            continue;
        }
        let project = Project::new()
            .file("cuno.toml", MANIFEST)
            .file("src/main.cpp", "int main() { int unused = 0; return 0; }\n");
        let first = project.cuno(&["build", "--compiler", name]);
        assert_eq!(first.code, Some(0), "{name}: {}", first.stderr);
        assert!(!warnings(&first).is_empty(), "{name}: {}", first.stderr);
        let again = project.cuno(&["build", "--compiler", name]);
        assert!(
            !again.stderr.contains("Compiling"),
            "{name}: {}",
            again.stderr
        );
        assert_eq!(warnings(&again), warnings(&first), "{name}");
        assert!(
            !again.stderr.lines().any(|l| l.trim() == "main.cpp"),
            "{name}: {}",
            again.stderr
        );

        std::fs::write(
            project.path().join("src/main.cpp"),
            "int main() { return 0; }\n",
        )
        .unwrap();
        // Dated back, so the rebuild is recorded and the next build skips it.
        std::fs::File::options()
            .write(true)
            .open(project.path().join("src/main.cpp"))
            .unwrap()
            .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600))
            .unwrap();
        let fixed = project.cuno(&["build", "--compiler", name]);
        assert!(warnings(&fixed).is_empty(), "{name}: {}", fixed.stderr);
        let after = project.cuno(&["build", "--compiler", name]);
        assert!(
            !after.stderr.contains("Compiling"),
            "{name}: {}",
            after.stderr
        );
        assert!(warnings(&after).is_empty(), "{name}: {}", after.stderr);
    }
}

/// A warning saved by a colored build is shown again without color when
/// cuno does not color, and where a step that ran would be: after the
/// `Compiling` line when another step runs (`commands.md`, "Format").
///
/// Mutation: replay the saved bytes as they are, or show them as soon as
/// the step is found up to date; this test fails.
#[test]
fn a_replayed_warning_follows_the_color_and_the_status_line() {
    if !here(Compiler::Clang) {
        return;
    }
    let project = Project::new()
        .file("cuno.toml", MANIFEST)
        .file(
            "src/main.cpp",
            "int helper();\nint main() { return helper(); }\n",
        )
        .file(
            "src/util.cpp",
            "int helper() { int unused = 0; return 0; }\n",
        );
    let colored = project.cuno(&["--color", "always", "build", "--compiler", "clang"]);
    assert!(colored.stderr.contains('\x1b'), "{}", colored.stderr);

    let plain = project.cuno(&["--color", "never", "build", "--compiler", "clang"]);
    assert!(!warnings(&plain).is_empty(), "{}", plain.stderr);
    assert!(!plain.stderr.contains('\x1b'), "{:?}", plain.stderr);

    std::fs::write(
        project.path().join("src/main.cpp"),
        "int helper();\nint main() { return helper() + 1; }\n",
    )
    .unwrap();
    // One step at a time, in plan order: the up-to-date body is judged
    // before the entry runs, so a replay printed at once would come first.
    let partial = project.cuno(&[
        "--color",
        "never",
        "build",
        "--compiler",
        "clang",
        "-j",
        "1",
    ]);
    let compiling = partial.stderr.find("Compiling").expect(&partial.stderr);
    let warning = partial.stderr.find("unused").expect(&partial.stderr);
    assert!(compiling < warning, "{}", partial.stderr);
}

/// On MSVC, `cuno check` fails where `cuno build` fails on a diagnostic cl
/// raises only while generating code: C4716 by default, C4715 under
/// `warnings-as-errors` (`build.md`, "Always-on flags").
///
/// Mutation: let MSVC check with `/Zs`; this test fails.
#[test]
fn an_msvc_check_fails_where_the_build_does() {
    if !here(Compiler::Msvc) {
        return;
    }
    for (extra, source) in [
        ("", "int f() { }\nint main() { return f(); }\n"),
        (
            "[build]\nwarnings-as-errors = true\n",
            "int f(int x) { if (x) return 1; }\nint main() { return f(0); }\n",
        ),
    ] {
        let project = Project::new()
            .file("cuno.toml", &format!("{MANIFEST}{extra}"))
            .file("src/main.cpp", source);
        for command in ["check", "build"] {
            let run = project.cuno(&[command, "--compiler", "msvc"]);
            assert_eq!(run.code, Some(1), "{command} {extra:?}: {}", run.stderr);
            assert!(
                run.stderr.ends_with("error: could not compile `app`\n"),
                "{command} {extra:?}: {}",
                run.stderr
            );
        }
    }
}

/// On MSVC, a build after a successful check runs only the link: the check
/// compiled with the build's own commands and objects.
///
/// Mutation: let the check compile with another command or object path
/// than the build; this test fails.
#[test]
fn an_msvc_build_after_a_check_only_links() {
    if !here(Compiler::Msvc) {
        return;
    }
    let project = package("");
    let check = project.cuno(&["check", "--compiler", "msvc"]);
    assert_eq!(check.code, Some(0), "{}", check.stderr);
    let build = project.cuno(&["-v", "build", "--compiler", "msvc"]);
    assert_eq!(build.code, Some(0), "{}", build.stderr);
    let running: Vec<&str> = build
        .stderr
        .lines()
        .filter(|l| l.contains("Running `"))
        .collect();
    assert!(!running.is_empty(), "{}", build.stderr);
    assert!(
        running.iter().all(|l| !l.contains(" /c ")),
        "{}",
        build.stderr
    );
}

/// A package whose body, entry and integration test are all valid, with
/// `broken` replaced by a line that does not compile: `entry`, `test`, or
/// `body under CUNO_TEST`.
fn check_targets_package(broken: &str) -> Project {
    let bad = "int bad = undeclared;\n";
    let body = if broken == "body under CUNO_TEST" {
        format!("int helper() {{ return 0; }}\n#ifdef CUNO_TEST\n{bad}#endif\n")
    } else {
        "int helper() { return 0; }\n".to_owned()
    };
    let entry = if broken == "entry" { bad } else { "" };
    let test = if broken == "test" { bad } else { "" };
    Project::new()
        .file("cuno.toml", MANIFEST)
        .file("src/util.cpp", &body)
        .file(
            "src/main.cpp",
            &format!("{entry}int helper();\nint main() {{ return helper(); }}\n"),
        )
        .file(
            "tests/t.cpp",
            &format!("{test}#include <cuno/test.hpp>\nstatic cuno::test::suite tests = [] {{\n    cuno::test::test(\"passes\", [] {{}});\n}};\n"),
        )
}

/// `cuno check` checks the program, `--tests` the test build (`tests/` and
/// the body with `CUNO_TEST`), and `--all-targets` both, as `cargo check`
/// chooses its targets (`commands.md`, "`cuno build` / `cuno check`").
///
/// Mutation: ignore `--tests` or `--all-targets`, check the tests by
/// default, check the program under `--tests`, or check the test build
/// without `CUNO_TEST`; this test fails.
#[test]
fn check_chooses_its_targets_as_cargo_check_does() {
    for (name, compiler) in COMPILERS {
        if !here(compiler) {
            continue;
        }
        for (broken, default, tests, all) in [
            ("test", 0, 1, 1),
            ("body under CUNO_TEST", 0, 1, 1),
            ("entry", 1, 0, 1),
        ] {
            let project = check_targets_package(broken);
            for (flags, want) in [
                (&[][..], default),
                (&["--tests"][..], tests),
                (&["--all-targets"][..], all),
            ] {
                let args = [&["check", "--compiler", name][..], flags].concat();
                let run = project.cuno(&args);
                assert_eq!(
                    run.code,
                    Some(want),
                    "{name}, {broken}, {flags:?}: {}",
                    run.stderr
                );
            }
        }
    }
}

/// On MSVC, `cuno check --tests` compiles as `cuno test` does, so the test
/// run that follows compiles nothing.
///
/// Mutation: let the test check compile with another command or object
/// than `cuno test`; this test fails.
#[test]
fn an_msvc_test_run_after_a_test_check_compiles_nothing() {
    if !here(Compiler::Msvc) {
        return;
    }
    let project = check_targets_package("none");
    // A first run installs the bundled headers in the fresh cuno home, just
    // now, inside the margin a step is not recorded in; dated back, they are
    // as a user's long-installed headers are.
    let first = project.cuno(&["check", "--compiler", "msvc"]);
    assert_eq!(first.code, Some(0), "{}", first.stderr);
    let mut dirs = vec![project.home().to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                dirs.push(path);
            } else {
                std::fs::File::options()
                    .write(true)
                    .open(&path)
                    .unwrap()
                    .set_modified(
                        std::time::SystemTime::now() - std::time::Duration::from_secs(3600),
                    )
                    .unwrap();
            }
        }
    }
    let check = project.cuno(&["check", "--tests", "--compiler", "msvc"]);
    assert_eq!(check.code, Some(0), "{}", check.stderr);
    let test = project.cuno(&["-v", "test", "--compiler", "msvc"]);
    assert_eq!(test.code, Some(0), "{}", test.stderr);
    assert!(
        test.stderr
            .lines()
            .filter(|l| l.contains("Running `"))
            .all(|l| !l.contains(" /c ")),
        "{}",
        test.stderr
    );
}
