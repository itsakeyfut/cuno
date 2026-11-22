//! The `cuno` command: parses the command line and calls the other crates.
//!
//! See `docs/en/specs/commands.md` for the commands, options, output and exit
//! codes, and `docs/en/specs/architecture.md` for how the crates are split.

mod build;
mod clean;
mod doctor;
mod scratch;

use std::ffi::OsString;
use std::io::IsTerminal;
use std::num::NonZeroU32;
use std::path::PathBuf;

use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
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

    /// Show more, such as each command a build runs
    #[arg(short, long, global = true, conflicts_with = "quiet")]
    verbose: bool,

    /// Show only errors, warnings and the compilers' output
    #[arg(short, long, global = true)]
    quiet: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

/// The commands (docs/en/specs/commands.md).
#[derive(Subcommand)]
enum Command {
    /// Compile the package
    Build(BuildOpts),
    /// Check the package for errors without building it
    Check(BuildOpts),
    /// Build the package and run one of its executables
    Run {
        /// The executable to run, when there are several
        #[arg(long, value_name = "NAME")]
        bin: Option<String>,
        #[command(flatten)]
        opts: BuildOpts,
        /// Arguments for the program
        #[arg(last = true, value_name = "ARGS")]
        args: Vec<OsString>,
    },
    /// Remove the build's output
    Clean {
        /// Remove only the release profile's output
        #[arg(long, conflicts_with = "profile")]
        release: bool,
        /// Remove only the given profile's output (dev / release)
        #[arg(long, value_name = "NAME")]
        profile: Option<String>,
        /// The output directory to remove from
        #[arg(long, value_name = "PATH")]
        target_dir: Option<PathBuf>,
    },
    /// Diagnose the environment and show a fix for each problem
    Doctor {
        /// The compiler to diagnose (msvc / clang / clang-cl / gcc / safec)
        #[arg(long, value_name = "NAME")]
        compiler: Option<String>,
    },
}

/// The options of the build commands (docs/en/specs/commands.md, "Build
/// commands").
#[derive(Args)]
struct BuildOpts {
    /// Build with the release profile
    #[arg(long, conflicts_with = "profile")]
    release: bool,
    /// Build with the given profile (dev / release)
    #[arg(long, value_name = "NAME")]
    profile: Option<String>,
    /// The compiler to use (msvc / clang / clang-cl / gcc / safec)
    #[arg(long, value_name = "NAME")]
    compiler: Option<String>,
    /// Cross-compile for a target (reserved)
    #[arg(long, value_name = "TRIPLE")]
    target: Option<String>,
    /// Where to put the build's output
    #[arg(long, value_name = "PATH")]
    target_dir: Option<PathBuf>,
    /// How many steps to run at once (default: the number of cores)
    #[arg(short, long, value_name = "N")]
    jobs: Option<NonZeroU32>,
}

impl From<BuildOpts> for build::BuildArgs {
    fn from(o: BuildOpts) -> build::BuildArgs {
        build::BuildArgs {
            release: o.release,
            profile: o.profile,
            compiler: o.compiler,
            target: o.target,
            target_dir: o.target_dir,
            jobs: o.jobs,
        }
    }
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
    match run(cli, &env, &mut shell) {
        Ok(0) => {}
        Ok(code) => std::process::exit(code),
        Err(error) => std::process::exit(report(&mut shell, &*error)),
    }
    // A panic anywhere exits with Rust's own code, 101, the spec's code for an
    // internal error. This holds while `panic = "abort"` is not set in any
    // profile; tests/cli.rs, `a_panic_exits_101`, guards it.
}

/// Carries out the command line and returns the exit code. Every error a
/// command can meet comes back here rather than being printed where it
/// happens; `doctor` prints its problems itself and only says how many.
fn run(cli: Cli, env: &Env, shell: &mut Shell) -> Result<i32, Box<dyn CunoError>> {
    // Destructured rather than ignored, so that adding a field to `Cli`
    // fails to compile here until it is handled.
    let Cli {
        color: _,
        verbose,
        quiet,
        command,
    } = cli;

    #[cfg(debug_assertions)]
    test_trigger::fire(env)?;

    let (goal, opts) = match command {
        Some(Command::Build(opts)) => (build::Goal::Build, opts),
        Some(Command::Check(opts)) => (build::Goal::Check, opts),
        Some(Command::Run { bin, opts, args }) => (build::Goal::Run { bin, args }, opts),
        Some(Command::Clean {
            release,
            profile,
            target_dir,
        }) => {
            let cwd = std::env::current_dir()
                .map_err(|e| Box::new(CwdError(e.to_string())) as Box<dyn CunoError>)?;
            let args = clean::CleanArgs {
                release,
                profile,
                target_dir,
            };
            clean::clean(args, quiet, env, &cwd, shell)?;
            return Ok(0);
        }
        Some(Command::Doctor { compiler }) => {
            // The binary alone reads the process (architecture.md).
            let cwd = std::env::current_dir()
                .map_err(|e| Box::new(CwdError(e.to_string())) as Box<dyn CunoError>)?;
            let problems = doctor::doctor(
                compiler,
                env,
                &cwd,
                cuno_toolchain::HostOs::current(),
                shell,
            );
            return Ok(if problems == 0 { 0 } else { 1 });
        }
        None => {
            // No command was given. Like Cargo, print the usage to stdout
            // and succeed; see docs/en/specs/commands.md, "Without a
            // command". A failed write (a closed stdout) is ignored,
            // matching what clap does for `--help` and what Cargo does in
            // both cases: the exit code stays 0.
            let _ = Cli::command().print_help();
            return Ok(0);
        }
    };
    let cwd = std::env::current_dir()
        .map_err(|e| Box::new(CwdError(e.to_string())) as Box<dyn CunoError>)?;
    build::build(
        goal,
        opts.into(),
        build::Verbosity { verbose, quiet },
        env,
        &cwd,
        cuno_toolchain::HostOs::current(),
        shell,
    )
}

/// The current directory cannot be read (deleted, or no permission).
#[derive(Debug)]
struct CwdError(String);

impl std::fmt::Display for CwdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cannot read the current directory: {}", self.0)
    }
}

impl std::error::Error for CwdError {}

impl CunoError for CwdError {
    fn exit_class(&self) -> cuno_core::ExitClass {
        cuno_core::ExitClass::Setup
    }
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
