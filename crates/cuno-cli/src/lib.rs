//! The `cuno` command line: parses it, resolves the settings, dispatches to
//! a command and reports what went wrong, from values the binary collected
//! from the process. Each command is a module that combines the other crates
//! and shows the result.
//!
//! See `docs/en/specs/commands.md` for the commands, options, output and exit
//! codes, and `docs/en/specs/architecture.md`, "What goes in `src/` and
//! `cuno-cli`", for what belongs here.

mod build;
mod clean;
mod doctor;
mod fmt;
mod lint;
mod new;
mod self_cmd;
mod toolchain;

use std::ffi::OsString;
use std::io::{BufRead, Write};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use cuno_core::{ColorChoice, CunoError, Env, Shell, Verbosity};
use cuno_manifest::{CliSettings, ConfigError, Inputs, Settings};

// So the binary, which may depend only on this crate and `cuno-core`, can
// name the host it collects (`architecture.md`, "Dependencies between
// crates").
pub use cuno_toolchain::HostOs;

/// What only the process has, collected by the binary and handed in as
/// values: no library reads the process itself (`docs/en/specs/
/// architecture.md`, "Dependencies between crates").
pub struct Process {
    /// The command line, the program's name first.
    pub args: Vec<OsString>,
    /// The environment variables.
    pub env: Env,
    /// The current directory, or why it cannot be read. Only a command that
    /// needs it reports the reason, so `--help` works without one.
    pub cwd: Result<PathBuf, String>,
    /// The operating system cuno runs on.
    pub host: HostOs,
    /// The temporary directory.
    pub temp: PathBuf,
    /// Where the running executable is, or why that cannot be found. Only
    /// `cuno self` needs it.
    pub exe: Result<PathBuf, String>,
    /// How many processors this machine has: the default number of jobs.
    pub cores: NonZeroU32,
    /// Standard output: help, the version and the usage. Any writer, so a
    /// test can read what was written.
    pub stdout: Box<dyn Write>,
    /// Whether standard output is a terminal.
    pub stdout_is_terminal: bool,
    /// Standard error: everything else cuno prints.
    pub stderr: Box<dyn Write>,
    /// Whether standard error is a terminal.
    pub stderr_is_terminal: bool,
    /// Standard input: the answer to a question cuno asks.
    pub stdin: Box<dyn BufRead>,
    /// Whether standard input is a terminal; cuno only asks one that is.
    pub stdin_is_terminal: bool,
}

/// The top-level command line. Subcommands are added with the milestones that
/// implement them (docs/en/roadmap.md).
#[derive(Parser)]
#[command(
    // `name` is what `--version` prints ("cuno <version>"); `bin_name` is what
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
    /// Create a new package
    New {
        /// Where to create it
        #[arg(value_name = "PATH")]
        path: PathBuf,
        /// The language (c / c++; default c++)
        #[arg(long, value_name = "LANG", value_parser = ["c", "c++"])]
        lang: Option<String>,
        /// The language standard (c++17 / c++20 / c++23 / c11 / c17 / c23)
        #[arg(long, value_name = "EDITION", value_parser = ["c++17", "c++20", "c++23", "c11", "c17", "c23"])]
        edition: Option<String>,
        /// The package's name (default: the directory's)
        #[arg(long, value_name = "NAME")]
        name: Option<String>,
        /// Version control (git / none)
        #[arg(long, value_name = "VCS", value_parser = ["git", "none"], default_value = "git")]
        vcs: String,
    },
    /// Make an existing directory into a package
    Init {
        /// The directory (default: the current one)
        #[arg(value_name = "PATH")]
        path: Option<PathBuf>,
        /// The language (c / c++; default: the sources', else c++)
        #[arg(long, value_name = "LANG", value_parser = ["c", "c++"])]
        lang: Option<String>,
        /// The language standard (c++17 / c++20 / c++23 / c11 / c17 / c23)
        #[arg(long, value_name = "EDITION", value_parser = ["c++17", "c++20", "c++23", "c11", "c17", "c23"])]
        edition: Option<String>,
        /// The package's name (default: the directory's)
        #[arg(long, value_name = "NAME")]
        name: Option<String>,
        /// Version control (git / none)
        #[arg(long, value_name = "VCS", value_parser = ["git", "none"], default_value = "git")]
        vcs: String,
    },
    /// Compile the package
    Build(BuildOpts),
    /// Check the package for errors without building it
    Check {
        /// Check the tests instead of the program
        #[arg(long)]
        tests: bool,
        /// Check both the program and the tests
        #[arg(long)]
        all_targets: bool,
        #[command(flatten)]
        opts: BuildOpts,
    },
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
    /// Build the package's tests and run them
    Test {
        /// Run only the tests whose names contain this
        #[arg(value_name = "FILTER")]
        filter: Option<String>,
        /// Show every test's output, not only a failed one's
        #[arg(long)]
        nocapture: bool,
        #[command(flatten)]
        opts: BuildOpts,
    },
    /// Format the package's code with clang-format
    Fmt {
        /// Do not format; fail if any file is not formatted (for CI)
        #[arg(long)]
        check: bool,
    },
    /// Analyse the package's code with clang-tidy
    Lint {
        /// Fix what clang-tidy can, in the sources themselves
        #[arg(long)]
        fix: bool,
        /// The compiler whose commands are analysed (msvc / clang / clang-cl / gcc / safec)
        #[arg(long, value_name = "NAME")]
        compiler: Option<String>,
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
    /// List the compilers cuno finds, or choose the default one
    Toolchain {
        #[command(subcommand)]
        command: ToolchainCommand,
    },
    /// Update cuno itself
    #[command(name = "self")]
    SelfCmd {
        #[command(subcommand)]
        command: SelfCommand,
    },
}

