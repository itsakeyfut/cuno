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

use std::process::Command;

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

Usage: cuno [OPTIONS]

Options:
      --color <WHEN>  Coloring [possible values: auto, always, never]
  -h, --help          Print help
  -V, --version       Print version
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

Usage: cuno [OPTIONS]

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

Usage: cuno [OPTIONS]

For more information, try '--help'.
",
    );
}

/// `--color` takes `auto`, `always` or `never` (docs/en/specs/commands.md);
/// any other value is a usage error.
///
/// Mutation: remove `value_enum` from the `--color` argument in `src/main.rs`
/// (or make it a free string); this test fails.
#[test]
fn an_unknown_color_is_a_usage_error() {
    cuno(&["--color", "bogus"]).assert(
        2,
        "",
        "error: invalid value 'bogus' for '--color <WHEN>'
  [possible values: auto, always, never]

For more information, try '--help'.
",
    );
}

/// `--color never` is accepted wherever a command is (it is a global option).
///
/// Mutation: remove the `--color` argument from `Cli`; this test fails.
#[test]
fn color_is_accepted_without_a_command() {
    cuno(&["--color", "never"]).assert(0, HELP, "");
}

/// Runs cuno with `args` and the debug-only `CUNO_TEST_TRIGGER` set to
/// `trigger`, plus `extra` environment variables, in a fresh project. The
/// trigger lets these tests reach the error path through the real process
/// before any command can fail (src/main.rs, `test_trigger`).
fn triggered(trigger: &str, args: &[&str], extra: &[(&str, &str)]) -> Run {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_cuno"));
    cmd.args(args).env("CUNO_TEST_TRIGGER", trigger);
    for (name, value) in extra {
        cmd.env(name, value);
    }
    Project::new().run_command(cmd)
}

/// An error reaches stderr as `error:` and `help:` lines, without color when
/// stderr is not a terminal and no `--color` was given, and the process exits
/// with the error's class (3 for a setup problem).
///
/// Mutation: drop the `std::process::exit` call in `main`, print the help
/// before the error in `report`, make `Env::from_process` return an empty
/// environment, or pass `true` as the terminal flag; this test fails.
#[test]
fn an_error_exits_with_its_class_and_prints_error_and_help() {
    triggered("setup-error", &[], &[]).assert(
        3,
        "",
        "error: triggered by CUNO_TEST_TRIGGER\nhelp: unset CUNO_TEST_TRIGGER\n",
    );
}

/// `--color always` colors cuno's own lines, and wins over `NO_COLOR`.
///
/// Mutation: ignore `cli.color` in `main` (pass `None`), or let `NO_COLOR`
/// override an explicit choice in `Shell::new`; this test fails.
#[test]
fn color_always_colors_errors_even_with_no_color() {
    let expected = "\x1b[1;31merror\x1b[0m: triggered by CUNO_TEST_TRIGGER\n\x1b[1;36mhelp\x1b[0m: unset CUNO_TEST_TRIGGER\n";
    triggered("setup-error", &["--color", "always"], &[]).assert(3, "", expected);
    triggered("setup-error", &["--color", "always"], &[("NO_COLOR", "1")]).assert(3, "", expected);
}

/// A panic exits 101, the spec's code for an internal error.
///
/// Mutation: set `panic = "abort"` in a `[profile.dev]` of `Cargo.toml`; the
/// exit code changes and this test fails.
#[test]
fn a_panic_exits_101() {
    let run = triggered("panic", &[], &[]);
    assert_eq!(run.code, Some(101), "{run:?}");
}
