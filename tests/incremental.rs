//! A second `cuno build` runs only what changed, on the real compilers
//! (`docs/en/specs/build.md`, "Deciding what changed").
#![cfg(windows)]

mod support;

use std::path::Path;
use std::time::{Duration, SystemTime};

use support::compilers::{Compiler, Language, require};
use support::{Project, Run};

const COMPILERS: [(&str, Compiler); 3] = [
    ("msvc", Compiler::Msvc),
    ("clang", Compiler::Clang),
    ("clang-cl", Compiler::ClangCl),
];

fn here(compiler: Compiler) -> bool {
    require(compiler, Language::C, "x.c", "x.exe").is_some()
}

/// A package whose `main.cpp` reads `inner.h` through `outer.h`, whose
/// `one.cpp` reads it directly, and whose `two.cpp` reads neither. The
/// inner header sits in a directory with a space and a non-ASCII name.
fn package(extra: &str) -> Project {
    Project::new()
        .file(
            "cuno.toml",
            &format!("[package]\nname = \"app\"\nversion = \"0.1.0\"\n{extra}"),
        )
        .file("src/inc dir/内側.h", "#pragma once\n#define INNER 1\n")
        .file("src/outer.h", "#pragma once\n#include \"inc dir/内側.h\"\n")
        .file(
            "src/main.cpp",
            "#include <cstdio>\n#include \"outer.h\"\nint one();\nint two();\n\
             int main() { std::printf(\"%d %d %d\\n\", INNER, one(), two()); }\n",
        )
        .file(
            "src/one.cpp",
            "#include \"inc dir/内側.h\"\nint one() { return INNER; }\n",
        )
        .file("src/two.cpp", "int two() { return 2; }\n")
}

/// The sources the `-v` output shows compiled, sorted, and whether it
/// shows a link.
fn ran(run: &Run) -> (Vec<&'static str>, bool) {
    let mut compiled = Vec::new();
    let mut linked = false;
    for line in run.stderr.lines().filter(|l| l.contains("Running `")) {
        let compile = line.contains(" /c ") || line.contains(" -c ");
        if !compile {
            linked = true;
            continue;
        }
        for name in ["main", "one", "two"] {
            let file = format!("{name}.cpp");
            let source = line
                .split(' ')
                .any(|arg| arg.trim_matches('"').ends_with(&format!("\\src\\{file}")));
            if source {
                compiled.push(name);
            }
        }
    }
    compiled.sort();
    (compiled, linked)
}

fn build(project: &Project, compiler: &str) -> Run {
    let run = project.cuno(&["-v", "build", "--compiler", compiler]);
    assert_eq!(run.code, Some(0), "{compiler}: {}", run.stderr);
    run
}

fn output(project: &Project) -> String {
    let run = project.run(project.path().join("target/debug/app.exe"), &[]);
    run.stdout.replace("\r\n", "\n")
}

/// A second build with nothing changed runs nothing and prints only its
/// `Finished` line; editing a header read through another header
/// recompiles exactly the two sources that read it, and relinks.
///
/// Mutation: record only the headers a source includes directly, rebuild
/// everything, relink nothing, or print `Compiling` when nothing runs;
/// this test fails.
#[test]
fn only_what_a_change_reaches_is_rebuilt() {
    for (name, compiler) in COMPILERS {
        if !here(compiler) {
            continue;
        }
        let project = package("");
        assert_eq!(
            ran(&build(&project, name)),
            (vec!["main", "one", "two"], true)
        );

        let again = build(&project, name);
        assert_eq!(ran(&again), (vec![], false), "{name}");
        let lines: Vec<&str> = again.stderr.lines().collect();
        assert_eq!(lines.len(), 1, "{name}: {}", again.stderr);
        assert!(lines[0].starts_with("    Finished `dev` profile"), "{name}");

        std::fs::write(
            project.path().join("src/inc dir/内側.h"),
            "#pragma once\n#define INNER 7\n",
        )
        .unwrap();
        assert_eq!(
            ran(&build(&project, name)),
            (vec!["main", "one"], true),
            "{name}"
        );
        assert_eq!(output(&project), "7 7 2\n", "{name}");
    }
}

/// Changing `[build] defines` changes every compile's command line, so
/// every source is recompiled.
///
/// Mutation: ignore the command line; this test fails.
#[test]
fn a_changed_setting_recompiles_what_it_affects() {
    for (name, compiler) in COMPILERS {
        if !here(compiler) {
            continue;
        }
        let project = package("");
        build(&project, name);
        std::fs::write(
            project.path().join("cuno.toml"),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\n[build]\ndefines = [\"X=1\"]\n",
        )
        .unwrap();
        assert_eq!(
            ran(&build(&project, name)),
            (vec!["main", "one", "two"], true),
            "{name}"
        );
    }
}

/// A source put back with an older time, as a restore from a backup does,
/// is rebuilt: what is recorded is compared, not which file is newer.
///
/// Mutation: judge by "newer than the output"; this test fails.
#[test]
fn a_restored_older_file_is_rebuilt() {
    if !here(Compiler::Msvc) {
        return;
    }
    let project = package("");
    build(&project, "msvc");
    let two = project.path().join("src/two.cpp");
    std::fs::write(&two, "int two() { return 9; }\n").unwrap();
    set_time(&two, SystemTime::now() - Duration::from_secs(86_400 * 365));
    assert_eq!(ran(&build(&project, "msvc")), (vec!["two"], true));
    assert_eq!(output(&project), "1 1 9\n");
}

fn set_time(path: &Path, time: SystemTime) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(time)
        .unwrap();
}