/// `cuno self`'s commands (docs/en/specs/commands.md).
#[derive(Subcommand)]
enum SelfCommand {
    /// Update cuno to the latest release
    Update,
    /// Remove cuno's home and, if its installer placed it, cuno itself
    Uninstall {
        /// Remove without asking
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

/// `cuno toolchain`'s commands (docs/en/specs/commands.md).
#[derive(Subcommand)]
enum ToolchainCommand {
    /// List the compilers and tools cuno finds, and which one builds use
    List,
    /// Make a compiler this user's default
    Default {
        /// The compiler (msvc / clang / clang-cl / gcc)
        #[arg(value_name = "COMPILER")]
        compiler: String,
    },
}

/// The options of the build commands (docs/en/specs/commands.md, "Available for
/// build commands (`build` / `run` / `check` / `test`)").
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
            target: o.target,
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

/// Whether what clap makes is written with its colors on a stream, by
/// [`cuno_core::colors`] with no configured choice: clap prints before the
/// files are read. Never clap's own `Auto`, which reads `CLICOLOR_FORCE`, a
/// variable cuno does not read.
fn parser_color(given: Option<ColorChoice>, no_color: bool, terminal: bool) -> clap::ColorChoice {
    if cuno_core::colors(given, None, no_color, terminal) {
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

/// Carries out the command line `process` holds and returns the exit code
/// for the binary to end with.
pub fn run(process: Process) -> i32 {
    let Process {
        args,
        env,
        cwd,
        host,
        temp,
        exe,
        cores,
        mut stdout,
        stdout_is_terminal,
        mut stderr,
        stderr_is_terminal,
        mut stdin,
        stdin_is_terminal,
    } = process;
    // What clap prints itself is colored by cuno's rule, not clap's
    // (docs/en/specs/commands.md, "Available for all commands"): `--color`
    // as written, then NO_COLOR, then whether the output is a terminal.
    let given = color_before_parsing(&args);
    let no_color = env.get("NO_COLOR").is_some();
    let stdout_color = parser_color(given, no_color, stdout_is_terminal);
    let stderr_color = parser_color(given, no_color, stderr_is_terminal);
    // On `--version`, `--help` and usage errors, what clap made is printed
    // and its code returned: 0 for the first two and 2 for a usage error, as
    // the spec's exit codes require. What clap makes carries its styles
    // whatever color the command is given; whether they are written is
    // decided per stream, in `write_styled`.
    let cli = match Cli::command().try_get_matches_from(&args) {
        // Formatted against the command, as `Cli::parse` does, so such an
        // error would carry the usage line too.
        Ok(matches) => match Cli::from_arg_matches(&matches) {
            Ok(cli) => cli,
            Err(e) => {
                let e = e.format(&mut Cli::command());
                return print_clap(&e, stdout_color, stderr_color, &mut stdout, &mut stderr);
            }
        },
        Err(error) => {
            return print_clap(&error, stdout_color, stderr_color, &mut stdout, &mut stderr);
        }
    };
    let mut shell = Shell::new(
        stderr,
        cli.color.map(ColorChoice::from),
        &env,
        stderr_is_terminal,
    );
    let given = Values {
        env: &env,
        cwd: &cwd,
        exe: &exe,
        host,
        temp: &temp,
        cores,
    };
    let input = Input {
        stdin: stdin.as_mut(),
        is_terminal: stdin_is_terminal,
    };
    match dispatch(cli, given, &mut shell, stdout_color, &mut stdout, input) {
        Ok(code) => code,
        Err(error) => report(&mut shell, &*error),
    }
}

/// What clap made for `--help`, `--version` or a usage error, written where
/// clap writes it (stdout for the first two, stderr for an error) in that
/// stream's color, and its exit code.
fn print_clap(
    error: &clap::Error,
    stdout_color: clap::ColorChoice,
    stderr_color: clap::ColorChoice,
    stdout: &mut Box<dyn Write>,
    stderr: &mut Box<dyn Write>,
) -> i32 {
    let (out, color) = if error.use_stderr() {
        (stderr, stderr_color)
    } else {
        (stdout, stdout_color)
    };
    // A failed write (a closed stream) is ignored, as clap ignores it: the
    // exit code stays clap's.
    let _ = write_styled(out.as_mut(), &error.render(), color);
    error.exit_code()
}

/// `text` written plain, or with its colors when `color` says so. clap's
/// own printing writes to the process's streams and exits, which a library
/// may not, so what clap makes is written here to the streams given.
fn write_styled(
    out: &mut dyn Write,
    text: &clap::builder::StyledStr,
    color: clap::ColorChoice,
) -> std::io::Result<()> {
    match color {
        clap::ColorChoice::Always => write!(out, "{}", text.ansi())?,
        _ => write!(out, "{text}")?,
    }
    out.flush()
}

/// The values the commands take from [`Process`], borrowed once the
/// standard streams have gone to the shell and to the usage.
#[derive(Clone, Copy)]
struct Values<'a> {
    env: &'a Env,
    cwd: &'a Result<PathBuf, String>,
    host: HostOs,
    temp: &'a Path,
    exe: &'a Result<PathBuf, String>,
    cores: NonZeroU32,
}

impl Values<'_> {
    /// The current directory, or the error a command reports without it.
    fn cwd(&self) -> Result<PathBuf, Box<dyn CunoError>> {
        self.cwd
            .clone()
            .map_err(|e| Box::new(CwdError(e)) as Box<dyn CunoError>)
    }
}

/// The settings, with `[term] color` applied to `shell` before the warnings
/// they raised are printed, so every command colors as configured
/// (`docs/en/specs/commands.md`, "Available for all commands"). An error
/// comes back before any configuration file is applied, so it is shown as
/// `--color` and `NO_COLOR` alone decide.
fn resolve_settings(inputs: Inputs<'_>, shell: &mut Shell) -> Result<Settings, ConfigError> {
    let loaded = cuno_manifest::resolve(inputs)?;
    shell.configure(loaded.value.color, loaded.value.verbose);
    for warning in &loaded.warnings {
        shell.warning(warning);
    }
    Ok(loaded.value)
}

/// `cuno new`'s settings: the user's `[term]` alone, applied to `shell`,
/// since the project around the new package is not its own
/// (`docs/en/specs/commands.md`, "Available for all commands").
fn resolve_user_settings(env: &Env, cwd: &Path, shell: &mut Shell) -> Result<(), ConfigError> {
    let loaded = cuno_manifest::resolve_user(env, cwd)?;
    shell.configure(loaded.value.color, loaded.value.verbose);
    for warning in &loaded.warnings {
        shell.warning(warning);
    }
    Ok(())
}

/// Carries out the parsed command line and returns the exit code. Every
/// error a command can meet comes back here rather than being printed where
/// it happens; `doctor` prints its problems itself and only says how many.
/// `help_color` is how the usage printed with no command is colored.
fn dispatch(
    cli: Cli,
    given: Values<'_>,
    shell: &mut Shell,
    help_color: clap::ColorChoice,
    stdout: &mut Box<dyn Write>,
    input: Input<'_>,
) -> Result<i32, Box<dyn CunoError>> {
    let env = given.env;
    // Destructured rather than ignored, so that adding a field to `Cli`
    // fails to compile here until it is handled.
    let Cli {
        color: _,
        verbose,
        quiet,
        command,
    } = cli;

    // Before anything is written, so no status line escapes `-q`.
    shell.set_verbosity(if quiet {
        Verbosity::Quiet
    } else if verbose {
        Verbosity::Verbose
    } else {
        Verbosity::Normal
    });

    #[cfg(debug_assertions)]
    test_trigger::fire(env)?;

    let (goal, opts) = match command {
        Some(Command::Build(opts)) => (build::Goal::Build, opts),
        Some(Command::Check {
            tests,
            all_targets,
            opts,
        }) => (
            // `--all-targets` wins over `--tests` (commands.md, "`cuno
            // build` / `cuno check`").
            build::Goal::Check {
                programs: !tests || all_targets,
                tests: tests || all_targets,
            },
            opts,
        ),
        Some(Command::Run { bin, opts, args }) => (build::Goal::Run { bin, args }, opts),
        Some(Command::Test {
            filter,
            nocapture,
            opts,
        }) => (build::Goal::Test { filter, nocapture }, opts),
        Some(Command::Clean {
            release,
            profile,
            target_dir,
        }) => {
            let cwd = given.cwd()?;
            let cli = CliSettings {
                target_dir,
                ..CliSettings::default()
            };
            let resolved = resolve_settings(
                Inputs {
                    cwd: &cwd,
                    env,
                    cores: given.cores,
                    cli: &cli,
                },
                shell,
            );
            clean::clean(
                clean::CleanArgs { release, profile },
                resolved,
                &cwd,
                given.host,
                shell,
            )?;
            return Ok(0);
        }
        Some(Command::New {
            path,
            lang,
            edition,
            name,
            vcs,
        }) => {
            let cwd = given.cwd()?;
            resolve_user_settings(env, &cwd, shell).map_err(build::boxed)?;
            // clap admits only these values.
            let args = new::NewArgs {
                path,
                lang: lang
                    .map(|l| cuno_toolchain::flags::Lang::parse(&l).expect("a lang clap admits")),
                edition: edition.map(|e| {
                    cuno_toolchain::flags::Edition::parse(&e).expect("an edition clap admits")
                }),
                name,
                vcs: if vcs == "none" {
                    new::Vcs::None
                } else {
                    new::Vcs::Git
                },
            };
            new::new(args, &cwd, shell).map_err(|e| Box::new(e) as Box<dyn CunoError>)?;
            return Ok(0);
        }
        Some(Command::Init {
            path,
            lang,
            edition,
            name,
            vcs,
        }) => {
            let cwd = given.cwd()?;
            resolve_user_settings(env, &cwd, shell).map_err(build::boxed)?;
            // clap admits only these values.
            let args = new::InitArgs {
                path: path.unwrap_or_else(|| PathBuf::from(".")),
                lang: lang
                    .map(|l| cuno_toolchain::flags::Lang::parse(&l).expect("a lang clap admits")),
                edition: edition.map(|e| {
                    cuno_toolchain::flags::Edition::parse(&e).expect("an edition clap admits")
                }),
                name,
                vcs: if vcs == "none" {
                    new::Vcs::None
                } else {
                    new::Vcs::Git
                },
            };
            new::init(args, &cwd, given.host, shell)?;
            return Ok(0);
        }
        Some(Command::Fmt { check }) => {
            let cwd = given.cwd()?;
            let resolved = resolve_settings(
                Inputs {
                    cwd: &cwd,
                    env,
                    cores: given.cores,
                    cli: &CliSettings::default(),
                },
                shell,
            );
            return fmt::fmt(check, resolved, env, &cwd, given.host, given.temp, shell);
        }
        Some(Command::Lint { fix, compiler }) => {
            let cwd = given.cwd()?;
            let cli = CliSettings {
                compiler,
                ..CliSettings::default()
            };
            let resolved = resolve_settings(
                Inputs {
                    cwd: &cwd,
                    env,
                    cores: given.cores,
                    cli: &cli,
                },
                shell,
            );
            return lint::lint(fix, resolved, env, &cwd, given.host, given.temp, shell);
        }
        Some(Command::Doctor { compiler }) => {
            let cwd = given.cwd()?;
            let cli = CliSettings {
                compiler: compiler.clone(),
                ..CliSettings::default()
            };
            let resolved = resolve_settings(
                Inputs {
                    cwd: &cwd,
                    env,
                    cores: given.cores,
                    cli: &cli,
                },
                shell,
            );
            let problems =
                doctor::doctor(compiler, resolved, env, &cwd, given.host, given.temp, shell);
            return Ok(if problems == 0 { 0 } else { 1 });
        }
        Some(Command::Toolchain { command }) => {
            let cwd = given.cwd()?;
            let settings = resolve_settings(
                Inputs {
                    cwd: &cwd,
                    env,
                    cores: given.cores,
                    cli: &CliSettings::default(),
                },
                shell,
            )
            .map_err(build::boxed)?;
            return match command {
                ToolchainCommand::List => {
                    toolchain::list(&settings, env, given.host, stdout.as_mut(), shell)
                }
                ToolchainCommand::Default { compiler } => {
                    toolchain::default(&compiler, &settings, env, given.host, shell)
                }
            };
        }
        Some(Command::SelfCmd { command }) => {
            // Every layer, as for any command but `cuno new`
            // (docs/en/specs/commands.md, "Available for all commands"):
            // a broken configuration file stops it like any other.
            let cwd = given.cwd()?;
            resolve_settings(
                Inputs {
                    cwd: &cwd,
                    env,
                    cores: given.cores,
                    cli: &CliSettings::default(),
                },
                shell,
            )
            .map_err(build::boxed)?;
            return match command {
                SelfCommand::Update => self_cmd::update(given.exe, env, given.temp, shell),
                SelfCommand::Uninstall { yes } => {
                    self_cmd::uninstall(given.exe, env, yes, input, shell)
                }
            };
        }
        None => {
            // No command was given. Like Cargo, print the usage to stdout and
            // succeed; see docs/en/specs/commands.md, "Without a command
            // (`--version` / `--help` / no arguments)". A failed write (a
            // closed stdout) is ignored, matching what clap does for `--help`
            // and what Cargo does in both cases: the exit code stays 0.
            let usage = Cli::command().render_help();
            let _ = write_styled(stdout.as_mut(), &usage, help_color);
            return Ok(0);
        }
    };
    let cwd = given.cwd()?;
    let cli = CliSettings {
        compiler: opts.compiler.clone(),
        target_dir: opts.target_dir.clone(),
        jobs: opts.jobs,
    };
    let resolved = resolve_settings(
        Inputs {
            cwd: &cwd,
            env,
            cores: given.cores,
            cli: &cli,
        },
        shell,
    );
    build::build(
        goal,
        opts.into(),
        resolved,
        env,
        &cwd,
        given.host,
        given.temp,
        shell,
    )
}

/// Standard input, for the one command that asks.
pub(crate) struct Input<'a> {
    pub(crate) stdin: &'a mut dyn BufRead,
    pub(crate) is_terminal: bool,
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
    use cuno_core::NameCase;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// `--profile`'s help, which clap takes from a doc comment and cannot
    /// ask `Profile` for, names every profile `Profile` has, for every
    /// command that takes it.
    ///
    /// Mutation: add a profile to `Profile` without naming it in the help,
    /// or drop `release` from one command's help; this test fails.
    #[test]
    fn the_profile_help_names_every_profile() {
        let mut cli = Cli::command();
        for command in ["build", "check", "run", "test", "clean"] {
            let sub = cli.find_subcommand_mut(command).expect(command);
            let help = sub
                .get_arguments()
                .find(|a| a.get_id() == "profile")
                .and_then(|a| a.get_help())
                .map(|h| h.to_string())
                .expect(command);
            for (_, name) in cuno_manifest::Profile::ALL {
                assert!(help.contains(name), "{command}: {help}");
            }
        }
    }

    /// A stream the test reads back after `run` has written to it. With
    /// `buffered`, what is written reaches it only when flushed, as through
    /// a buffered writer.
    #[derive(Clone, Default)]
    struct Captured {
        text: Rc<RefCell<Vec<u8>>>,
        pending: Rc<RefCell<Vec<u8>>>,
        buffered: bool,
    }

    impl Write for Captured {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let to = if self.buffered {
                &self.pending
            } else {
                &self.text
            };
            to.borrow_mut().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            let pending = std::mem::take(&mut *self.pending.borrow_mut());
            self.text.borrow_mut().extend(pending);
            Ok(())
        }
    }

