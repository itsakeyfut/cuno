//! `cuno init`: what it writes for each kind of directory, what it refuses,
//! and the `.gitignore` it leaves. See docs/en/specs/commands.md, "`cuno
//! init [path]`".
#![cfg(windows)]

mod support;

use std::path::Path;

use support::compilers::{Compiler, Language, require};
use support::{Project, Run};

const GITIGNORE: &str = "# What cuno makes, which is not edited by hand: the build's output, its\n\
                         # local settings, and the files it writes for editors, fmt and lint.\n\
                         target/\n.cuno/\ncompile_commands.json\n.clang-format\n.clang-tidy\n";

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The files under `dir`, by their path from it with `/`, sorted; `.git`
/// left out.
fn files(dir: &Path) -> Vec<String> {
    fn walk(dir: &Path, from: &str, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == ".git" {
                continue;
            }
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

/// `cuno init` with `args`, in `p`'s directory.
fn init(p: &Project, args: &[&str]) -> Run {
    let mut all = vec!["init"];
    all.extend_from_slice(args);
    p.cuno(&all)
}

/// A directory holding a C program in cuno's layout gets a `Cuno.toml`
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
    let run = init(&p, &["--vcs", "none", "--name", "calc"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    let manifest = read(&p.path().join("Cuno.toml"));
    assert!(manifest.contains("lang = \"c\"\n"), "{manifest}");
    assert_eq!(files(p.path()), ["Cuno.toml", "src/add.c", "src/main.c"]);
    let run = p.cuno(&["run", "--compiler", "msvc"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
}

/// The language: C++ when any source in `src/` is C++, even beside C;
/// `--lang` and `--edition` over what the sources say.
///
/// Mutation: call mixed sources C, or let the sources decide over
/// `--lang` or `--edition`; this test fails.
#[test]
fn the_language_comes_from_the_sources_unless_given() {
    let mixed = || {
        Project::new()
            // An entry in src/bin/, which is one whatever the language.
            .file("src/bin/app.cpp", "int main() { return 0; }\n")
            .file("src/more.c", "int more(void) { return 0; }\n")
    };
    for (args, lang, edition) in [
        (&[][..], "c++", "c++20"),
        (&["--lang", "c"][..], "c", "c17"),
        (&["--edition", "c11"][..], "c", "c11"),
    ] {
        let p = mixed();
        let mut all = vec!["--vcs", "none", "--name", "app"];
        all.extend_from_slice(args);
        let run = init(&p, &all);
        assert_eq!(run.code, Some(0), "{args:?}: {}", run.stderr);
        let manifest = read(&p.path().join("Cuno.toml"));
        assert!(
            manifest.contains(&format!("lang = \"{lang}\"\nedition = \"{edition}\"\n")),
            "{args:?}: {manifest}"
        );
    }
}

/// Programs in `src/bin/` alone are entry files too: the package is made
/// and builds. A capitalised `Src/` is `src/` on Windows, as for the build.
///
/// Mutation: require `src/main.*`, or tell `src/` apart by its spelling;
/// this test fails.
#[test]
fn programs_in_src_bin_or_a_capitalised_src_are_enough() {
    if require(Compiler::Msvc, Language::Cxx, "x.cpp", "x.exe").is_none() {
        return;
    }
    for entry in ["src/bin/tool.cpp", "Src/main.cpp"] {
        let p = Project::new().file(entry, "int main() { return 0; }\n");
        let run = init(&p, &["--vcs", "none", "--name", "tools"]);
        assert_eq!(run.code, Some(0), "{entry}: {}", run.stderr);
        assert!(read(&p.path().join("Cuno.toml")).contains("lang = \"c++\"\n"));
        let run = p.cuno(&["build", "--compiler", "msvc"]);
        assert_eq!(run.code, Some(0), "{entry}: {}", run.stderr);
    }
}

/// A directory with no sources gets `cuno new`'s sample; sources in
/// `target/`, in a hidden directory or in another package do not count.
///
/// Mutation: write only `Cuno.toml` there, or look in `target/`, in a
/// hidden directory, or in another package; this test fails.
#[test]
fn a_directory_with_no_sources_of_its_own_gets_the_sample() {
    let p = Project::new()
        .file("target/old.cpp", "")
        .file(".vscode/x.cpp", "")
        .file(
            "vendor/Cuno.toml",
            "[package]\nname = \"v\"\nversion = \"0.1.0\"\n",
        )
        .file("vendor/src/main.cpp", "");
    let run = init(&p, &["--vcs", "none", "--name", "app"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert!(
        run.stderr.contains("Created `app` package"),
        "{}",
        run.stderr
    );
    for sample in ["src/calc.cpp", "src/calc.h", "src/main.cpp"] {
        assert!(p.path().join(sample).is_file(), "{sample}");
    }
}

/// The path given is the directory made into a package, named after it.
///
/// Mutation: make the current directory the package, or name it otherwise;
/// this test fails.
#[test]
fn the_path_given_is_the_package_and_names_it() {
    let p = Project::new();
    std::fs::create_dir(p.path().join("calc")).unwrap();
    let run = init(&p, &["calc", "--vcs", "none"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert!(!p.path().join("Cuno.toml").exists());
    let manifest = read(&p.path().join("calc").join("Cuno.toml"));
    assert!(manifest.contains("name = \"calc\"\n"), "{manifest}");
    assert!(
        run.stderr.contains("Created `calc` package"),
        "{}",
        run.stderr
    );
}

/// Outside a repository, `git init` makes one and the `.gitignore` is
/// cuno's; inside one, there is no `git init`, and the `.gitignore` is
/// made all the same.
///
/// Mutation: run anything but `git init`, or take the directory as outside
/// the repository it is in; this test fails.
#[test]
fn git_is_initialised_only_outside_a_repository() {
    let p = Project::new();
    let run = init(&p, &["--name", "app"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert!(p.path().join(".git").is_dir());
    assert_eq!(read(&p.path().join(".gitignore")), GITIGNORE);

    let p = Project::new();
    std::fs::create_dir(p.path().join(".git")).unwrap();
    std::fs::create_dir(p.path().join("sub")).unwrap();
    let run = init(&p, &["sub", "--name", "app"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert!(!p.path().join("sub").join(".git").exists());
    assert_eq!(read(&p.path().join("sub").join(".gitignore")), GITIGNORE);
}

/// Every refusal is exit code 2 with the spec's message and help, and
/// writes nothing.
///
/// Mutation: write anything before refusing (`Cuno.toml` before the
/// `.gitignore` is found unusable, or the sample before a file there is
/// found), stop naming at 5 sources, or change a message; this test fails.
#[test]
fn every_refusal_writes_nothing() {
    // The files the directory holds, the arguments, the error, and its help.
    type Case<'a> = (&'a [(&'a str, &'a [u8])], &'a [&'a str], &'a str, &'a str);
    let cases: [Case; 10] = [
        (
            &[("main.cpp", b""), ("lib/util.cpp", b"")],
            &[],
            "error: sources found outside src/: lib/util.cpp, main.cpp\n",
            "help: move them under src/, with the program's main in src/main.cpp (or src/main.c)\n",
        ),
        (
            &[
                ("a1.c", b""),
                ("a2.c", b""),
                ("a3.c", b""),
                ("a4.c", b""),
                ("a5.c", b""),
                ("a6.c", b""),
                ("a7.c", b""),
            ],
            &[],
            "error: sources found outside src/: a1.c, a2.c, a3.c, a4.c, a5.c and 2 more\n",
            "help: move them under src/, with the program's main in src/main.cpp (or src/main.c)\n",
        ),
        (
            &[("src/a.cpp", b"")],
            &[],
            "error: src/ has no executable to build\n",
            "help: put the program's main in src/main.cpp (or src/main.c), or each program in src/bin/\n",
        ),
        (
            &[("src/calc.h", b"// mine\n")],
            &[],
            "error: src/calc.h already exists\n",
            "help: cuno init writes no file that is there; move it aside, or add Cuno.toml by hand\n",
        ),
        (
            &[("src", b"not a directory")],
            &[],
            "error: src already exists\n",
            "help: cuno init writes no file that is there; move it aside, or add Cuno.toml by hand\n",
        ),
        (
            &[("Cuno.toml", b"# mine\n")],
            &[],
            "error: `.` already has a Cuno.toml\n",
            "",
        ),
        (
            &[(".gitignore/x", b"")],
            &["--vcs", "git"],
            "error: cannot add to .gitignore: it is not a file\n",
            "help: make it a text file in UTF-8, or use --vcs none\n",
        ),
        (
            &[(".gitignore", b"\xff\xfet\x00")],
            &["--vcs", "git"],
            "error: cannot add to .gitignore: stream did not contain valid UTF-8\n",
            "help: make it a text file in UTF-8, or use --vcs none\n",
        ),
        (
            &[],
            &["--name", "1app"],
            "error: `1app` is not a valid package name\n",
            "help: a package name starts with a letter and has only letters, digits, - and _\n",
        ),
        (
            &[],
            &["--name", "con"],
            "error: `con` is a reserved name on Windows\n",
            "help: choose another name\n",
        ),
    ];
    for (held, args, error, help) in cases {
        let p = Project::new();
        for (path, bytes) in held {
            let full = p.path().join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(&full, bytes).unwrap();
        }
        let before = files(p.path());
        // No version control and the name `app`, unless the case says.
        let mut all = Vec::new();
        if !args.contains(&"--vcs") {
            all.extend(["--vcs", "none"]);
        }
        if !args.contains(&"--name") {
            all.extend(["--name", "app"]);
        }
        all.extend_from_slice(args);
        let run = init(&p, &all);
        assert_eq!(run.code, Some(2), "{error}{}", run.stderr);
        assert_eq!(run.stderr, format!("{error}{help}"), "{error}");
        assert_eq!(files(p.path()), before, "{error}");
        assert!(!p.path().join(".git").exists(), "{error}");
    }

    let p = Project::new().file("notes.txt", "");
    let run = init(&p, &["notes.txt"]);
    assert_eq!(run.code, Some(2), "{}", run.stderr);
    assert_eq!(run.stderr, "error: `notes.txt` is not a directory\n");
    let run = init(&p, &["nowhere"]);
    assert_eq!(run.code, Some(2), "{}", run.stderr);
    assert_eq!(
        run.stderr,
        "error: `nowhere` does not exist\nhelp: use cuno new to create a package in a new directory\n"
    );
    // A name from the directory says how to give one.
    std::fs::create_dir(p.path().join("my proj")).unwrap();
    let run = init(&p, &["my proj", "--vcs", "none"]);
    assert_eq!(run.code, Some(2), "{}", run.stderr);
    assert!(
        run.stderr.ends_with("help: name the package with --name\n"),
        "{}",
        run.stderr
    );
    assert!(!p.path().join("my proj").join("Cuno.toml").exists());
}

/// What the build itself refuses about `src/` is refused here, before
/// anything is written: two executables of one name.
///
/// Mutation: let the build's other refusals through; this test fails.
#[test]
fn the_builds_own_refusals_stop_it() {
    let p = Project::new()
        .file("src/main.cpp", "int main() { return 0; }\n")
        .file("src/bin/app.cpp", "int main() { return 0; }\n");
    let before = files(p.path());
    let run = init(&p, &["--vcs", "none", "--name", "app"]);
    assert_ne!(run.code, Some(0), "{}", run.stderr);
    assert!(run.stderr.starts_with("error: "), "{}", run.stderr);
    assert_eq!(files(p.path()), before);
}

/// An existing `.gitignore` keeps its lines, CRLF endings and all, and
/// gains only cuno's lines it lacked, in its own line ending; one that has
/// them all, spaces around them or not, is left as it is.
///
/// Mutation: write the template over it, add every line, add them with LF,
/// or compare lines with their spaces; this test fails.
#[test]
fn an_existing_gitignore_gains_only_the_missing_lines() {
    let p = Project::new()
        .file(".gitignore", "build/\r\ntarget/")
        .file("src/main.cpp", "int main() { return 0; }\n");
    std::fs::create_dir(p.path().join(".git")).unwrap();
    let run = init(&p, &["--name", "app"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(
        read(&p.path().join(".gitignore")),
        "build/\r\ntarget/\r\n.cuno/\r\ncompile_commands.json\r\n.clang-format\r\n.clang-tidy\r\n"
    );

    // No newline at its end, which nothing should add when nothing is missing.
    let complete = "target/  \n.cuno/\ncompile_commands.json\n.clang-format\n.clang-tidy";
    let p = Project::new()
        .file(".gitignore", complete)
        .file("src/main.cpp", "int main() { return 0; }\n");
    std::fs::create_dir(p.path().join(".git")).unwrap();
    let run = init(&p, &["--name", "app"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(read(&p.path().join(".gitignore")), complete);
}
