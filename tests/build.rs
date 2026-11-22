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
