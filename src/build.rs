//! `cuno build`, `check` and `run`: resolve the settings, ready the
//! toolchain, then hand the build to `cuno_build::pipeline`; `run` then runs
//! the program. See `docs/en/specs/commands.md`, "`cuno build` / `cuno
//! check`".

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::time::Instant;

use cuno_build::exec::SystemSpawner;
use cuno_build::pipeline::{self, Request, Toolchain};
use cuno_core::{CunoError, Env, ExitClass, Shell};
use cuno_manifest::{CliSettings, Inputs, Profile};
use cuno_toolchain::{HostOs, SystemRunner};

use crate::{bundled, toolchain};

/// The options of `cuno build` (`commands.md`, "Build commands").
#[derive(Debug, Default)]
pub struct BuildArgs {
    /// `--release`.
    pub release: bool,
    /// `--profile`.
    pub profile: Option<String>,
    /// `--compiler`.
    pub compiler: Option<String>,
    /// `--target`, reserved.
    pub target: Option<String>,
    /// `--target-dir`.
    pub target_dir: Option<PathBuf>,
    /// `-j, --jobs`.
    pub jobs: Option<NonZeroU32>,
}

/// How much to show (`commands.md`, "Available for all commands").
#[derive(Clone, Copy, Debug, Default)]
pub struct Verbosity {
    /// `-v`: the command of each step.
    pub verbose: bool,
    /// `-q`: errors, warnings and the compilers' output only.
    pub quiet: bool,
}

/// What a build command does once the project is built.
#[derive(Debug)]
pub enum Goal {
    /// `cuno build`: compile and link.
    Build,
    /// `cuno check`: check every source, without objects or links.
    Check,
    /// `cuno run`: build, then run the executable `bin` names, or the only
    /// one, with `args`.
    Run {
        /// `--bin`.
        bin: Option<String>,
        /// What follows `--`.
        args: Vec<OsString>,
    },
    /// `cuno test`: build the test executables, then run their tests
    /// (`commands.md`, "`cuno test`").
    Test {
        /// The filter: run only the tests whose names contain it.
        filter: Option<String>,
        /// `--nocapture`.
        nocapture: bool,
    },
}

/// Carries out `goal` for the project around `cwd` and returns the exit
/// code: 0; for `cuno run` the program's; for `cuno test`, 1 when a test
/// failed or an executable's tests could not be listed. A failed build is
/// an error, which exits 1 too.
pub fn build(
    goal: Goal,
    args: BuildArgs,
    verbosity: Verbosity,
    env: &Env,
    cwd: &Path,
    host: HostOs,
    shell: &mut Shell,
) -> Result<i32, Box<dyn CunoError>> {
    let started = Instant::now();
    if args.target.is_some() {
        return Err(Box::new(CommandError::TargetNotSupported));
    }
    let profile = profile_named(args.release, args.profile.as_deref())?;
    let cli = CliSettings {
        compiler: args.compiler,
        target_dir: args.target_dir,
        jobs: args.jobs,
    };
    let settings = crate::settings::resolve(
        Inputs {
            cwd,
            env,
            cli: &cli,
        },
        shell,
    )
    .map_err(boxed)?;
    let verbose = verbosity.verbose || settings.verbose;
    let Some((project, manifest)) = &settings.project else {
        return Err(Box::new(CommandError::NoProject {
            cwd: cwd.to_owned(),
        }));
    };
    let root = project.root.as_path();

    // The compiler, its environment, and its pairing with the standard
    // library, checked before anything is built (toolchain.md).
    let runner = SystemRunner::new();
    let toolchain::Ready { selected, prepared } = toolchain::ready(
        &settings,
        (root, manifest),
        profile,
        env,
        host,
        &runner,
        shell,
    )?;
    let version = selected.version.to_string();
    let identity: Vec<OsString> = prepared
        .identity(&version)
        .into_iter()
        .map(OsStr::to_owned)
        .collect();
    let jobs = settings
        .jobs
        .map(|n| n.get() as usize)
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()));
    // `goal` is the command's; the pipeline gets its own, and the command's
    // is still read after the build, for what `cuno run` and `cuno test` do
    // with what was built.
    let (pipeline_goal, run) = match &goal {
        Goal::Build => (pipeline::Goal::Build, None),
        Goal::Check => (pipeline::Goal::Check, None),
        Goal::Run { bin, args } => (
            pipeline::Goal::Run {
                bin: bin.as_deref(),
            },
            Some(args),
        ),
        Goal::Test { .. } => (pipeline::Goal::Test, None),
    };
    let bundled = bundled::expand(&settings.home).map_err(boxed)?;
    let request = Request {
        root,
        manifest,
        target_dir: &settings.target_dir,
        profile,
        goal: pipeline_goal,
        host,
        bundled: &bundled,
        jobs,
        verbose,
        quiet: verbosity.quiet,
        started,
        toolchain: Toolchain {
            kind: selected.kind,
            executable: &selected.executable,
            version: &selected.version,
            msvc: selected.msvc.as_ref().map(|toolset| &toolset.cl_version),
            env: &prepared.env,
            identity,
        },
        spawner: &SystemSpawner,
    };
    let built = pipeline::build(&request, shell).map_err(boxed)?;
    if let Goal::Test { filter, nocapture } = &goal {
        let executables: Vec<cuno_test::TestExecutable> = built
            .test_executables
            .into_iter()
            .map(|test| cuno_test::TestExecutable {
                path: test.path,
                integration: test.file.is_some(),
            })
            .collect();
        // Beside the test build's own output, so `cuno clean` removes
        // whatever an interrupted run leaves; each run makes a directory of
        // its own there (`build.md`, "Output directory").
        let scratch = settings
            .target_dir
            .join(cuno_build::layout::profile_dir_name(profile))
            .join("test");
        let summary = cuno_test::run(
            &cuno_test::TestRun {
                executables: &executables,
                filter: filter.as_deref(),
                nocapture: *nocapture,
                jobs,
                root,
                cwd,
                quiet: verbosity.quiet,
                // The environment cuno was given, as `cuno run` passes it.
                env,
                scratch: &scratch,
                case: host.name_case(),
                launcher: &cuno_test::SystemLauncher,
            },
            shell,
        );
        // commands.md, "`cuno test`": 0 only when every test that was to
        // run ran and none failed.
        return Ok(if summary.succeeded() { 0 } else { 1 });
    }
    let (Some(exe), Some(args)) = (built.exe, run) else {
        return Ok(0);
    };
    if !verbosity.quiet {
        let shown = exe.strip_prefix(cwd).unwrap_or(&exe);
        shell.status("Running", format!("`{}`", shown.display()));
    }
    // The environment cuno was given, not the compiler's; the standard
    // streams are left connected (commands.md, "`cuno run`").
    let status = std::process::Command::new(&exe)
        .args(args)
        .env_clear()
        .envs(env.vars())
        .current_dir(root)
        .status()
        .map_err(|e| {
            Box::new(CommandError::CannotRun {
                exe: exe.clone(),
                reason: e.to_string(),
            }) as Box<dyn CunoError>
        })?;
    // `None` only for a signal, which Windows does not have; the exit code
    // for one is to be decided with the hosts that do (roadmap M8).
    Ok(status.code().unwrap_or(1))
}

