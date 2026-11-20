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
    /// Coloring
    #[arg(long, value_name = "WHEN", value_enum, global = true, default_value_t = Color::Auto)]
    color: Color,
}

/// The values of `--color` (docs/en/specs/commands.md, "Available for all
/// commands"). A separate type because `cuno-core` does not depend on clap.
#[derive(Clone, Copy, ValueEnum)]
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
        cli.color.into(),
        &env,
        std::io::stderr().is_terminal(),
    );
    if let Err(error) = run(cli) {
        std::process::exit(report(&mut shell, &*error));
    }
    // A panic anywhere keeps Rust's own exit code, 101, which is the spec's
    // code for an internal error.
}

/// Carries out the command line. Every error a command can meet comes back
/// here rather than being printed where it happens.
fn run(cli: Cli) -> Result<(), Box<dyn CunoError>> {
    // Destructured rather than ignored, so that adding a field to `Cli` (the
    // first subcommand) fails to compile here until it is handled.
    let Cli { color: _ } = cli;

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
    error.exit_class().code()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cuno_core::ExitClass;
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);
    impl Write for Captured {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().write(buf)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[derive(Debug)]
    struct NoCompiler;
    impl std::fmt::Display for NoCompiler {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("no compiler found")
        }
    }
    impl std::error::Error for NoCompiler {}
    impl CunoError for NoCompiler {
        fn exit_class(&self) -> ExitClass {
            ExitClass::Config
        }
        fn help(&self) -> Option<String> {
            Some("install Build Tools".into())
        }
    }

    /// Mutation: make `report` return a fixed 1, or skip `shell.error`; this
    /// test fails.
    #[test]
    fn an_error_is_shown_and_exits_with_its_class() {
        let out = Captured::default();
        let mut shell = Shell::new(
            Box::new(out.clone()),
            ColorChoice::Never,
            &Env::default(),
            false,
        );
        assert_eq!(report(&mut shell, &NoCompiler), 3);
        assert_eq!(
            String::from_utf8(out.0.lock().unwrap().clone()).unwrap(),
            "error: no compiler found\nhelp: install Build Tools\n"
        );
    }
}
