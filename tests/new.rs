//! `cuno new`: every error and exit code, the projects it generates
//! building, testing and running on every compiler here, and when it runs
//! `git init`. See docs/en/specs/commands.md, "`cuno new <path>`".

mod support;

use std::path::Path;
use std::process::Command;

use support::compilers::{Compiler, Language, require};
use support::{Project, Run};

const COMPILERS: [(&str, Compiler); 3] = [
    ("msvc", Compiler::Msvc),
    ("clang", Compiler::Clang),
    ("clang-cl", Compiler::ClangCl),
];

/// Runs cuno with `args` in `dir`, with the project's hermetic environment
/// and `vars`.
fn cuno_in(p: &Project, dir: &Path, args: &[&str], vars: &[(&str, &str)]) -> Run {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_cuno"));
    cmd.args(args).current_dir(dir);
    p.run_command_with(cmd, vars)
}

fn lf(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// Every refusal is exit code 2 with the spec's message, and makes
/// nothing (`commands.md`, "`cuno new <path>`").
///
/// Mutation: change any message, help or exit code, check the name only
/// when given, accept an existing directory, or a mismatched edition; this
/// test fails.
#[test]
fn every_refusal_says_what_to_do() {
    let p = Project::new();
    p.cuno(&["new", "app", "--name", "1app"]).assert(
        2,
        "",
        "error: `1app` is not a valid package name\n\
         help: a package name starts with a letter and has only letters, digits, - and _\n",
    );
    p.cuno(&["new", "my game"]).assert(
        2,
        "",
        "error: `my game` is not a valid package name\n\
         help: a package name starts with a letter and has only letters, digits, - and _\n\
         help: name the package with --name\n",
    );
    p.cuno(&["new", "app", "--lang", "c++", "--edition", "c17"])
        .assert(
            2,
            "",
            "error: `c17` is not a standard of C++\n\
         help: use --lang c, or an edition of C++: c++17, c++20, c++23\n",
        );
    assert!(!p.path().join("app").exists(), "a refusal made nothing");
    std::fs::create_dir(p.path().join("taken")).unwrap();
    p.cuno(&["new", "taken"])
        .assert(2, "", "error: `taken` already exists\n");
    let unknown = p.cuno(&["new", "app", "--lang", "rust"]);
    assert_eq!(unknown.code, Some(2), "{}", unknown.stderr);
}

/// A C++20, a C++17 and a C package, as generated, build, pass their tests
/// and run on every compiler; each sample uses the test entry its standard
/// selects (`testing.md`, "Recommended split and the `cuno new` sample").
///
/// Mutation: give C++17 the C++20 entry, put the tests in the entry file,
/// leave `src/calc` out, or have `main` print anything else; this test
/// fails.
#[test]
fn every_generated_package_builds_tests_and_runs() {
    for (args, test_line) in [
        (&["new", "app"][..], "test add adds ... ok"),
        (
            &["new", "app", "--edition", "c++17"][..],
            "test add_adds ... ok",
        ),
        (&["new", "app", "--lang", "c"][..], "test add_adds ... ok"),
    ] {
        for (name, compiler) in COMPILERS {
            if require(compiler, Language::C, "x.c", "x.exe").is_none() {
                continue;
            }
            let p = Project::new();
            let made = p.cuno(&[args, &["--vcs", "none"]].concat());
            assert_eq!(made.code, Some(0), "{args:?}: {}", made.stderr);
            let app = p.path().join("app");
            let test = cuno_in(&p, &app, &["-q", "test", "--compiler", name], &[]);
            assert_eq!(test.code, Some(0), "{args:?} {name}:\n{}", test.stderr);
            assert!(
                lf(&test.stderr).contains(test_line),
                "{args:?} {name}:\n{}",
                test.stderr
            );
            let run = cuno_in(&p, &app, &["-q", "run", "--compiler", name], &[]);
            assert_eq!(
                (run.code, lf(&run.stdout)),
                (Some(0), "1 + 2 = 3\n".to_owned()),
                "{args:?} {name}:\n{}",
                run.stderr
            );
        }
    }
}

/// The package gets a repository and a `.gitignore` of cuno's outputs,
/// except inside a repository, where it gets the `.gitignore` alone; with
/// `--vcs none`, or no git, neither; with no git, a warning
/// (`commands.md`, "`--vcs`").
///
/// Mutation: run `git init` inside a repository, skip it outside one,
/// write a `.gitignore` under `--vcs none` or with no git, change what it
/// ignores, or stay silent with no git; this test fails.
#[test]
fn git_is_initialised_where_it_should_be() {
    let p = Project::new();
    let made = p.cuno(&["new", "fresh"]);
    assert_eq!(made.code, Some(0), "{}", made.stderr);
    assert_eq!(lf(&made.stderr), "     Created `fresh` package\n");
    assert!(p.path().join("fresh/.git").is_dir());
    assert_eq!(
        lf(&std::fs::read_to_string(p.path().join("fresh/.gitignore")).unwrap()),
        "target/\n.cuno/\ncompile_commands.json\n.clang-format\n.clang-tidy\n"
    );

    std::fs::create_dir_all(p.path().join("repo/.git")).unwrap();
    let inside = p.cuno(&["new", "repo/app"]);
    assert_eq!(inside.code, Some(0), "{}", inside.stderr);
    assert!(!p.path().join("repo/app/.git").exists());
    assert!(p.path().join("repo/app/.gitignore").is_file());

    let none = p.cuno(&["new", "plain", "--vcs", "none"]);
    assert_eq!(none.code, Some(0), "{}", none.stderr);
    assert!(!p.path().join("plain/.git").exists());
    assert!(!p.path().join("plain/.gitignore").exists());

    let missing = cuno_in(&p, p.path(), &["new", "nogit"], &[("PATH", "")]);
    assert_eq!(missing.code, Some(0), "{}", missing.stderr);
    assert_eq!(
        lf(&missing.stderr),
        "warning: git was not found; the package is not under version control\n     Created `nogit` package\n"
    );
    assert!(!p.path().join("nogit/.git").exists());
    assert!(!p.path().join("nogit/.gitignore").exists());
    assert!(p.path().join("nogit/cuno.toml").is_file());
}

/// `-q` leaves out the `Created` line.
///
/// Mutation: print it under `-q`; this test fails.
#[test]
fn quiet_creates_silently() {
    let p = Project::new();
    p.cuno(&["-q", "new", "app", "--vcs", "none"])
        .assert(0, "", "");
}
