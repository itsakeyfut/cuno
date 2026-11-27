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
    for reserved in ["con", "Com1", "LPT9"] {
        p.cuno(&["new", "dir", "--name", reserved]).assert(
            2,
            "",
            &format!(
                "error: `{reserved}` is a reserved name on Windows\nhelp: choose another name\n"
            ),
        );
    }
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

/// Every edition `cuno new` takes gives a package that builds, passes its
/// tests and runs on every compiler, and with no warning even at the
/// strictest settings a user may add (pedantic, warnings as errors, and for
/// C++ no exceptions or RTTI): the sample is code cuno wrote into their
/// package. Each sample uses the test entry its standard selects
/// (`testing.md`, "Recommended split and the `cuno new` sample").
///
/// Mutation: give C++17 or C++23 the other entry, put a warning in a
/// sample, put the tests in the entry file, map an edition to another, or
/// have `main` print anything else; this test fails.
#[test]
fn every_generated_package_builds_tests_and_runs() {
    for (edition, test_line) in [
        ("c++20", "test add adds ... ok"),
        ("c++23", "test add adds ... ok"),
        ("c++17", "test add_adds ... ok"),
        ("c11", "test add_adds ... ok"),
        ("c17", "test add_adds ... ok"),
        ("c23", "test add_adds ... ok"),
    ] {
        for (name, compiler) in COMPILERS {
            if require(compiler, Language::C, "x.c", "x.exe").is_none() {
                continue;
            }
            let p = Project::new();
            let made = p.cuno(&["new", "app", "--edition", edition, "--vcs", "none"]);
            assert_eq!(made.code, Some(0), "{edition}: {}", made.stderr);
            let app = p.path().join("app");
            // Listed, not opened: the file system here opens either case.
            let names: Vec<String> = std::fs::read_dir(&app)
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            assert!(
                names.contains(&"Cuno.toml".to_owned()),
                "{edition}: {names:?}"
            );
            let manifest = std::fs::read_to_string(app.join("Cuno.toml")).unwrap();
            assert!(
                manifest.contains(&format!("edition = \"{edition}\"\n")),
                "{edition}: {manifest}"
            );
            let strict = if edition.starts_with("c++") {
                "\n[build]\nwarnings = \"pedantic\"\nwarnings-as-errors = true\nexceptions = false\nrtti = false\n"
            } else {
                "\n[build]\nwarnings = \"pedantic\"\nwarnings-as-errors = true\n"
            };
            std::fs::write(app.join("Cuno.toml"), format!("{manifest}{strict}")).unwrap();
            let test = cuno_in(&p, &app, &["-q", "test", "--compiler", name], &[]);
            assert_eq!(test.code, Some(0), "{edition} {name}:\n{}", test.stderr);
            let out = lf(&test.stderr);
            assert!(out.contains(test_line), "{edition} {name}:\n{out}");
            assert!(!out.contains("warning"), "{edition} {name}:\n{out}");
            let run = cuno_in(&p, &app, &["-q", "run", "--compiler", name], &[]);
            assert_eq!(
                (run.code, lf(&run.stdout), lf(&run.stderr)),
                (Some(0), "1 + 2 = 3\n".to_owned(), String::new()),
                "{edition} {name}"
            );
        }
    }
}

/// The package gets a repository and a `.gitignore` of cuno's outputs,
/// except inside a repository, where it gets the `.gitignore` alone; with
/// `--vcs none`, or no git, neither; with no git, a warning
/// (`commands.md`, "`cuno new <path>`").
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
        "# What cuno makes, which is not edited by hand: the build's output, its\n\
         # local settings, and the files it writes for editors, fmt and lint.\n\
         target/\n.cuno/\ncompile_commands.json\n.clang-format\n.clang-tidy\n"
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
    assert!(p.path().join("nogit/Cuno.toml").is_file());
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

