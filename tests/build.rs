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
/// `Finished`, writes no object and no executable, and a second check with
/// nothing changed checks nothing.
///
/// Mutation: link in check mode, write objects, report success on an
/// error, or check everything again; this test fails.
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
                    assert!(
                        !matches!(ext, Some("obj" | "exe")),
                        "{name}: {}",
                        path.display()
                    );
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