    impl Captured {
        fn text(&self) -> String {
            String::from_utf8(self.text.borrow().clone()).unwrap()
        }
    }

    /// How a run is set up: whether each stream is a terminal, the current
    /// directory, and whether stdout buffers.
    struct Setup {
        stdout_is_terminal: bool,
        stderr_is_terminal: bool,
        cwd: Result<PathBuf, String>,
        buffered: bool,
    }

    /// `cuno <args>` run with `setup`, in a directory holding no project and
    /// a cuno home of its own: the exit code, stdout and stderr.
    fn run_with(args: &[&str], setup: Setup) -> (i32, String, String) {
        let dir = tempfile::tempdir().unwrap();
        let env = Env::from_pairs(
            [("CUNO_HOME".into(), dir.path().join("home").into_os_string())],
            NameCase::Insensitive,
        );
        let stdout = Captured {
            buffered: setup.buffered,
            ..Captured::default()
        };
        let stderr = Captured::default();
        let code = run(Process {
            args: std::iter::once("cuno")
                .chain(args.iter().copied())
                .map(OsString::from)
                .collect(),
            env,
            cwd: setup.cwd.map(|_| dir.path().to_owned()),
            host: HostOs::Windows,
            temp: dir.path().to_owned(),
            exe: Err("not needed".to_owned()),
            cores: NonZeroU32::new(2).unwrap(),
            stdout: Box::new(stdout.clone()),
            stdout_is_terminal: setup.stdout_is_terminal,
            stderr: Box::new(stderr.clone()),
            stderr_is_terminal: setup.stderr_is_terminal,
            stdin: Box::new(std::io::empty()),
            stdin_is_terminal: false,
        });
        (code, stdout.text(), stderr.text())
    }

