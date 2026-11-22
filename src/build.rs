//! `cuno build`: resolve the settings, prepare the toolchain, classify the
//! sources, plan the graph and run it. See `docs/en/specs/commands.md`,
//! "`cuno build` / `cuno check`".

use std::ffi::OsStr;
use std::fmt;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::time::Instant;

use cuno_build::exec::{self, Event, SystemSpawner};
use cuno_build::graph::{self, Plan, Step, StepKind};
use cuno_build::input::compile_settings;
use cuno_build::layout::Layout;
use cuno_build::sources;
use cuno_build::state::Records;
use cuno_core::{CunoError, Env, ExitClass, Shell};
use cuno_manifest::{CliSettings, Inputs, Lang, OptLevel, Profile};
use cuno_toolchain::flags::{Mode, check_edition, compile_flags};
use cuno_toolchain::msvc::SystemRegistry;
use cuno_toolchain::select::{self, Found, ProbeInputs};
use cuno_toolchain::{HostOs, SystemRunner, prepare};

use crate::scratch;

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

/// Builds the project around `cwd`.
pub fn build(
    args: BuildArgs,
    verbosity: Verbosity,
    env: &Env,
    cwd: &Path,
    host: HostOs,
    shell: &mut Shell,
) -> Result<(), Box<dyn CunoError>> {
    let started = Instant::now();
    if args.target.is_some() {
        return Err(Box::new(CommandError::TargetNotSupported));
    }
    let profile = profile(&args)?;
    let cli = CliSettings {
        compiler: args.compiler,
        target_dir: args.target_dir,
        jobs: args.jobs,
    };
    let loaded = cuno_manifest::resolve(Inputs {
        cwd,
        env,
        cli: &cli,
    })
    .map_err(boxed)?;
    for warning in &loaded.warnings {
        shell.warning(warning);
    }
    let settings = loaded.value;
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
    let detected = prepare::detect(env, host, &settings.llvm_paths(), &runner);
    let installs = detected.installs.map_err(boxed)?;
    let llvm = detected.llvm.map_err(boxed)?;
    let found = Found {
        installs: &installs,
        llvm: &llvm,
        explicit_msvc: settings.msvc_path(),
    };
    let selected =
        select::select(&settings.compiler, found, &settings.constraints, host).map_err(boxed)?;
    check_edition(selected.kind, selected.version, manifest.package.edition).map_err(boxed)?;
    let prepared = prepare::prepare(&selected, env, &SystemRegistry).map_err(boxed)?;
    // Left out of the build's environment, since they add flags no command
    // line shows (toolchain.md, "How it is resolved").
    for name in &prepared.ignored {
        shell.warning(format!("{name} is set in the environment and is not used"));
        shell.help("write the flags in cuno.toml");
    }
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
    if manifest.package.lang == Lang::Cxx {
        let flags = compile_flags(&compile(Lang::Cxx, Mode::Check).input());
        scratch::in_scratch("build", selected.kind.name(), |dir| {
            select::check(
                &selected,
                &prepared.env,
                ProbeInputs {
                    scratch: dir,
                    flags: &flags,
                    cwd: root,
                },
                &runner,
            )
        })?;
    }

    let sources = sources::classify(root, manifest, &settings.target_dir, host).map_err(boxed)?;
    for warning in &sources.warnings {
        shell.warning(warning);
    }
    let layout = Layout::new(&settings.target_dir, profile, root, host);
    let link = compile(manifest.package.lang, Mode::Compile);
    let compile_for = |lang| compile(lang, Mode::Compile);
    let package = &manifest.package.name;
    let graph = graph::plan(&Plan {
        package,
        sources: &sources,
        layout: &layout,
        kind: selected.kind,
        program: &selected.executable,
        compile: &compile_for,
        link: (&link.build, &link.profile),
    });

    // What besides each step's arguments identifies its command: the
    // compiler's version, and the variables that decide which headers and
    // libraries it reads (build.md, "Deciding what changed").
    let version = selected.version.to_string();
    let empty = OsStr::new("");
    let records = Records::new(
        std::iter::once(OsStr::new(&version)).chain(
            ["INCLUDE", "LIB", "LIBPATH"]
                .into_iter()
                .map(|name| prepared.env.get(name).unwrap_or(empty)),
        ),
    );
    // Printed when the first step starts, so a build with nothing to do
    // shows only its Finished line (commands.md, "Output").
    let mut compiling = Some(format!(
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
                if let Some(line) = compiling.take()
                    && !verbosity.quiet
                {
                    shell.status("Compiling", line);
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
    Ok(())
}

/// The profile `--release` or `--profile` names, `dev` by default; only the
/// two built in are accepted (`build.md`, "Profiles").
fn profile(args: &BuildArgs) -> Result<Profile, Box<dyn CunoError>> {
    if args.release {
        return Ok(Profile::Release);
    }
    match args.profile.as_deref() {
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

fn boxed<E: CunoError + 'static>(error: E) -> Box<dyn CunoError> {
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
            CommandError::Failed { .. } => ExitClass::Failure,
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
            CommandError::TargetNotSupported | CommandError::Failed { .. } => None,
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
