//! The `cuno` command: parses the command line and calls the other crates.
//!
//! See `docs/en/specs/commands.md` for the commands, options, output and exit
//! codes, and `docs/en/specs/architecture.md` for how the crates are split.

use clap::Parser;

/// The top-level command line. Subcommands are added with the milestones that
/// implement them (docs/en/roadmap.md).
#[derive(Parser)]
#[command(
    name = "cuno",
    // Without this, clap names the program after argv[0], which is
    // `cuno.exe` on Windows and whatever path the user typed elsewhere.
    bin_name = "cuno",
    version,
    about = "A Cargo-like build, package and test tool for C and C++",
    // Running `cuno` with nothing to do is a usage error (exit code 2), not a
    // success; see docs/en/specs/commands.md, "Exit codes".
    arg_required_else_help = true
)]
struct Cli {}

fn main() {
    // On `--version`, `--help` and usage errors, clap prints and exits itself:
    // 0 for the first two and 2 for the last, matching the spec's exit codes.
    let Cli {} = Cli::parse();
}
