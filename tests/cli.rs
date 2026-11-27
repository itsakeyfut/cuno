//! The top-level command line: `--version`, `--help`, no command, and usage
//! errors.
//!
//! The output and exit codes asserted here are the user-facing interface
//! described in `docs/en/specs/commands.md`, "Without a command (`--version` /
//! `--help` / no arguments)" and "Exit codes", so each test compares the exact
//! text and code rather than a substring.
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
Everything C/C++ development needs, in one tool.

Usage: cuno [OPTIONS] [COMMAND]

Commands:
  new        Create a new package
  init       Make an existing directory into a package
  build      Compile the package
  check      Check the package for errors without building it
  run        Build the package and run one of its executables
  test       Build the package's tests and run them
  fmt        Format the package's code with clang-format
  lint       Analyse the package's code with clang-tidy
  clean      Remove the build's output
  doctor     Diagnose the environment and show a fix for each problem
  toolchain  List the compilers cuno finds, or choose the default one
  help       Print this message or the help of the given subcommand(s)

Options:
      --color <WHEN>  Coloring [possible values: auto, always, never]
  -v, --verbose       Show more, such as each command a build runs
  -q, --quiet         Show only errors, warnings and the compilers' output
  -h, --help          Print help
  -V, --version       Print version
";

/// Mutation: drop `version` from `#[command(...)]` in
/// `crates/cuno-cli/src/lib.rs`, or change the version format; this test fails.
#[test]
fn version_prints_the_name_and_the_package_version() {
    cuno(&["--version"]).assert(0, &format!("cuno {}\n", env!("CARGO_PKG_VERSION")), "");
}

/// Mutation (Windows only, where the executable is `cuno.exe`): remove
/// `bin_name = "cuno"` in `crates/cuno-cli/src/lib.rs`; the usage line
/// becomes `Usage: cuno.exe` and this test fails. Mutation: change
/// `description` in `crates/cuno-cli/Cargo.toml`; the first line changes
/// and this test fails.
#[test]
fn help_prints_usage_and_exits_zero() {
    cuno(&["--help"]).assert(0, HELP, "");
}

/// Like Cargo, `cuno` with no command prints the usage to stdout and succeeds.
///
/// Mutation: leave out writing the usage when no command is given, in
/// `dispatch` in `crates/cuno-cli/src/lib.rs`; `cuno` then exits 0 with no
/// output and this test fails.
#[test]
fn no_command_prints_usage_and_exits_zero() {
    cuno(&[]).assert(0, HELP, "");
}

/// An unknown option is a usage error (exit code 2), reported on stderr.
///
/// Mutation: add `ignore_errors = true` to `#[command(...)]` in
/// `crates/cuno-cli/src/lib.rs`;
/// the unknown option is then accepted and this test fails.
#[test]
fn unknown_option_is_a_usage_error() {
    cuno(&["--unknown"]).assert(
        2,
        "",
        "\
error: unexpected argument '--unknown' found

Usage: cuno [OPTIONS] [COMMAND]

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
error: unrecognized subcommand 'foo'

Usage: cuno [OPTIONS] [COMMAND]

For more information, try '--help'.
",
    );
}

/// `--color` takes `auto`, `always` or `never` (docs/en/specs/commands.md);
/// any other value is a usage error.
///
/// Mutation: remove `value_enum` from the `--color` argument in
/// `crates/cuno-cli/src/lib.rs`
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
/// before any command can fail (`crates/cuno-cli/src/lib.rs`,
/// `test_trigger`).
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

/// `[term] color` in a configuration file colors every command that reads
/// the settings: it ranks above `NO_COLOR` and below `--color`
/// (docs/en/specs/commands.md, "Available for all commands"), from the
/// warning the settings raise onwards.
///
/// Mutation: leave out `shell.configure` in `settings::resolve`, or call
/// it after the settings' warnings are printed, have `build`, `clean` or
/// `doctor` read the settings without `settings::resolve`, or let the
/// configured choice beat `--color`; this test fails.
#[test]
fn the_configured_color_colors_every_command() {
    let p = Project::new()
        .file(
            "cuno.toml",
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
        )
        // An unknown key, so that there is a warning to color.
        .file(".cuno/config.toml", "[term]\ncolor = \"always\"\nzzz = 1\n");
    let run = |args: &[&str]| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_cuno"));
        cmd.args(args);
        p.run_command_with(cmd, &[("NO_COLOR", "1")]).stderr
    };
    for command in ["build", "clean", "doctor"] {
        let colored = run(&[command]);
        assert!(
            colored.starts_with("\x1b[1;33mwarning\x1b[0m: "),
            "{command}: {colored}"
        );
        let plain = run(&["--color", "never", command]);
        assert!(!plain.contains('\x1b'), "{command}: {plain}");
    }
}

/// What clap prints itself (usage errors, `--help`, `cuno` with no command)
/// follows `--color`, then NO_COLOR, and never `CLICOLOR_FORCE`
/// (docs/en/specs/commands.md, "Available for all commands").
///
/// Mutation: leave clap's own color rule in place for its parsing, or for
/// the usage `cuno` prints with no command; this test fails.
#[test]
fn what_clap_prints_follows_the_color_order() {
    let p = Project::new();
    let run = |args: &[&str], vars: &[(&str, &str)]| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_cuno"));
        cmd.args(args);
        let run = p.run_command_with(cmd, vars);
        run.stdout + &run.stderr
    };
    let force: &[(&str, &str)] = &[("CLICOLOR_FORCE", "1")];
    for (args, vars, colored) in [
        (&["--color", "never", "--bogus"][..], force, false),
        (&["--color", "always", "--bogus"][..], &[][..], true),
        (
            &["--bogus"][..],
            &[("NO_COLOR", ""), ("CLICOLOR_FORCE", "1")][..],
            false,
        ),
        (&["--bogus"][..], force, false),
        (&["--color", "always"][..], &[][..], true),
        (&["--color", "always", "build", "--help"][..], &[][..], true),
        (&["build", "--help"][..], force, false),
    ] {
        let out = run(args, vars);
        assert_eq!(out.contains('\x1b'), colored, "{args:?} {vars:?}: {out}");
    }
}