    fn terminals(stdout_is_terminal: bool, stderr_is_terminal: bool) -> Setup {
        Setup {
            stdout_is_terminal,
            stderr_is_terminal,
            cwd: Ok(PathBuf::new()),
            buffered: false,
        }
    }

    const ESC: char = '\u{1b}';

    /// Each stream is colored by whether it is the terminal, not the other:
    /// help and the usage on stdout, a usage error and cuno's own `error:`
    /// on stderr (`docs/en/specs/commands.md`, "Available for all
    /// commands").
    ///
    /// Mutation: decide a stream's color from the other stream's terminal,
    /// give the shell stdout's, write clap's message in the other stream's
    /// color, or color the usage with stderr's; this test fails.
    #[test]
    fn each_stream_is_colored_by_its_own_terminal() {
        for (stdout_tty, stderr_tty) in [(true, false), (false, true)] {
            let at = format!("stdout terminal {stdout_tty}, stderr terminal {stderr_tty}");
            let (code, out, _) = run_with(&["--help"], terminals(stdout_tty, stderr_tty));
            assert_eq!((code, out.contains(ESC)), (0, stdout_tty), "{at}: {out}");
            let (code, out, _) = run_with(&[], terminals(stdout_tty, stderr_tty));
            assert_eq!((code, out.contains(ESC)), (0, stdout_tty), "{at}: {out}");
            let (code, out, err) = run_with(&["bogus"], terminals(stdout_tty, stderr_tty));
            assert_eq!((code, out.as_str()), (2, ""), "{at}");
            assert_eq!(err.contains(ESC), stderr_tty, "{at}: {err}");
            let (code, _, err) = run_with(&["clean"], terminals(stdout_tty, stderr_tty));
            assert_eq!(code, 3, "{at}: {err}");
            assert_eq!(err.contains(ESC), stderr_tty, "{at}: {err}");
        }
    }

