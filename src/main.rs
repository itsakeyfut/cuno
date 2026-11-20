//! The `cuno` command: parses the command line and calls the other crates.
//!
//! See `docs/en/specs/commands.md` for the commands, options, output and exit
//! codes, and `docs/en/specs/architecture.md` for how the crates are split.

use std::io::IsTerminal;

use clap::{CommandFactory, Parser, ValueEnum};
use cuno_core::{ColorChoice, CunoError, Env, Shell};

/// The top-level command line. Subcommands are added with the milestones that
/// implement them (docs/en/roadmap.md).
#[derive(Parser)]
#[command(
    // `name` is what `--version` prints ("cuno 0.0.0"); `bin_name` is what
    // the usage line prints. Without `bin_name`, clap takes the executable's
    // file name, which is `cuno.exe` on Windows.
    name = "cuno",
    bin_name = "cuno",
    version,
    // Taken from `description` in Cargo.toml, so the two cannot drift apart.
    about
)]
struct Cli {
    // The line below is the help text `cuno --help` prints for this option,
    // pinned by tests/cli.rs. `None` when not given: NO_COLOR applies only
    // then (docs/en/specs/commands.md, "Available for all commands").
    /// Coloring
    #[arg(long, value_name = "WHEN", value_enum, global = true)]
    color: Option<Color>,
}

/// The values of `--color`. A separate type because `cuno-core` does not
/// depend on clap.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum Color {
    Auto,
    Always,
    Never,
}

impl From<Color> for ColorChoice {
    fn from(color: Color) -> ColorChoice {
        match color {
            Color::Auto => ColorChoice::Auto,
            Color::Always => ColorChoice::Always,
            Color::Never => ColorChoice::Never,
        }
    }
}

fn main() {
    // On `--version`, `--help` and usage errors, clap prints and exits by
    // itself: 0 for the first two and 2 for a usage error, as the spec's exit
    // codes require.
    let cli = Cli::parse();
    let env = Env::from_process();
    let mut shell = Shell::new(
        Box::new(std::io::stderr()),
        cli.color.map(ColorChoice::from),
        &env,
        std::io::stderr().is_terminal(),
    );
    if let Err(error) = run(cli, &env) {
        std::process::exit(report(&mut shell, &*error));
    }
    // A panic anywhere exits with Rust's own code, 101, the spec's code for an
    // internal error. This holds while `panic = "abort"` is not set in any
    // profile; tests/cli.rs, `a_panic_exits_101`, guards it.
}

/// Carries out the command line. Every error a command can meet comes back
/// here rather than being printed where it happens.
fn run(cli: Cli, env: &Env) -> Result<(), Box<dyn CunoError>> {
    // Destructured rather than ignored, so that adding a field to `Cli` (the
    // first subcommand) fails to compile here until it is handled.
    let Cli { color: _ } = cli;

    #[cfg(debug_assertions)]
    test_trigger::fire(env)?;
    let _ = env;

    // Reaching here means no command was given. Like Cargo, print the usage to
    // stdout and succeed; see docs/en/specs/commands.md, "Without a command".
    // A failed write (a closed stdout) is ignored, matching what clap does for
    // `--help` and what Cargo does in both cases: the exit code stays 0.
    let _ = Cli::command().print_help();
    Ok(())
}

/// The one place an error is shown and turned into an exit code
/// (docs/en/specs/architecture.md, "What goes in `cuno-core`").
fn report(shell: &mut Shell, error: &dyn CunoError) -> i32 {
    shell.error(error);
    if let Some(help) = error.help() {
        shell.help(help);
    }
    error.exit_class().code()
}

/// A way for tests/cli.rs to make the binary fail before any command can, so
/// that the error path, colors and exit codes are tested through the real
/// process. Compiled into debug builds only (which is what `cargo test`
/// runs); release builds have no trace of it.
#[cfg(debug_assertions)]
mod test_trigger {
    use cuno_core::{CunoError, Env, ExitClass};

    /// `CUNO_TEST_TRIGGER=setup-error` fails with an exit-class-3 error that
    /// has a help line; `CUNO_TEST_TRIGGER=panic` panics.
    pub fn fire(env: &Env) -> Result<(), Box<dyn CunoError>> {
        match env.get("CUNO_TEST_TRIGGER").and_then(|v| v.to_str()) {
            Some("setup-error") => Err(Box::new(Triggered)),
            Some("panic") => panic!("triggered by CUNO_TEST_TRIGGER"),
            _ => Ok(()),
        }
    }

    #[derive(Debug)]
    struct Triggered;
    impl std::fmt::Display for Triggered {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("triggered by CUNO_TEST_TRIGGER")
        }
    }
    impl std::error::Error for Triggered {}
    impl CunoError for Triggered {
        fn exit_class(&self) -> ExitClass {
            ExitClass::Setup
        }
        fn help(&self) -> Option<String> {
            Some("unset CUNO_TEST_TRIGGER".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mutation: swap any two arms of `From<Color> for ColorChoice`; this
    /// test fails.
    #[test]
    fn color_values_map_to_the_same_choice() {
        assert_eq!(ColorChoice::from(Color::Auto), ColorChoice::Auto);
        assert_eq!(ColorChoice::from(Color::Always), ColorChoice::Always);
        assert_eq!(ColorChoice::from(Color::Never), ColorChoice::Never);
    }
}
