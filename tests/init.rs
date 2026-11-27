//! `cuno init`: what it writes for each kind of directory, what it refuses,
//! and the `.gitignore` it leaves. See docs/en/specs/commands.md, "`cuno
//! init [path]`".
#![cfg(windows)]

mod support;

use std::path::Path;

use support::Project;
use support::compilers::{Compiler, Language, require};

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The files under `dir`, by their path from it with `/`, sorted.
fn files(dir: &Path) -> Vec<String> {
    fn walk(dir: &Path, from: &str, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let relative = if from.is_empty() {
                name
            } else {
                format!("{from}/{name}")
            };
            if entry.file_type().unwrap().is_dir() {
                walk(&entry.path(), &relative, out);
            } else {
                out.push(relative);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, "", &mut out);
    out.sort();
    out
}

/// A directory holding a C program in cuno's layout gets a `cuno.toml`
/// that says C, and nothing else of the sample; it then builds and runs.
///
/// Mutation: decide C++ whatever the sources, or write the sample too;
/// this test fails.
#[test]
fn c_sources_in_place_get_a_c_manifest_that_builds() {
    if require(Compiler::Msvc, Language::C, "x.c", "x.exe").is_none() {
        return;
    }
    let p = Project::new()
        .file(
            "src/main.c",
            "int add(int, int);\nint main(void) { return add(1, 2) == 3 ? 0 : 1; }\n",
        )
        .file("src/add.c", "int add(int a, int b) { return a + b; }\n");
    let run = p.cuno(&["init", "--vcs", "none"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    let manifest = read(&p.path().join("cuno.toml"));
    assert!(manifest.contains("lang = \"c\"\n"), "{manifest}");
    assert_eq!(files(p.path()), ["cuno.toml", "src/add.c", "src/main.c"]);
    let run = p.cuno(&["run", "--compiler", "msvc"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
}

/// Programs in `src/bin/` alone are entry files too: the package is made
/// and builds.
///
/// Mutation: require `src/main.*`; this test fails.
#[test]
fn programs_in_src_bin_are_enough() {
    if require(Compiler::Msvc, Language::Cxx, "x.cpp", "x.exe").is_none() {
        return;
    }
    let p = Project::new().file("src/bin/tool.cpp", "int main() { return 0; }\n");
    let run = p.cuno(&["init", "--vcs", "none", "--name", "tools"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert!(read(&p.path().join("cuno.toml")).contains("lang = \"c++\"\n"));
    let run = p.cuno(&["build", "--compiler", "msvc"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
}

/// A directory with no sources gets `cuno new`'s sample, and runs.
///
/// Mutation: write only `cuno.toml` there; this test fails.
#[test]
fn an_empty_directory_gets_the_sample() {
    let p = Project::new();
    let run = p.cuno(&["init", "--vcs", "none", "--name", "app"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert!(
        run.stderr.contains("Created `app` package"),
        "{}",
        run.stderr
    );
    assert_eq!(
        files(p.path()),
        ["cuno.toml", "src/calc.cpp", "src/calc.h", "src/main.cpp"]
    );
}

/// Every refusal is exit code 2 with the spec's message and help, and
/// writes nothing: sources outside `src/`, sources in `src/` with no entry
/// file, a file it would write, a package already, and no directory.
///
/// Mutation: write `cuno.toml` before refusing, write the sample's other
/// files before finding one there, or change a message; this test fails.
#[test]
fn every_refusal_writes_nothing() {
    // The files the directory holds, the error, and its help.
    type Case<'a> = (&'a [(&'a str, &'a str)], &'a str, &'a str);
    let cases: [Case; 4] = [
        (
            &[("main.cpp", ""), ("lib/util.cpp", "")],
            "error: the sources are not where cuno looks for them: lib/util.cpp, main.cpp\n",
            "help: move them under src/, with the program's main in src/main.cpp (or src/main.c)\n",
        ),
        (
            &[("src/a.cpp", "")],
            "error: src/ has no entry file\n",
            "help: put the program's main in src/main.cpp (or src/main.c), or each program in src/bin/\n",
        ),
        (
            &[("src/calc.h", "// mine\n")],
            "error: src/calc.h already exists\n",
            "help: cuno init writes no file that is there; move it aside, or add cuno.toml by hand\n",
        ),
        (
            &[("cuno.toml", "# mine\n")],
            "error: `.` already has a cuno.toml\n",
            "",
        ),
    ];
    for (held, error, help) in cases {
        let mut p = Project::new();
        for (path, text) in held {
            p = p.file(path, text);
        }
        let before = files(p.path());
        let run = p.cuno(&["init", "--vcs", "none", "--name", "app"]);
        assert_eq!(run.code, Some(2), "{error}{}", run.stderr);
        assert_eq!(run.stderr, format!("{error}{help}"), "{error}");
        assert_eq!(files(p.path()), before, "{error}");
    }
    let p = Project::new();
    let run = p.cuno(&["init", "nowhere"]);
    assert_eq!(run.code, Some(2), "{}", run.stderr);
    assert_eq!(
        run.stderr,
        "error: `nowhere` does not exist\nhelp: use cuno new to create a package in a new directory\n"
    );
}

/// An existing `.gitignore` keeps its lines, CRLF endings and all, and
/// gains only cuno's lines it lacked, in its own line ending.
///
/// Mutation: write the template over it, add every line, or add them with
/// LF; this test fails.
#[test]
fn an_existing_gitignore_gains_only_the_missing_lines() {
    let p = Project::new()
        .file(".gitignore", "build/\r\ntarget/")
        .file("src/main.cpp", "int main() { return 0; }\n");
    std::fs::create_dir(p.path().join(".git")).unwrap();
    let run = p.cuno(&["init", "--name", "app"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(
        read(&p.path().join(".gitignore")),
        "build/\r\ntarget/\r\n.cuno/\r\ncompile_commands.json\r\n.clang-format\r\n.clang-tidy\r\n"
    );
}
