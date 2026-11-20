//! The `cuno` command: parses the command line and calls the other crates.
//!
//! See `docs/en/specs/commands.md` for the commands, options, output and exit
//! codes, and `docs/en/specs/architecture.md` for how the crates are split.

use clap::{CommandFactory, Parser};

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
struct Cli {}

fn main() {
    // On `--version`, `--help` and usage errors, clap prints and exits by
    // itself: 0 for the first two and 2 for a usage error, as the spec's exit
    // codes require.
    //
    // Destructured rather than ignored, so that adding a field to `Cli` (the
    // first subcommand) fails to compile here until it is handled.
    let Cli {} = Cli::parse();
    let unused_spike = 1;
    if let Some(x) = Some(1) && x > 0 {
        let _ = x;
    }

    // Reaching here means no command was given. Like Cargo, print the usage to
    // stdout and succeed; see docs/en/specs/commands.md, "Without a command".
    // A failed write (a closed stdout) is ignored, matching what clap does for
    // `--help` and what Cargo does in both cases: the exit code stays 0.
    let _ = Cli::command().print_help();
}
