//! `cuno toolchain list` and `cuno toolchain default` run as a process,
//! against the compilers installed here (`docs/en/specs/commands.md`,
//! "`cuno toolchain list`" and "`cuno toolchain default <compiler>`").
#![cfg(windows)]

mod support;

use std::process::Command;

use support::compilers::{Compiler, Language, require};
use support::{Project, Run};

/// `cuno toolchain` with `args` in `project`, with `vars` set.
fn toolchain(project: &Project, args: &[&str], vars: &[(&str, &str)]) -> Run {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_cuno"));
    cmd.args(args);
    project.run_command_with(cmd, vars)
}

/// The line of the list that names `name`.
fn line<'a>(run: &'a Run, name: &str) -> &'a str {
    run.stdout
        .lines()
        .find(|l| l.split_whitespace().next() == Some(name))
        .unwrap_or_else(|| panic!("no {name} line in:\n{}", run.stdout))
}

/// One line per compiler and tool, in the spec's order, on stdout even
/// under `-q`, with the compiler selection picks marked with why, and no
/// other line marked.
///
/// Mutation: print the list to stderr or as status lines, leave a kind
/// out, reorder them, or mark another kind; this test fails.
#[test]
fn list_names_every_kind_and_marks_the_selected() {
    if require(Compiler::Msvc, Language::C, "x.c", "x.exe").is_none() {
        return;
    }
    let project = Project::new();
    let run = toolchain(
        &project,
        &["-q", "toolchain", "list"],
        &[("CUNO_COMPILER", "msvc")],
    );
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    let names: Vec<&str> = run
        .stdout
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .collect();
    assert_eq!(
        names,
        [
            "msvc",
            "clang",
            "clang-cl",
            "gcc",
            "clang-format",
            "clang-tidy"
        ],
        "{}",
        run.stdout
    );
    assert!(
        line(&run, "msvc").ends_with("(selected: CUNO_COMPILER)"),
        "{}",
        run.stdout
    );
    assert_eq!(
        run.stdout.matches("(selected:").count(),
        1,
        "{}",
        run.stdout
    );
}

/// `default` writes the user's default, which the selection then follows
/// where no other layer chooses: the list marks it as chosen by the
/// user-level config.toml.
///
/// Mutation: write the project's config, or another key, or under another
/// table; this test fails.
#[test]
fn default_changes_what_a_build_selects() {
    let project = Project::new();
    let run = toolchain(&project, &["toolchain", "default", "clang-cl"], &[]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert!(
        run.stderr
            .contains("Updated [toolchain] compiler = \"clang-cl\" in "),
        "{}",
        run.stderr
    );
    let config = std::fs::read_to_string(project.home().join("config.toml")).unwrap();
    assert!(config.contains("compiler = \"clang-cl\""), "{config}");

    let list = toolchain(&project, &["toolchain", "list"], &[]);
    assert!(
        line(&list, "clang-cl").contains("compiler in the user-level config.toml"),
        "{}",
        list.stdout
    );
}

/// A name `--compiler` would refuse is refused the same way (exit 3), and
/// the user's config is left as it was.
///
/// Mutation: write before judging the name; this test fails.
#[test]
fn default_refuses_an_unknown_name_and_writes_nothing() {
    let project = Project::new();
    let config = project.home().join("config.toml");
    std::fs::create_dir_all(project.home()).unwrap();
    std::fs::write(&config, "# mine\n").unwrap();
    let run = toolchain(&project, &["toolchain", "default", "cc"], &[]);
    assert_eq!(run.code, Some(3), "{}", run.stderr);
    assert!(
        run.stderr.contains("not a compiler cuno knows"),
        "{}",
        run.stderr
    );
    assert_eq!(std::fs::read_to_string(&config).unwrap(), "# mine\n");
}

/// Where a project chooses another compiler, the default is written and a
/// warning says what selects instead, and from where.
///
/// Mutation: drop the warning, or name the default as what selects; this
/// test fails.
#[test]
fn default_warns_when_a_project_selects_another() {
    let project = Project::new().file(
        "cuno.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[toolchain]\ncompiler = \"clang\"\n",
    );
    let run = toolchain(&project, &["toolchain", "default", "msvc"], &[]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert!(
        run.stderr.contains(
            "warning: msvc is the default, but here [toolchain] compiler in cuno.toml selects clang"
        ),
        "{}",
        run.stderr
    );
}

/// A default this machine cannot build with is written, and the warning
/// gives the reason a build would give: here, a configured clang that does
/// not run.
///
/// Mutation: drop the warning; this test fails.
#[test]
fn default_warns_when_the_compiler_cannot_be_used_here() {
    let project = Project::new().file("fake/clang.cmd", "@echo off\r\nexit /b 1\r\n");
    let fake = project.path().join("fake").join("clang.cmd");
    std::fs::create_dir_all(project.home()).unwrap();
    std::fs::write(
        project.home().join("config.toml"),
        format!("[toolchain.paths]\nclang = '{}'\n", fake.display()),
    )
    .unwrap();
    let run = toolchain(&project, &["toolchain", "default", "clang"], &[]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert!(
        run.stderr.contains("warning: [toolchain.paths] clang is"),
        "{}",
        run.stderr
    );
    let config = std::fs::read_to_string(project.home().join("config.toml")).unwrap();
    assert!(config.contains("compiler = \"clang\""), "{config}");
    assert!(config.contains("[toolchain.paths]"), "{config}");
}
