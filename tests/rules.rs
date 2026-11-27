//! Each rule that every command must follow, run through every command.
//!
//! A rule here once lived in each command that needed it, and each new
//! command that needed it was one more place to forget it. One test per rule
//! has a row per command: the arguments it runs, or why the rule does not
//! apply to it. [`every_command_has_a_row_in_every_rule`] reads the commands
//! from `cuno --help`, so a new command fails until each rule says what it
//! does there.

mod support;

use std::process::Command;

use support::{Project, Run};

/// What a rule does for one command.
enum Row {
    /// The command's arguments, after which the rule is asserted.
    Runs(&'static [&'static str]),
    /// The rule does not apply, and why.
    DoesNotApply(&'static str),
}

use Row::{DoesNotApply, Runs};

/// The colour order: `[term] color` colors every command's output, from
/// the settings' own warnings onwards.
const COLOR: &[(&str, Row)] = &[
    ("new", Runs(&["new", "app", "--vcs", "none"])),
    ("build", Runs(&["build"])),
    ("check", Runs(&["check"])),
    ("run", Runs(&["run"])),
    ("test", Runs(&["test"])),
    ("fmt", Runs(&["fmt"])),
    ("lint", Runs(&["lint"])),
    ("clean", Runs(&["clean"])),
    ("doctor", Runs(&["doctor"])),
    ("toolchain", Runs(&["toolchain", "list"])),
];

/// Outside a project the missing project is the error, whatever the
/// settings say.
const OUTSIDE_A_PROJECT: &[(&str, Row)] = &[
    (
        "new",
        DoesNotApply("it creates a package, and needs none around it"),
    ),
    ("build", Runs(&["build"])),
    ("check", Runs(&["check"])),
    ("run", Runs(&["run"])),
    ("test", Runs(&["test"])),
    ("fmt", Runs(&["fmt"])),
    ("lint", Runs(&["lint"])),
    ("clean", Runs(&["clean"])),
    (
        "doctor",
        DoesNotApply("it diagnoses the environment, in a project or not"),
    ),
    (
        "toolchain",
        DoesNotApply("it lists the machine's compilers, in a project or not"),
    ),
];

/// The output directory is judged as the file system names it: a setting
/// that names the project root in another case is the project root.
const OUTPUT_DIRECTORY: &[(&str, Row)] = &[
    ("new", DoesNotApply("it has no output directory")),
    ("build", Runs(&["build"])),
    ("check", Runs(&["check"])),
    ("run", Runs(&["run"])),
    ("test", Runs(&["test"])),
    ("fmt", Runs(&["fmt"])),
    ("lint", Runs(&["lint"])),
    ("clean", Runs(&["clean"])),
    ("doctor", Runs(&["doctor"])),
    ("toolchain", Runs(&["toolchain", "list"])),
];

/// A configured tool copy that cannot be used is reported, even when
/// another compiler is selected.
const CONFIGURED_COPY: &[(&str, Row)] = &[
    ("new", DoesNotApply("it runs no tool")),
    ("build", Runs(&["build", "--compiler", "msvc"])),
    ("check", Runs(&["check", "--compiler", "msvc"])),
    ("run", Runs(&["run", "--compiler", "msvc"])),
    ("test", Runs(&["test", "--compiler", "msvc"])),
    ("fmt", Runs(&["fmt"])),
    ("lint", Runs(&["lint", "--compiler", "msvc"])),
    ("clean", DoesNotApply("it runs no tool")),
    ("doctor", Runs(&["doctor", "--compiler", "msvc"])),
    (
        "toolchain",
        DoesNotApply(
            "it lists the copy as not usable, with this reason, on stdout; reporting it as a problem is doctor's",
        ),
    ),
];

/// Every rule's table, named.
const RULES: &[(&str, &[(&str, Row)])] = &[
    ("COLOR", COLOR),
    ("OUTSIDE_A_PROJECT", OUTSIDE_A_PROJECT),
    ("OUTPUT_DIRECTORY", OUTPUT_DIRECTORY),
    ("CONFIGURED_COPY", CONFIGURED_COPY),
];

/// A package with one source, so that each command has a project to work in.
fn package() -> Project {
    Project::new()
        .file(
            "cuno.toml",
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
        )
        .file("src/main.cpp", "int main() { return 0; }\n")
}

/// Runs cuno with `args` in `p`, with `vars` added to its environment.
fn cuno(p: &Project, args: &[&str], vars: &[(&str, &str)]) -> Run {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_cuno"));
    cmd.args(args);
    p.run_command_with(cmd, vars)
}

/// The commands `cuno --help` lists, `help` aside.
fn commands() -> Vec<String> {
    let help = Project::new().cuno(&["--help"]).stdout;
    help.lines()
        .skip_while(|line| !line.starts_with("Commands:"))
        .skip(1)
        .take_while(|line| !line.trim().is_empty())
        .filter_map(|line| line.split_whitespace().next())
        .filter(|name| *name != "help")
        .map(str::to_owned)
        .collect()
}

