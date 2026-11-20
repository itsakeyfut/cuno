//! The top-level command line: `--version`, `--help`, no command, and usage
//! errors.
//!
//! The output and exit codes asserted here are the user-facing interface
//! described in `docs/en/specs/commands.md`, "Without a command" and "Exit
//! codes", so each test compares the exact text and code rather than a
//! substring.
//!
//! The expected strings are clap's own layout and wording. They are expected to
//! change when the first subcommand is added (the usage line gains
//! `<COMMAND>`) or when clap is upgraded; update them then rather than bending
//! the code to keep the old text.

use std::process::{Command, Output};

/// Runs the built `cuno` binary with `args` and returns what it printed.
///
/// The colour variables clap reads are removed, so the output does not depend
/// on the environment the tests run in: with `CLICOLOR_FORCE=1` set, clap would
/// otherwise add ANSI escapes to the help and error text and every exact-text
/// test would fail.
fn cuno(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cuno"))
        .args(args)
        .env_remove("CLICOLOR_FORCE")
        .env_remove("CLICOLOR")
        .env_remove("NO_COLOR")
        .output()
        .expect("the cuno binary should start")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout should be UTF-8")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr should be UTF-8")
}

/// The usage text, printed by `--help` and by `cuno` with no command.
const HELP: &str = "\
One comfortable tool for C/C++ development.

Usage: cuno

Options:
  -h, --help     Print help
  -V, --version  Print version
";

/// Mutation: drop `version` from `#[command(...)]` in `src/main.rs`, or change
/// the version format; this test fails.
#[test]
fn version_prints_the_name_and_the_package_version() {
    let out = cuno(&["--version"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        stdout(&out),
        format!("cuno {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(stderr(&out), "");
}

/// Mutation (Windows only, where the executable is `cuno.exe`): remove
/// `bin_name = "cuno"` in `src/main.rs`; the usage line becomes
/// `Usage: cuno.exe` and this test fails. Mutation: change `description` in
/// `Cargo.toml`; the first line changes and this test fails.
#[test]
fn help_prints_usage_and_exits_zero() {
    let out = cuno(&["--help"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(stdout(&out), HELP);
    assert_eq!(stderr(&out), "");
}

/// Like Cargo, `cuno` with no command prints the usage to stdout and succeeds.
///
/// Mutation: remove the `print_help` call at the end of `main` in
/// `src/main.rs`; `cuno` then exits 0 with no output and this test fails.
#[test]
fn no_command_prints_usage_and_exits_zero() {
    let out = cuno(&[]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(stdout(&out), HELP);
    assert_eq!(stderr(&out), "");
}

/// An unknown option is a usage error (exit code 2), reported on stderr.
///
/// Mutation: add `ignore_errors = true` to `#[command(...)]` in `src/main.rs`;
/// the unknown option is then accepted and this test fails.
#[test]
fn unknown_option_is_a_usage_error() {
    let out = cuno(&["--unknown"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(stdout(&out), "");
    assert_eq!(
        stderr(&out),
        "\
error: unexpected argument '--unknown' found

Usage: cuno

For more information, try '--help'.
"
    );
}

/// An unexpected positional argument is a usage error (exit code 2), reported
/// on stderr.
///
/// Mutation: give `Cli` a hidden catch-all field
/// (`#[arg(hide = true)] _rest: Vec<String>`); `cuno foo` is then accepted and
/// this test fails.
#[test]
fn unexpected_argument_is_a_usage_error() {
    let out = cuno(&["foo"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(stdout(&out), "");
    assert_eq!(
        stderr(&out),
        "\
error: unexpected argument 'foo' found

Usage: cuno

For more information, try '--help'.
"
    );
}