/// The profile `--release` or `--profile` names, `dev` by default; only the
/// two built in are accepted (`build.md`, "Profiles").
pub(crate) fn profile_named(
    release: bool,
    name: Option<&str>,
) -> Result<Profile, Box<dyn CunoError>> {
    if release {
        return Ok(Profile::Release);
    }
    match name {
        None | Some("dev") => Ok(Profile::Dev),
        Some("release") => Ok(Profile::Release),
        Some(other) => Err(Box::new(CommandError::UnknownProfile {
            name: other.to_owned(),
        })),
    }
}

pub(crate) fn boxed<E: CunoError + 'static>(error: E) -> Box<dyn CunoError> {
    Box::new(error)
}

/// What stops a build command, beyond the errors of the crates it calls.
#[derive(Debug)]
pub enum CommandError {
    /// Run outside a project.
    NoProject {
        /// Where it was run.
        cwd: PathBuf,
    },
    /// `--profile` names a profile cuno does not have.
    UnknownProfile {
        /// The name given.
        name: String,
    },
    /// `--target`, reserved for cross-compiling.
    TargetNotSupported,
    /// The built executable could not be started.
    CannotRun {
        /// The executable.
        exe: PathBuf,
        /// Why.
        reason: String,
    },
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommandError::NoProject { cwd } => write!(
                f,
                "could not find cuno.toml in {} or any parent directory",
                cwd.display()
            ),
            CommandError::UnknownProfile { name } => write!(f, "no profile named `{name}`"),
            CommandError::TargetNotSupported => f.write_str("--target is not supported yet"),
            CommandError::CannotRun { exe, reason } => {
                write!(f, "could not run {}: {reason}", exe.display())
            }
        }
    }
}

impl std::error::Error for CommandError {}

impl CunoError for CommandError {
    fn exit_class(&self) -> ExitClass {
        match self {
            // The program cuno built would not start: a problem with what
            // was built, not with the setup (commands.md, "Exit codes").
            CommandError::CannotRun { .. } => ExitClass::Failure,
            CommandError::NoProject { .. }
            | CommandError::UnknownProfile { .. }
            | CommandError::TargetNotSupported => ExitClass::Setup,
        }
    }

    fn help(&self) -> Option<String> {
        match self {
            CommandError::NoProject { .. } => {
                Some("run it in a directory that holds a cuno.toml, or below one".to_owned())
            }
            CommandError::UnknownProfile { .. } => Some("use dev or release".to_owned()),
            CommandError::TargetNotSupported | CommandError::CannotRun { .. } => None,
        }
    }
}