/// Every rule has exactly one row for each command `cuno --help` lists,
/// and a row the rule does not apply to says why.
///
/// Mutation: drop a row from any table, or add a command to the CLI
/// without rows; this test fails, naming the rule and the command.
#[test]
fn every_command_has_a_row_in_every_rule() {
    let commands = commands();
    assert!(commands.contains(&"build".to_owned()), "{commands:?}");
    for (rule, rows) in RULES {
        let mut named: Vec<&str> = rows.iter().map(|(command, _)| *command).collect();
        named.sort_unstable();
        let mut expected: Vec<&str> = commands.iter().map(String::as_str).collect();
        expected.sort_unstable();
        assert_eq!(named, expected, "{rule}: the rows are not the commands");
        for (command, row) in rows.iter() {
            if let DoesNotApply(why) = row {
                assert!(!why.is_empty(), "{rule}: {command} says no reason");
            }
        }
    }
}

/// `[term] color = "always"` in the user's `config.toml` colors every
/// command's output, from the warning the settings raise onwards
/// (`docs/en/specs/commands.md`, "Available for all commands").
///
/// Mutation: dispatch any one command without resolving the settings, or
/// apply `[term] color` after the warnings; this test fails, naming the
/// command.
#[test]
fn the_configured_color_colors_every_command() {
    for (command, row) in COLOR {
        let Runs(args) = row else { continue };
        let p = package();
        // An unknown key, so that there is a warning to color.
        std::fs::write(
            p.home().join("config.toml"),
            "zzz = 1\n[term]\ncolor = \"always\"\n",
        )
        .unwrap();
        let stderr = cuno(&p, args, &[]).stderr;
        assert!(
            stderr.starts_with("\x1b[1;33mwarning\x1b[0m: "),
            "{command}: {stderr}"
        );
    }
}

/// Outside a project every command that needs one reports the missing
/// project, and nothing the settings say: here the output directory, which
/// `CUNO_TARGET_DIR=.` puts on the current directory
/// (`docs/en/specs/commands.md`, "`cuno build` / `cuno check`").
///
/// Mutation: report the settings' error first, in any one of these
/// commands; this test fails, naming the command.
#[test]
fn outside_a_project_the_missing_project_is_the_error() {
    for (command, row) in OUTSIDE_A_PROJECT {
        let Runs(args) = row else { continue };
        let p = Project::new();
        let run = cuno(&p, args, &[("CUNO_TARGET_DIR", ".")]);
        assert_eq!(run.code, Some(3), "{command}: {}", run.stderr);
        assert!(
            run.stderr
                .starts_with("error: could not find cuno.toml in "),
            "{command}: {}",
            run.stderr
        );
    }
}

/// An output directory that names the project root in another case is the
/// project root, and every command that uses the output directory refuses
/// it (`docs/en/specs/build.md`, "Output directory").
///
/// Windows only: elsewhere the project's directory in another case is
/// another directory.
///
/// Mutation: in `check_output_dir`, replace `paths::within(guarded, dir,
/// case)` with `dir.starts_with(guarded)`, comparing the paths as written;
/// this test fails, naming the command.
#[cfg(windows)]
#[test]
fn the_output_directory_is_judged_as_the_file_system_names_it() {
    for (command, row) in OUTPUT_DIRECTORY {
        let Runs(args) = row else { continue };
        let p = package();
        let root = p.path();
        let other_case =
            root.with_file_name(root.file_name().unwrap().to_string_lossy().to_uppercase());
        let run = cuno(
            &p,
            args,
            &[("CUNO_TARGET_DIR", other_case.to_str().unwrap())],
        );
        assert!(
            run.stderr
                .contains("is the project root or a directory above it"),
            "{command}: {}",
            run.stderr
        );
    }
}

/// A `[toolchain.paths]` copy with no file there is reported by every command
/// that runs a tool, even when another compiler is selected
/// (`docs/en/specs/toolchain.md`, "LLVM (clang / clang-cl / clang-format /
/// clang-tidy)").
///
/// Mutation: skip the check that a configured copy is a file
/// (`ConfiguredToolMissing` in `llvm::detect`); this test fails, naming
/// the command.
#[test]
fn a_configured_copy_is_checked_by_every_command_that_runs_a_tool() {
    for (command, row) in CONFIGURED_COPY {
        let Runs(args) = row else { continue };
        let p = package();
        let missing = p.path().join("nope").join("clang.exe");
        let missing = missing.to_str().unwrap().replace('\\', "/");
        let p = p.file(
            ".cuno/config.toml",
            &format!("[toolchain.paths]\nclang = \"{missing}\"\n"),
        );
        let run = cuno(&p, args, &[]);
        assert!(
            run.stderr.contains(&format!(
                "[toolchain.paths] clang is {missing}, but there is no file there"
            )),
            "{command}: {}",
            run.stderr
        );
    }
}
