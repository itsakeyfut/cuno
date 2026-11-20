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

mod support;

use support::{Project, Run};

/// Runs the built `cuno` binary with `args` in a fresh, empty project.
///
/// The environment is hermetic (see `support::hermetic`): with
/// `CLICOLOR_FORCE=1` inherited, clap would add ANSI escapes to the help and
/// error text and every exact-text test would fail.
fn cuno(args: &[&str]) -> Run {
    Project::new().cuno(args)
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
    cuno(&["--version"]).assert(0, &format!("cuno {}\n", env!("CARGO_PKG_VERSION")), "");
}

/// Mutation (Windows only, where the executable is `cuno.exe`): remove
/// `bin_name = "cuno"` in `src/main.rs`; the usage line becomes
/// `Usage: cuno.exe` and this test fails. Mutation: change `description` in
/// `Cargo.toml`; the first line changes and this test fails.
#[test]
fn help_prints_usage_and_exits_zero() {
    cuno(&["--help"]).assert(0, HELP, "");
}

/// Like Cargo, `cuno` with no command prints the usage to stdout and succeeds.
///
/// Mutation: remove the `print_help` call at the end of `main` in
/// `src/main.rs`; `cuno` then exits 0 with no output and this test fails.
#[test]
fn no_command_prints_usage_and_exits_zero() {
    cuno(&[]).assert(0, HELP, "");
}

/// An unknown option is a usage error (exit code 2), reported on stderr.
///
/// Mutation: add `ignore_errors = true` to `#[command(...)]` in `src/main.rs`;
/// the unknown option is then accepted and this test fails.
#[test]
fn unknown_option_is_a_usage_error() {
    cuno(&["--unknown"]).assert(
        2,
        "",
        "\
error: unexpected argument '--unknown' found

Usage: cuno

For more information, try '--help'.
",
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
    cuno(&["foo"]).assert(
        2,
        "",
        "\
error: unexpected argument 'foo' found

Usage: cuno

For more information, try '--help'.
",
    );
}
