//! The `cuno` command: parses the command line and calls the other crates.
//!
//! See `docs/en/specs/commands.md` for the commands, options, output and exit
//! codes, and `docs/en/specs/architecture.md` for how the crates are split.

mod build;
mod clean;
mod doctor;
mod scratch;
mod settings;
mod toolchain;

use std::ffi::OsString;
use std::io::IsTerminal;
use std::num::NonZeroU32;
use std::path::PathBuf;

use clap::error::ErrorKind;
use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
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
    // pinned by tests/cli.rs. `None` when not given: `[term] color`, then
    // NO_COLOR, apply only then (docs/en/specs/commands.md, "Available for
    // all commands").
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

/// The `--color` the command line gives, read before clap parses it so
/// that what clap prints follows it too: the last one before a bare `--`
/// (what follows belongs to the program `cuno run` runs), as
/// `--color <when>` or `--color=<when>`. A value that is not one of
/// `--color`'s counts as none, so the usage error it causes is colored as if
/// `--color` were not given.
fn color_before_parsing(args: &[OsString]) -> Option<ColorChoice> {
    let mut found = None;
    let mut args = args.iter().skip(1);
    while let Some(arg) = args.next() {
        // `Some(None)` is a `--color` whose value is missing or not
        // Unicode, which counts as none like any other invalid value.
        let value = match arg.to_str() {
            Some("--") => break,
            Some("--color") => Some(args.next().and_then(|v| v.to_str())),
            Some(arg) => arg.strip_prefix("--color=").map(Some),
            None => arg
                .as_encoded_bytes()
                .starts_with(b"--color=")
                .then_some(None),
        };
        if let Some(value) = value {
            found = Some(
                value
                    .and_then(|v| Color::from_str(v, false).ok())
                    .map(ColorChoice::from),
            );
        }
    }
    found.flatten()
}

/// The color clap uses for what it prints itself: the choice given, or with
/// none, no color under `NO_COLOR` (set to anything, even empty), and
/// otherwise whether the output is a terminal. Never clap's own `Auto`,
/// which reads `CLICOLOR_FORCE`, a variable cuno does not read.
fn parser_color(given: Option<ColorChoice>, no_color: bool, terminal: bool) -> clap::ColorChoice {
    let on = match given {
        Some(ColorChoice::Always) => true,
        Some(ColorChoice::Never) => false,
        Some(ColorChoice::Auto) => terminal,
        None => terminal && !no_color,
    };
    if on {
        clap::ColorChoice::Always
    } else {
        clap::ColorChoice::Never
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
    let args: Vec<OsString> = std::env::args_os().collect();
    let env = Env::from_process();
    // What clap prints itself is colored by cuno's rule, not clap's
    // (docs/en/specs/commands.md, "Available for all commands"): `--color`
    // as written, then NO_COLOR, then whether the output is a terminal.
    let given = color_before_parsing(&args);
    let no_color = env.get("NO_COLOR").is_some();
    let to_stdout = parser_color(given, no_color, std::io::stdout().is_terminal());
    let to_stderr = parser_color(given, no_color, std::io::stderr().is_terminal());
    // On `--version`, `--help` and usage errors, clap prints and exits by
    // itself: 0 for the first two and 2 for a usage error, as the spec's exit
    // codes require. Help and the version go to stdout, so they are parsed
    // again with stdout's color.
    let parse = |color| Cli::command().color(color).try_get_matches_from(&args);
    let cli = match parse(to_stderr) {
        // Formatted against the command, as `Cli::parse` does, so such an
        // error would carry the usage line too.
        Ok(matches) => Cli::from_arg_matches(&matches)
            .unwrap_or_else(|e| e.format(&mut Cli::command().color(to_stderr)).exit()),
        Err(error) => match error.kind() {
            ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
                parse(to_stdout).err().unwrap_or(error).exit()
            }
            _ => error.exit(),
        },
    };
    let mut shell = Shell::new(
        Box::new(std::io::stderr()),
        cli.color.map(ColorChoice::from),
        &env,
        std::io::stderr().is_terminal(),
    );
    match run(cli, &env, &mut shell, to_stdout) {
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
/// `help_color` is how the usage printed with no command is colored.
fn run(
    cli: Cli,
    env: &Env,
    shell: &mut Shell,
    help_color: clap::ColorChoice,
) -> Result<i32, Box<dyn CunoError>> {
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
                quiet,
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
            let _ = Cli::command().color(help_color).print_help();
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

    fn given(args: &[&str]) -> Option<ColorChoice> {
        let args: Vec<OsString> = std::iter::once("cuno")
            .chain(args.iter().copied())
            .map(OsString::from)
            .collect();
        color_before_parsing(&args)
    }

    /// Mutation: read only one of the two spellings, take the first
    /// `--color` instead of the last, read past `--`, or keep an earlier
    /// value past an invalid one, a missing one or one that is not Unicode;
    /// this test fails.
    #[test]
    fn the_color_is_read_before_parsing() {
        assert_eq!(
            given(&["--color", "never", "build"]),
            Some(ColorChoice::Never)
        );
        assert_eq!(
            given(&["build", "--color=always"]),
            Some(ColorChoice::Always)
        );
        assert_eq!(
            given(&["--color", "never", "build", "--color", "always"]),
            Some(ColorChoice::Always)
        );
        assert_eq!(given(&["run", "--", "--color", "never"]), None);
        assert_eq!(given(&["--color", "bogus"]), None);
        assert_eq!(given(&["--color=never", "--color=bogus"]), None);
        assert_eq!(given(&["build"]), None);
        assert_eq!(given(&["--color=always", "--color"]), None);
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStringExt;
            let lone = OsString::from_wide(&[0xD800]);
            let mut equals = OsString::from("--color=");
            equals.push(&lone);
            for bad in [vec![OsString::from("--color"), lone], vec![equals]] {
                let args: Vec<OsString> = ["cuno", "--color", "always"]
                    .into_iter()
                    .map(OsString::from)
                    .chain(bad)
                    .collect();
                assert_eq!(color_before_parsing(&args), None, "{args:?}");
            }
        }
    }

    /// Mutation: hand clap its own `Auto`, let NO_COLOR beat a choice given,
    /// or ignore it with none; this test fails.
    #[test]
    fn what_clap_prints_follows_cunos_order() {
        use clap::ColorChoice as Clap;
        assert_eq!(
            parser_color(Some(ColorChoice::Always), true, false),
            Clap::Always
        );
        assert_eq!(
            parser_color(Some(ColorChoice::Never), false, true),
            Clap::Never
        );
        assert_eq!(
            parser_color(Some(ColorChoice::Auto), true, true),
            Clap::Always
        );
        assert_eq!(
            parser_color(Some(ColorChoice::Auto), false, false),
            Clap::Never
        );
        assert_eq!(parser_color(None, true, true), Clap::Never);
        assert_eq!(parser_color(None, false, true), Clap::Always);
        assert_eq!(parser_color(None, false, false), Clap::Never);
    }
}
