//! The top-level command line: `--version`, `--help` and usage errors.
//!
//! The output and exit codes asserted here are the user-facing interface
//! described in `docs/en/specs/commands.md`, so each test compares the exact
//! text and code rather than a substring.

use std::process::{Command, Output};

/// Runs the built `cuno` binary with `args` and returns what it printed.
fn cuno(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cuno"))
        .args(args)
        .output()
        .expect("the cuno binary should start")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout should be UTF-8")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr should be UTF-8")
}

/// The help text, shared by `--help` (on stdout) and by a bare `cuno` (on
/// stderr).
const HELP: &str = "\
A Cargo-like build, package and test tool for C and C++

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

/// Mutation: remove `bin_name = "cuno"` in `src/main.rs`; on Windows the usage
/// line becomes `Usage: cuno.exe` and this test fails.
#[test]
fn help_prints_usage_and_exits_zero() {
    let out = cuno(&["--help"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(stdout(&out), HELP);
    assert_eq!(stderr(&out), "");
}

/// Mutation: remove `arg_required_else_help = true` in `src/main.rs`; `cuno`
/// then exits 0 with no output and this test fails.
#[test]
fn no_arguments_is_a_usage_error() {
    let out = cuno(&[]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(stdout(&out), "");
    assert_eq!(stderr(&out), HELP);
}

/// An unknown option is a usage error (exit code 2), reported on stderr.
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
