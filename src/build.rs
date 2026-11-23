//! `cuno build`: resolve the settings, prepare the toolchain, classify the
//! sources, plan the graph and run it. See `docs/en/specs/commands.md`,
//! "`cuno build` / `cuno check`".

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::time::Instant;

use cuno_build::compdb;
use cuno_build::exec::{self, Event, SystemSpawner};
use cuno_build::graph::{self, Plan, Step, StepKind};
use cuno_build::input::compile_settings;
use cuno_build::layout::{self, Layout};
use cuno_build::sources::{self, Bin};
use cuno_build::state::Records;
use cuno_core::{CunoError, Env, ExitClass, Shell};
use cuno_manifest::{CliSettings, Inputs, Lang, OptLevel, Profile};
use cuno_toolchain::flags::Mode;
use cuno_toolchain::{HostOs, SystemRunner};

use crate::toolchain;

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
}

/// Carries out `goal` for the project around `cwd` and returns the exit
/// code: 0, or for `cuno run` the program's.
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
    let compile = |lang: Lang, mode: Mode| {
        compile_settings(
            Some((root, manifest)),
            selected.kind,
            profile,
            lang,
            host,
            false,
            mode,
        )
    };

    let sources = sources::classify(root, manifest, &settings.target_dir, host).map_err(boxed)?;
    for warning in &sources.warnings {
        shell.warning(warning);
    }
    let layout = Layout::new(&settings.target_dir, profile, root, host);
    // Chosen before anything is built, so a wrong `--bin` costs nothing.
    let (mode, run) = match &goal {
        Goal::Build => (Mode::Compile, None),
        Goal::Check => (Mode::Check, None),
        Goal::Run { bin, args } => {
            let exe = layout.exe(&choose(&sources.bins, bin.as_deref()).map_err(boxed)?.name);
            (Mode::Compile, Some((exe, args)))
        }
    };
    let verb = match mode {
        Mode::Compile => "Compiling",
        Mode::Check => "Checking",
    };
    let link = compile(manifest.package.lang, Mode::Compile);
    let compile_for = |lang| compile(lang, mode);
    let package = &manifest.package.name;
    let graph = graph::plan(&Plan {
        package,
        sources: &sources,
        layout: &layout,
        kind: selected.kind,
        program: &selected.executable,
        mode,
        compile: &compile_for,
        link: (&link.build, &link.profile),
    });

    // Written before anything runs, so it is current even when a compile
    // fails, and always from the `dev` profile with `CUNO_TEST` defined,
    // whatever is built (build.md, "Output").
    let for_editors = |lang| {
        compile_settings(
            Some((root, manifest)),
            selected.kind,
            Profile::Dev,
            lang,
            host,
            true,
            Mode::Compile,
        )
    };
    let editors = graph::plan(&Plan {
        package,
        sources: &sources,
        layout: &Layout::new(&settings.target_dir, Profile::Dev, root, host),
        kind: selected.kind,
        program: &selected.executable,
        mode: Mode::Compile,
        compile: &for_editors,
        link: (&link.build, &link.profile),
    });
    let include = compdb::include_dirs(prepared.env.get("INCLUDE"));
    let compiler = compdb::Compiler {
        kind: selected.kind,
        include: &include,
        msvc: selected.msvc.as_ref().map(|toolset| &toolset.cl_version),
    };
    if let Err(e) = compdb::write(root, &editors, compiler) {
        shell.warning(format!("could not write {}: {e}", compdb::FILE));
    }

    let version = selected.version.to_string();
    // So that `cuno clean` knows the output directory is cuno's; one it
    // cannot mark is only one `cuno clean` will refuse.
    let _ = layout::claim(&settings.target_dir);
    let records = Records::new(prepared.identity(&version), &include, host.name_case());
    // Printed when the first step starts, so a build with nothing to do
    // shows only its Finished line (commands.md, "Output").
    let mut first_status = Some(format!(
        "{package} v{} ({} {})",
        manifest.package.version,
        selected.kind.name(),
        selected.version
    ));
    let jobs = settings
        .jobs
        .map(|n| n.get() as usize)
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()));
    let result = exec::execute(
        &graph,
        &prepared.env,
        root,
        jobs,
        &SystemSpawner,
        Some(&records),
        &mut |event| match event {
            Event::Started(step) => {
                if let Some(line) = first_status.take()
                    && !verbosity.quiet
                {
                    shell.status(verb, line);
                }
                if verbose && !verbosity.quiet {
                    shell.status("Running", format!("`{}`", command_line(step)));
                }
            }
            Event::Finished(_, output) => {
                shell.raw(&output.stdout);
                shell.raw(&output.stderr);
            }
        },
    );
    if let Err(failed) = result {
        let link = failed
            .0
            .first()
            .is_some_and(|&i| matches!(graph.steps[i].kind, StepKind::Link { .. }));
        return Err(Box::new(CommandError::Failed {
            package: package.clone(),
            link,
        }));
    }

    if !verbosity.quiet {
        let settings = manifest.profile_settings(profile);
        let optimized = match settings.opt_level {
            OptLevel::O0 => "unoptimized",
            OptLevel::O1 | OptLevel::O2 | OptLevel::O3 | OptLevel::Size => "optimized",
        };
        let debuginfo = if settings.debug_info {
            " + debuginfo"
        } else {
            ""
        };
        shell.status(
            "Finished",
            format!(
                "`{}` profile [{optimized}{debuginfo}] in {:.2}s",
                profile_name(profile),
                started.elapsed().as_secs_f64()
            ),
        );
    }
    let Some((exe, args)) = run else {
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

/// The executable `cuno run` runs: the one `bin` names, or the only one.
fn choose<'a>(bins: &'a [Bin], bin: Option<&str>) -> Result<&'a Bin, CommandError> {
    let available = || {
        bins.iter()
            .map(|b| b.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    match (bin, bins) {
        (Some(name), _) => {
            bins.iter()
                .find(|b| b.name == name)
                .ok_or_else(|| CommandError::NoSuchBin {
                    name: name.to_owned(),
                    available: available(),
                })
        }
        (None, [only]) => Ok(only),
        (None, [_, _, ..]) => Err(CommandError::SeveralBins {
            available: available(),
        }),
        // Classifying refuses a package with no executable before this.
        (None, []) => unreachable!("a package to run has an executable"),
    }
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

/// The profile's name as `cuno.toml` writes it.
fn profile_name(profile: Profile) -> &'static str {
    match profile {
        Profile::Dev => "dev",
        Profile::Release => "release",
    }
}

/// A step's command as `-v` shows it: the program and its arguments,
/// each quoted when it holds a space.
fn command_line(step: &Step) -> String {
    // Quoted as the Windows command line reads it back: an argument with a
    // space, a tab or a quote, or an empty one, in quotes, with each quote
    // escaped and the backslashes before it doubled.
    let quote = |s: &OsStr| {
        let s = s.to_string_lossy();
        if !s.is_empty() && !s.contains([' ', '\t', '"']) {
            return s.into_owned();
        }
        let mut quoted = String::from('"');
        let mut backslashes = 0;
        for c in s.chars() {
            match c {
                '\\' => backslashes += 1,
                '"' => {
                    quoted.push_str(&"\\".repeat(backslashes * 2 + 1));
                    backslashes = 0;
                }
                _ => {
                    quoted.push_str(&"\\".repeat(backslashes));
                    backslashes = 0;
                }
            }
            if c != '\\' {
                quoted.push(c);
            }
        }
        quoted.push_str(&"\\".repeat(backslashes * 2));
        quoted.push('"');
        quoted
    };
    std::iter::once(quote(step.program.as_os_str()))
        .chain(step.args.iter().map(|a| quote(a)))
        .collect::<Vec<_>>()
        .join(" ")
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
    /// `cuno run` without `--bin`, and more than one executable.
    SeveralBins {
        /// The executables' names, comma separated.
        available: String,
    },
    /// `--bin` names no executable.
    NoSuchBin {
        /// The name given.
        name: String,
        /// The executables' names, comma separated.
        available: String,
    },
    /// The built executable could not be started.
    CannotRun {
        /// The executable.
        exe: PathBuf,
        /// Why.
        reason: String,
    },
    /// A compile or a link failed; the compiler said why.
    Failed {
        /// The package.
        package: String,
        /// Whether the first failure was a link.
        link: bool,
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
            CommandError::SeveralBins { .. } => {
                f.write_str("several executables can be run; choose one with --bin")
            }
            CommandError::NoSuchBin { name, .. } => write!(f, "no executable named `{name}`"),
            CommandError::CannotRun { exe, reason } => {
                write!(f, "could not run {}: {reason}", exe.display())
            }
            CommandError::Failed { package, link } => {
                let what = if *link { "link" } else { "compile" };
                write!(f, "could not {what} `{package}`")
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
            CommandError::Failed { .. } | CommandError::CannotRun { .. } => ExitClass::Failure,
            CommandError::SeveralBins { .. } | CommandError::NoSuchBin { .. } => ExitClass::Usage,
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
            CommandError::SeveralBins { available } | CommandError::NoSuchBin { available, .. } => {
                Some(format!("available: {available}"))
            }
            CommandError::TargetNotSupported
            | CommandError::CannotRun { .. }
            | CommandError::Failed { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn shown(program: &str, args: &[&str]) -> String {
        command_line(&Step {
            kind: StepKind::Link { bin: "app".into() },
            program: program.into(),
            args: args.iter().map(OsString::from).collect(),
            output: PathBuf::new(),
            after: Vec::new(),
            echo: None,
            headers: None,
            reads: Vec::new(),
            searched: Vec::new(),
            looked_for: Vec::new(),
        })
    }

    /// `-v` shows each argument as the Windows command line would read it
    /// back: plain when it can be, otherwise quoted with its quotes escaped
    /// and the backslashes before a quote doubled.
    ///
    /// Mutation: quote only on a space, leave inner quotes as they are, or
    /// drop an empty argument; this test fails.
    #[test]
    fn verbose_quotes_as_the_command_line_reads() {
        assert_eq!(shown("cl.exe", &["/c", "a.cpp"]), "cl.exe /c a.cpp");
        assert_eq!(
            shown(r"C:\Program Files\cl.exe", &[r#"/DGREETING="hi there""#]),
            r#""C:\Program Files\cl.exe" "/DGREETING=\"hi there\"""#
        );
        assert_eq!(shown("x", &[r"C:\a b\"]), r#"x "C:\a b\\""#);
        assert_eq!(shown("x", &["a\tb", ""]), "x \"a\tb\" \"\"");
    }
}