/// `--name` names the package whatever the directory is called; the
/// package's directory is found with `..` resolved, so it is not taken to
/// be inside a repository it only passes through on the way.
///
/// Mutation: write the directory's name into `Cuno.toml`, or walk up the
/// path as written; this test fails.
#[test]
fn the_name_and_the_place_are_the_packages() {
    let p = Project::new();
    let named = p.cuno(&["new", "dir", "--name", "other", "--vcs", "none"]);
    assert_eq!(lf(&named.stderr), "     Created `other` package\n");
    let manifest = std::fs::read_to_string(p.path().join("dir/Cuno.toml")).unwrap();
    assert!(manifest.contains("name = \"other\"\n"), "{manifest}");

    std::fs::create_dir_all(p.path().join("repo/.git")).unwrap();
    std::fs::create_dir_all(p.path().join("repo/sub")).unwrap();
    std::fs::create_dir_all(p.path().join("elsewhere")).unwrap();
    let run = cuno_in(
        &p,
        &p.path().join("repo/sub"),
        &["new", "../../elsewhere/app"],
        &[],
    );
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert!(
        p.path().join("elsewhere/app/.git").is_dir(),
        "git init was skipped"
    );
}

/// When `git init` fails, a warning carries git's own message and no
/// `.gitignore` is written; a warning survives `-q`; a file that cannot be
/// written is exit code 3 naming it (`commands.md`, "`cuno new <path>`").
///
/// Mutation: drop the warning of a failed `git init`, write a
/// `.gitignore` anyway, hide warnings under `-q`, or change the exit code
/// of a write that fails; this test fails.
#[test]
fn failures_are_said_and_left_to_see() {
    let p = Project::new();
    std::fs::write(p.path().join("afile"), "").unwrap();
    let bad_git = p.path().join("afile").join("sub");
    let failed = cuno_in(
        &p,
        p.path(),
        &["new", "app"],
        &[("GIT_DIR", bad_git.to_str().unwrap())],
    );
    assert_eq!(failed.code, Some(0), "{}", failed.stderr);
    assert!(
        lf(&failed.stderr).starts_with("warning: git init failed: fatal: "),
        "{}",
        failed.stderr
    );
    assert!(!p.path().join("app/.gitignore").exists());

    let quiet = cuno_in(&p, p.path(), &["-q", "new", "quiet"], &[("PATH", "")]);
    assert_eq!(
        lf(&quiet.stderr),
        "warning: git was not found; the package is not under version control\n"
    );

    let blocked = p.cuno(&["new", "afile/sub"]);
    assert_eq!(blocked.code, Some(3), "{}", blocked.stderr);
    assert!(
        blocked.stderr.starts_with("error: cannot write "),
        "{}",
        blocked.stderr
    );
}

/// Writes the user's `<cuno home>/config.toml`.
fn user_config(p: &Project, contents: &str) {
    std::fs::write(p.home().join("config.toml"), contents).unwrap();
}

/// `cuno new` follows the user's `[term] color`, as every command does
/// (`commands.md`, "Available for all commands").
///
/// Mutation: dispatch `new` without reading the user's settings; this test
/// fails.
#[test]
fn the_users_term_color_applies() {
    let p = Project::new();
    user_config(&p, "[term]\ncolor = \"always\"\n");
    let run = p.cuno(&["new", "app", "--vcs", "none"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert!(run.stderr.contains("\u{1b}["), "{:?}", run.stderr);
}

/// `cuno new` reads no project: inside one whose `Cuno.toml` is broken it
/// still creates the package.
///
/// Mutation: resolve every layer for `new`, the surrounding project
/// included; this test fails.
#[test]
fn a_surrounding_project_is_not_read() {
    let p = Project::new().file("Cuno.toml", "[package\n");
    let run = p.cuno(&["new", "app", "--vcs", "none"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert!(p.path().join("app").join("Cuno.toml").is_file());
}

/// A user `config.toml` that cannot be read stops `cuno new` with exit 3,
/// before anything is created.
///
/// Mutation: let `new` go on when the user's settings fail; this test
/// fails.
#[test]
fn a_broken_user_config_stops_it() {
    let p = Project::new();
    user_config(&p, "[term\n");
    let run = p.cuno(&["new", "app", "--vcs", "none"]);
    assert_eq!(run.code, Some(3), "{}", run.stderr);
    assert!(run.stderr.contains("config.toml"), "{}", run.stderr);
    assert!(!p.path().join("app").exists());
}