    /// A current directory that cannot be read is reported, exit code 3, by
    /// every command that needs one, and help still works without one.
    ///
    /// Mutation: run a command somewhere else instead, change the message
    /// or its exit class, or read the directory before `--help`; this test
    /// fails.
    #[test]
    fn an_unreadable_current_directory_is_reported_by_each_command() {
        let unreadable = || Setup {
            cwd: Err("gone".to_owned()),
            ..terminals(false, false)
        };
        for args in [
            &["build"][..],
            &["check"],
            &["run"],
            &["test"],
            &["clean"],
            &["doctor"],
            &["new", "app"],
        ] {
            let (code, _, err) = run_with(args, unreadable());
            assert_eq!(code, 3, "{args:?}: {err}");
            assert_eq!(
                err, "error: cannot read the current directory: gone\n",
                "{args:?}"
            );
        }
        let (code, out, _) = run_with(&["--help"], unreadable());
        assert_eq!(code, 0);
        assert!(out.starts_with("Everything C/C++"), "{out}");
    }

    /// What clap makes is flushed, so it is not lost in a buffered stream
    /// when the process ends.
    ///
    /// Mutation: leave out the flush in `write_styled`; this test fails.
    #[test]
    fn help_is_flushed() {
        let (code, out, _) = run_with(
            &["--help"],
            Setup {
                buffered: true,
                ..terminals(false, false)
            },
        );
        assert_eq!(code, 0);
        assert!(out.starts_with("Everything C/C++"), "{out:?}");
    }

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
