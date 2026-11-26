//! `cuno build`, `check` and `run`: resolve the settings, ready the
//! toolchain, then hand the build to `cuno_build::pipeline`; `run` then runs
//! the program. See `docs/en/specs/commands.md`, "`cuno build` / `cuno
//! check`".

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Instant;

use cuno_build::exec::SystemSpawner;
use cuno_build::pipeline::{self, Request, Toolchain};
use cuno_build::{bundled, toolchain};
use cuno_core::{CunoError, Env, ExitClass, Shell};
use cuno_manifest::{ConfigError, Profile, Settings};
use cuno_toolchain::msvc::SystemRegistry;
use cuno_toolchain::{HostOs, SystemRunner};

/// The options of `cuno build` (`commands.md`, "Available for build commands
/// (`build` / `run` / `check` / `test`)").
#[derive(Debug, Default)]
pub struct BuildArgs {
    /// `--release`.
    pub release: bool,
    /// `--profile`.
    pub profile: Option<String>,
    /// `--target`, reserved.
    pub target: Option<String>,
}

/// The settings of a command that needs a project: build, check, run, test
/// and clean. Outside a project the missing project is the error, whatever
/// the settings say, so every such command says the same thing there
/// (`docs/en/specs/commands.md`, "`cuno build` / `cuno check`").
pub(crate) fn in_project(
    resolved: Result<Settings, ConfigError>,
    cwd: &Path,
) -> Result<Settings, Box<dyn CunoError>> {
    if cuno_manifest::find_project(cwd).is_none() {
        return Err(Box::new(CommandError::NoProject {
            cwd: cwd.to_owned(),
        }));
    }
    resolved.map_err(boxed)
}

/// What a build command does once the project is built.
#[derive(Debug)]
pub enum Goal {
    /// `cuno build`: compile and link.
    Build,
    /// `cuno check`: check the program's sources, the test build's, or both
    /// (`commands.md`, "`cuno build` / `cuno check`").
    Check {
        /// The program's sources.
        programs: bool,
        /// The test build's.
        tests: bool,
    },
    /// `cuno run`: build, then run the executable `bin` names, or the only
    /// one, with `args`.
    Run {
        /// `--bin`.
        bin: Option<String>,
        /// What follows `--`.
        args: Vec<OsString>,
    },
    /// `cuno test`: build the test executables, then run their tests
    /// (`commands.md`, "`cuno test [filter]`").
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
/// an error, which exits 1 too. `settings` were resolved before dispatch;
/// their error is reported after the command's own argument errors.
// Each argument answers its own question; a struct would only rename them.
#[allow(clippy::too_many_arguments)]
pub fn build(
    goal: Goal,
    args: BuildArgs,
    settings: Result<Settings, ConfigError>,
    env: &Env,
    cwd: &Path,
    host: HostOs,
    temp: &Path,
    shell: &mut Shell,
) -> Result<i32, Box<dyn CunoError>> {
    let started = Instant::now();
    if args.target.is_some() {
        return Err(Box::new(CommandError::TargetNotSupported));
    }
    let profile = profile_named(args.release, args.profile.as_deref())?;
    let settings = in_project(settings, cwd)?;
    let (project, manifest) = settings
        .project
        .as_ref()
        .expect("in_project found the project the settings were read from");
    let root = project.root.as_path();

    // The compiler, its environment, and its pairing with the standard
    // library, checked before anything is built (toolchain.md).
    let runner = SystemRunner::new();
    let toolchain::Ready { selected, prepared } = toolchain::ready(
        toolchain::ReadyFor::Build,
        &settings,
        (root, manifest),
        profile,
        env,
        host,
        temp,
        &runner,
        &SystemRegistry,
        shell,
    )?;
    let version = selected.version.to_string();
    let identity: Vec<OsString> = prepared
        .identity(&version)
        .into_iter()
        .map(OsStr::to_owned)
        .collect();
    let jobs = settings.jobs.get() as usize;
    // `goal` is the command's; the pipeline gets its own, and the command's
    // is still read after the build, for what `cuno run` and `cuno test` do
    // with what was built.
    let (pipeline_goal, run) = match &goal {
        Goal::Build => (pipeline::Goal::Build, None),
        Goal::Check { programs, tests } => (
            pipeline::Goal::Check {
                programs: *programs,
                tests: *tests,
            },
            None,
        ),
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
    // Beside compile_commands.json, so editors format and analyse as cuno
    // does (dev-tools.md, "Configuration"); a file that cannot be written
    // or is the user's, only costs the editor.
    for warning in cuno_tools::write_editor_files(root, manifest) {
        shell.warning(warning);
    }
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
        let scratch = cuno_build::layout::Layout::new(&settings.target_dir, profile, root, host)
            .for_tests()
            .test_dir();
        let summary = cuno_test::run(
            &cuno_test::TestRun {
                executables: &executables,
                filter: filter.as_deref(),
                nocapture: *nocapture,
                jobs,
                root,
                cwd,
                // The environment cuno was given, as `cuno run` passes it.
                env,
                scratch: &scratch,
                case: host.name_case(),
                launcher: &cuno_test::SystemLauncher,
            },
            shell,
        );
        // commands.md, "`cuno test [filter]`": 0 only when every test that was
        // to run ran and none failed.
        return Ok(if summary.succeeded() { 0 } else { 1 });
    }
    let (Some(exe), Some(args)) = (built.exe, run) else {
        return Ok(0);
    };
    let shown = cuno_core::program::shown(&exe, cwd);
    shell.status("Running", format!("`{}`", shown.display()));
    // The standard streams are left connected (commands.md, "`cuno run`").
    let status = cuno_core::program::command(&exe, args, env, root)
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
    let Some(name) = name else {
        return Ok(Profile::Dev);
    };
    Profile::parse(name).ok_or_else(|| {
        Box::new(CommandError::UnknownProfile {
            name: name.to_owned(),
        }) as Box<dyn CunoError>
    })
}

/// The profiles there are, as `--profile` takes them: `dev or release`.
fn profile_names() -> String {
    let names: Vec<&str> = Profile::ALL.iter().map(|(_, name)| *name).collect();
    match names.split_last() {
        Some((last, [])) => (*last).to_owned(),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
        None => String::new(),
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
            CommandError::UnknownProfile { .. } => Some(format!("use {}", profile_names())),
            CommandError::TargetNotSupported | CommandError::CannotRun { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `--profile` takes every profile `Profile` has, by the name
    /// `cuno.toml` writes it with, and no other; the help lists them all.
    ///
    /// Mutation: write a profile name in `profile_named` instead of asking
    /// `Profile`, or leave one out of `profile_names`; this test fails.
    #[test]
    fn every_profile_is_taken_by_its_name() {
        for (profile, name) in Profile::ALL {
            assert_eq!(profile_named(false, Some(name)).unwrap(), *profile);
            assert!(profile_names().contains(name), "{name}");
        }
        assert_eq!(profile_named(false, None).unwrap(), Profile::Dev);
        assert_eq!(profile_named(true, Some("dev")).unwrap(), Profile::Release);
        assert!(profile_named(false, Some("bench")).is_err());
        assert_eq!(profile_names(), "dev or release");
    }
}
