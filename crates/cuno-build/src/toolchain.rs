//! Getting a compiler ready to build with: each step once, for every
//! command that compiles. `cuno build` (and the other build commands) take
//! the steps in order through [`ready`], stopping at the first problem;
//! `cuno doctor` calls the same steps one at a time and goes on after one.
//!
//! As a library this reads nothing from the process: the temporary
//! directory, the environment and the host come in as values, and so do the
//! real registry and runner, which the caller chooses (`docs/en/specs/
//! architecture.md`, "Dependencies between crates"). See
//! `docs/en/specs/toolchain.md`.

use std::ops::ControlFlow;
use std::path::{Path, PathBuf};

use cuno_core::scratch;
use cuno_core::{CunoError, Env, Shell};
use cuno_manifest::{Lang, Manifest, Profile, Settings};
use cuno_toolchain::compiler::CompilerKind;
use cuno_toolchain::flags::{Mode, ToolContext, check_edition};
use cuno_toolchain::gcc::find_gcc;
use cuno_toolchain::llvm::LlvmTool;

use cuno_toolchain::msvc::Registry;
use cuno_toolchain::prepare::{self, Detected, Prepared};
use cuno_toolchain::probe::{self as stl, ProbeInputs};
use cuno_toolchain::select::{self, Choices, LlvmChoice, Selected};
use cuno_toolchain::{HostOs, Runner, ToolchainError};

use crate::bundled;
use crate::input::{CompileSettings, compile_settings, editor_settings};

/// The steps fail with errors of several crates, each a `CunoError`, and
/// return them as one type. The command code above this crate has a helper
/// of its own for the same; a crate cannot share one with the code that
/// depends on it.
fn boxed(e: impl CunoError + 'static) -> Box<dyn CunoError> {
    Box::new(e)
}

/// Detects Visual Studio and the LLVM tools, and GCC when `gcc` is set,
/// the copies `[toolchain.paths]` names first; none are named when the
/// settings could not be read. GCC is asked for, not always searched,
/// because finding it runs each copy (`docs/en/specs/toolchain.md`, "MinGW
/// (GCC)"): a command passes `true` only when GCC is wanted, or when it
/// shows every compiler.
pub fn detect(
    settings: Option<&Settings>,
    env: &Env,
    host: HostOs,
    runner: &dyn Runner,
    gcc: bool,
) -> Detected {
    let explicit = settings.map(Settings::llvm_paths).unwrap_or_default();
    let mut detected = prepare::detect(env, host, &explicit, runner);
    if gcc {
        detected.gcc = Some(find_gcc(
            env,
            host,
            settings.and_then(Settings::gcc_path),
            runner,
        ));
    }
    detected
}

/// The directory of cuno's bundled headers that a build puts on the
/// include path: under the settings' cuno home, or, when the settings
/// could not be read, the home the environment names. `None` without a
/// home (a problem the configuration already reported). The standard
/// library probe and clang-tidy's check both take it from here, so they
/// analyse with the same include path as each other and as the build.
pub fn bundled_include(settings: Option<&Settings>, env: &Env) -> Option<PathBuf> {
    let home = settings
        .map(|s| s.home.clone())
        .or_else(|| cuno_core::cuno_home(env).ok())?;
    Some(bundled::include_dir(&home))
}

/// What the standard library probe compiles: the build's settings for a
/// C++ input in `profile`, the resolved environment, and where the build
/// runs.
pub struct ProbeCompile {
    /// The resolved Windows environment.
    pub env: Env,
    /// The build's compile settings for a C++ input, from which the probe
    /// makes the build's own command line.
    pub settings: CompileSettings,
    /// Where the build runs: the project root, or the current directory
    /// outside a project, so a relative path in the flags means the same.
    pub cwd: PathBuf,
}

/// The probe's compile, or `None` for a C package, which uses no C++
/// standard library (`toolchain.md`, "Minimum compiler versions"). The
/// probe compiles C++, so the flags are a C++ input's, never the test
/// build's. `cwd` is where it runs only outside a project; in one, it runs
/// in the root, as the build does.
pub fn probe_compile(
    selected: &Selected,
    prepared: &Prepared,
    project: Option<(&Path, &Manifest)>,
    profile: Profile,
    cwd: &Path,
    host: HostOs,
    bundled: &Path,
) -> Option<ProbeCompile> {
    if project.is_some_and(|(_, manifest)| manifest.package.lang == Lang::C) {
        return None;
    }
    let settings = compile_settings(
        project,
        selected.kind,
        profile,
        Lang::Cxx,
        host,
        bundled,
        false,
        Mode::Check,
    );
    Some(ProbeCompile {
        env: prepared.env.clone(),
        settings,
        cwd: project.map_or_else(|| cwd.to_owned(), |(root, _)| root.to_owned()),
    })
}

/// What `cuno doctor`'s clang-tidy check analyses: the probe as
/// compile_commands.json would hold it, the settings `cuno lint` runs
/// clang-tidy with (`editor_settings`), and what clang-tidy is told of
/// the compiler (`toolchain.md`, "Minimum compiler versions").
pub struct TidyCompile {
    /// The resolved Windows environment.
    pub env: Env,
    /// compile_commands.json's settings for a C++ input.
    pub settings: CompileSettings,
    /// Where the build runs, as for [`ProbeCompile`].
    pub cwd: PathBuf,
    selected: Selected,
    include: Vec<PathBuf>,
}

impl TidyCompile {
    /// What clang-tidy is told of the compiler, as compile_commands.json
    /// tells it.
    pub fn context(&self) -> ToolContext<'_> {
        ToolContext::of(&self.selected, &self.include)
    }
}

/// The clang-tidy check's compile, or `None` for a C package, which uses no
/// C++ standard library: the same cases as [`probe_compile`], with
/// compile_commands.json's settings and the `INCLUDE` and cl version it
/// writes.
pub fn tidy_compile(
    selected: &Selected,
    prepared: &Prepared,
    project: Option<(&Path, &Manifest)>,
    cwd: &Path,
    host: HostOs,
    bundled: &Path,
) -> Option<TidyCompile> {
    // The check asks the MSVC standard library, which GCC does not use.
    if project.is_some_and(|(_, manifest)| manifest.package.lang == Lang::C)
        || !selected.kind.uses_msvc_stl()
    {
        return None;
    }
    Some(TidyCompile {
        env: prepared.env.clone(),
        settings: editor_settings(project, selected.kind, Lang::Cxx, host, bundled),
        cwd: project.map_or_else(|| cwd.to_owned(), |(root, _)| root.to_owned()),
        selected: selected.clone(),
        include: prepared.include_dirs(),
    })
}

/// Whether `chosen`, the clang-tidy selection picked, can analyse this
/// package: its standard library check on `tidy`'s compile, in a scratch
/// directory under `temp` named for `command` (`doctor`, `lint`).
/// `cuno doctor` reports the error and `cuno lint` stops on it
/// (`docs/en/specs/toolchain.md`, "Minimum compiler versions").
pub fn tidy_ready(
    chosen: &LlvmChoice,
    tidy: &TidyCompile,
    temp: &Path,
    command: &str,
    runner: &dyn Runner,
) -> Result<(), Box<dyn CunoError>> {
    in_scratch(temp, command, "clang-tidy", |scratch| {
        stl::check_tidy(
            chosen,
            &tidy.env,
            tidy.context(),
            ProbeInputs {
                scratch,
                input: &tidy.settings.input(),
                cwd: &tidy.cwd,
            },
            runner,
        )
    })
}

/// Runs the standard library probe of `selected`, in a scratch directory
/// under `temp`, the temporary directory, whose name carries `command`
/// (`build`, `doctor`). Ok, with nothing made, for a compiler it does not
/// apply to (`cuno_toolchain::probe::applies` decides), so a caller need not
/// ask first; doctor still does, to print `Accepted` only for one it checked.
pub fn probe(
    selected: &Selected,
    compile: &ProbeCompile,
    temp: &Path,
    command: &str,
    runner: &dyn Runner,
) -> Result<(), Box<dyn CunoError>> {
    // Asked before the scratch directory is made: MSVC is not checked, so
    // an unusable `TEMP` must not stop its build.
    if !stl::applies(selected) {
        return Ok(());
    }
    in_scratch(temp, command, selected.kind.name(), |dir| {
        stl::check(
            selected,
            &compile.env,
            ProbeInputs {
                scratch: dir,
                input: &compile.settings.input(),
                cwd: &compile.cwd,
            },
            runner,
        )
    })
}

/// A compiler ready to build with.
pub struct Ready {
    /// The compiler.
    pub selected: Selected,
    /// Its environment.
    pub prepared: Prepared,
    /// Every LLVM tool detection found, for clang-format and clang-tidy.
    pub llvm: Vec<LlvmTool>,
}

/// What a toolchain is made ready for: it names the scratch directories
/// (`cuno-build-…`, `cuno-lint-…`), decides whether GCC is searched for
/// when it is not wanted, and only a build runs the compiler, which needs
/// the temporary directory itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadyFor {
    /// `cuno build`, `run`, `check` and `test`: the compiler runs.
    Build,
    /// `cuno lint`: only clang-tidy runs.
    Lint,
    /// `cuno doctor`: every compiler is looked for, for its `Info` line.
    Doctor,
}

impl ReadyFor {
    /// The command, as scratch directories are named.
    fn command(self) -> &'static str {
        match self {
            ReadyFor::Build => "build",
            ReadyFor::Lint => "lint",
            ReadyFor::Doctor => "doctor",
        }
    }

    /// Whether every compiler is searched for, not only the one wanted
    /// (`docs/en/specs/toolchain.md`, "MinGW (GCC)": GCC is run to be
    /// found, so a build searches for it only when it is wanted).
    fn looks_for_every_compiler(self) -> bool {
        match self {
            ReadyFor::Build | ReadyFor::Lint => false,
            ReadyFor::Doctor => true,
        }
    }

    /// Whether the compiler itself runs afterwards, so the temporary
    /// directory it writes in must be usable: only for a build. Lint runs
    /// clang-tidy, and doctor runs nothing more.
    fn runs_the_compiler(self) -> bool {
        match self {
            ReadyFor::Build => true,
            ReadyFor::Lint | ReadyFor::Doctor => false,
        }
    }
}

/// What [`steps`] tells as it goes: the build commands stop at the first
/// problem, `cuno doctor` prints each and goes on.
pub trait Steps {
    /// What a step found: `Selected` (and the executable, with an empty
    /// label), `Using`, `Accepted` (`docs/en/specs/commands.md`, "What
    /// `cuno doctor` prints").
    fn status(&mut self, label: &'static str, line: String);
    /// A variable left out of the compiler's environment
    /// (`docs/en/specs/toolchain.md`, "How it is resolved").
    fn ignored(&mut self, name: &str);
    /// A step failed. `Break` ends [`steps`] with the error; `Continue`
    /// goes on with the steps that do not need the one that failed. [`ready`]
    /// relies on its sink always answering `Break`: it has no `Ready` to
    /// return after a problem it went on past.
    fn problem(&mut self, error: Box<dyn CunoError>) -> ControlFlow<Box<dyn CunoError>>;
}

/// The build commands' [`Steps`]: the first problem is the result, and
/// each ignored variable is warned about.
struct StopAtFirst<'a> {
    shell: &'a mut Shell,
}

impl Steps for StopAtFirst<'_> {
    fn status(&mut self, _: &'static str, _: String) {}

    fn ignored(&mut self, name: &str) {
        self.shell
            .warning(format!("{name} is set in the environment and is not used"));
        self.shell.help("write the flags in cuno.toml");
    }

    fn problem(&mut self, error: Box<dyn CunoError>) -> ControlFlow<Box<dyn CunoError>> {
        ControlFlow::Break(error)
    }
}

/// What [`steps`] leaves for the caller.
pub struct Surveyed {
    /// What detection found, for the `Info` lines (`select::survey`).
    pub detected: Detected,
    /// The compiler wanted, if its name is one cuno knows.
    pub wanted: Option<CompilerKind>,
    /// Whether selection reported the Visual Studio in `[toolchain.paths]
    /// msvc` as unusable.
    pub vs_reported: bool,
    /// The compiler, when it was selected and its environment prepared.
    /// A problem the sink went on past in another step (the edition, the
    /// probe) leaves it `Some`; one in selection or preparing leaves it
    /// `None`, and is always reported to the sink first.
    pub ready: Option<Ready>,
}

/// Hands `error` to `sink`: `Err` when it stops the steps.
fn report(sink: &mut dyn Steps, error: Box<dyn CunoError>) -> Result<(), Box<dyn CunoError>> {
    match sink.problem(error) {
        ControlFlow::Break(error) => Err(error),
        ControlFlow::Continue(()) => Ok(()),
    }
}

/// Every step a compiler takes before it builds, in order: detect, select,
/// check the edition, prepare the environment, probe the standard library,
/// and, for a build, check the temporary directory. Each finding goes to
/// `sink`, which decides whether a problem stops the rest; a step that
/// needs one that failed does not run. The build commands take these
/// through [`ready`] and `cuno doctor` directly, so the two cannot disagree
/// about what a compiler needs. `settings` is `None` when the
/// configuration could not be read, and `choices` is then what does not
/// come from a file. The probe runs in `cwd` outside a project, and its
/// scratch directory is made under `temp`. `Err` only when the sink
/// answered `Break`; with `Continue`, every problem has gone to the sink
/// and the result is `Ok`.
// Each argument answers its own question; a struct would only rename them.
#[allow(clippy::too_many_arguments)]
pub fn steps(
    ready_for: ReadyFor,
    settings: Option<&Settings>,
    choices: &Choices,
    project: Option<(&Path, &Manifest)>,
    profile: Profile,
    cwd: &Path,
    env: &Env,
    host: HostOs,
    temp: &Path,
    runner: &dyn Runner,
    registry: &dyn Registry,
    sink: &mut dyn Steps,
) -> Result<Surveyed, Box<dyn CunoError>> {
    // The compiler asked for decides what it needs: one with no MSVC
    // toolset builds without Visual Studio, and GCC is searched only when
    // it is wanted. A name cuno does not know is selection's error, below.
    let wanted = choices.kind(host).ok().map(|(kind, _)| kind);
    let detected = detect(
        settings,
        env,
        host,
        runner,
        ready_for.looks_for_every_compiler() || wanted == Some(CompilerKind::Gcc),
    );
    if let Err(error) = &detected.installs
        && wanted.is_none_or(|kind| kind.uses_msvc_toolset(host))
    {
        report(sink, boxed(error.clone()))?;
    }
    if let Err(error) = &detected.llvm {
        report(sink, boxed(error.clone()))?;
    }
    if let Some(Err(error)) = &detected.gcc {
        report(sink, boxed(error.clone()))?;
    }

    let mut surveyed = Surveyed {
        detected,
        wanted,
        vs_reported: false,
        ready: None,
    };
    let found = surveyed
        .detected
        .found(settings.and_then(Settings::msvc_path));
    let constraints = settings.map(|s| s.constraints.clone()).unwrap_or_default();
    let selected = match select::select(choices, found, &constraints, host) {
        Ok(selected) => selected,
        Err(error) => {
            let vs_reported = matches!(error, ToolchainError::VisualStudioNotAt { .. });
            let vs_error = vs_reported || matches!(error, ToolchainError::NoVisualStudio);
            // Already reported: no Visual Studio is what detection's
            // failure means, and detection reported it, since selection
            // asks for Visual Studio only for a compiler that uses a
            // toolset, the case detection reports it for.
            if !(surveyed.detected.installs.is_err()
                && matches!(error, ToolchainError::NoVisualStudio))
            {
                report(sink, boxed(error))?;
            }
            // Selection stops at Visual Studio before it looks at the copy
            // of the compiler asked for, which has no `Info` line: a
            // configured one that cannot be used is reported here, or
            // nowhere.
            if vs_error
                && let Some(kind) = wanted
                && let Err(unusable @ ToolchainError::ConfiguredToolUnusable { .. }) =
                    select::copy_of(kind, found, &constraints, host)
            {
                report(sink, boxed(unusable))?;
            }
            surveyed.vs_reported = vs_reported;
            return Ok(surveyed);
        }
    };
    // Without a readable configuration, automatic selection is not what the
    // configuration chose (commands.md, "`cuno doctor`").
    let source = match (selected.source, settings) {
        (select::Source::Automatic, None) => {
            "automatic selection; the configuration could not be read".to_owned()
        }
        (source, _) => source.to_string(),
    };
    sink.status(
        "Selected",
        format!("{} {} ({source})", selected.kind.name(), selected.version),
    );
    // The executable, on the line under `Selected`, with no label.
    sink.status("", selected.executable.display().to_string());

    if let Some((_, manifest)) = project
        && let Err(error) = check_edition(selected.kind, selected.version, manifest.package.edition)
    {
        report(sink, boxed(error))?;
    }
    // A Clang borrows a toolset, named here; cl's own is its `Selected`
    // line.
    if let Some(toolset) = &selected.msvc
        && selected.kind.borrows_msvc_toolset()
    {
        sink.status(
            "Using",
            format!(
                "MSVC {} ({})",
                toolset.cl_version,
                toolset.install_dir().display()
            ),
        );
    }
    let prepared = match prepare::prepare(&selected, env, host, registry, runner) {
        Ok(prepared) => prepared,
        Err(error) => {
            report(sink, boxed(error))?;
            return Ok(surveyed);
        }
    };
    for name in &prepared.ignored {
        sink.ignored(name);
    }
    if let Some(sdk) = &prepared.sdk {
        sink.status("Using", format!("Windows SDK {}", sdk.version));
    }

    // The probe compiles with the build's include path, so its command line
    // is the build's. The headers are written there only later, by the
    // build, and need not exist yet: the probe includes none of them, and a
    // missing include directory is no error to any compiler. Without a cuno
    // home there is no such path, and no probe.
    if let Some(bundled) = bundled_include(settings, env)
        && stl::applies(&selected)
        && let Some(compile) =
            probe_compile(&selected, &prepared, project, profile, cwd, host, &bundled)
    {
        match probe(&selected, &compile, temp, ready_for.command(), runner) {
            Ok(()) => sink.status("Accepted", "by the MSVC standard library".to_owned()),
            Err(error) => report(sink, error)?,
        }
    }
    // A compiler with no check still writes in the temporary directory (cl
    // stops with D8050 when it cannot), so the build asks whether it can
    // before compiling, and says how to fix it in cuno's words, not the
    // compiler's (`architecture.md`, "What goes in `cuno-core`").
    if ready_for.runs_the_compiler()
        && !stl::applies(&selected)
        && let Err(error) = scratch::in_temp(temp, ready_for.command(), "compile")
    {
        report(sink, boxed(error))?;
    }
    let llvm = surveyed.detected.llvm.clone().unwrap_or_default();
    surveyed.ready = Some(Ready {
        selected,
        prepared,
        llvm,
    });
    Ok(surveyed)
}

/// Every step a build takes before compiling ([`steps`]), stopping at the
/// first problem, which is returned for the caller to report. Each
/// variable left out of the environment is warned about as the build
/// prepares it.
// Each argument answers its own question; a struct would only rename them.
#[allow(clippy::too_many_arguments)]
pub fn ready(
    ready_for: ReadyFor,
    settings: &Settings,
    project: (&Path, &Manifest),
    profile: Profile,
    env: &Env,
    host: HostOs,
    temp: &Path,
    runner: &dyn Runner,
    registry: &dyn Registry,
    shell: &mut Shell,
) -> Result<Ready, Box<dyn CunoError>> {
    let surveyed = steps(
        ready_for,
        Some(settings),
        &settings.compiler,
        Some(project),
        profile,
        project.0,
        env,
        host,
        temp,
        runner,
        registry,
        &mut StopAtFirst { shell },
    )?;
    Ok(surveyed
        .ready
        .expect("`StopAtFirst` breaks at every problem, and only a problem leaves no `Ready`"))
}

/// Runs the check of `what` with a scratch directory of this process's own
/// under `temp`, the temporary directory, removed afterwards. `command`
/// names the directory, so a reader of the temporary directory can tell
/// which cuno command left one behind.
pub fn in_scratch(
    temp: &Path,
    command: &str,
    what: &'static str,
    check: impl FnOnce(&Path) -> Result<(), ToolchainError>,
) -> Result<(), Box<dyn CunoError>> {
    let scratch = scratch::in_temp(temp, command, &format!("check {what}")).map_err(boxed)?;
    check(scratch.path()).map_err(boxed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cuno_core::ExitClass;
    use cuno_core::NameCase;
    use cuno_toolchain::Version;
    use cuno_toolchain::msvc::MsvcToolset;
    use cuno_toolchain::select::Source;

    fn selected() -> Selected {
        Selected {
            kind: CompilerKind::Clang,
            executable: "clang.exe".into(),
            version: Version::find_in("22.1.8").unwrap(),
            msvc: Some(MsvcToolset {
                dir: "C:/msvc".into(),
                cl_version: Version::find_in("19.52.37029").unwrap(),
            }),
            source: Source::CommandLine,
        }
    }

    fn prepared() -> Prepared {
        Prepared {
            env: Env::from_pairs(
                [("INCLUDE".into(), "C:/msvc/include;C:/sdk/ucrt".into())],
                NameCase::Insensitive,
            ),
            // An environment with INCLUDE is one prepared for a toolset,
            // which always has its SDK.
            sdk: Some(cuno_toolchain::msvc::WindowsSdk {
                dir: "C:/sdk".into(),
                version: "10.0.28000.0".to_owned(),
            }),
            ignored: Vec::new(),
            gcc: None,
        }
    }

    fn manifest(toml: &str) -> Manifest {
        cuno_manifest::parse_manifest(toml, Path::new("cuno.toml"), Path::new("C:/p"))
            .unwrap()
            .value
    }

    /// Both compiles a probe is made from run where the build would: in a
    /// project, its root; outside one, the current directory. A C package
    /// gets neither (`toolchain.md`, "Minimum compiler versions").
    ///
    /// Mutation: run either outside a project somewhere other than the
    /// current directory, run it in the current directory inside one, or
    /// check a C package; this test fails.
    #[test]
    fn the_probes_run_where_the_build_would() {
        let (selected, prepared) = (selected(), prepared());
        let cwd = Path::new("C:/here");
        let bundled = Path::new("C:/home/include");
        let cxx = manifest("[package]\nname = \"app\"\nversion = \"0.1.0\"\n");
        let c = manifest("[package]\nname = \"app\"\nversion = \"0.1.0\"\nlang = \"c\"\n");
        let root = Path::new("C:/p");
        for (project, want) in [(None, cwd), (Some((root, &cxx)), root)] {
            let stl = probe_compile(
                &selected,
                &prepared,
                project,
                Profile::Dev,
                cwd,
                HostOs::Windows,
                bundled,
            )
            .unwrap();
            assert_eq!(stl.cwd, want);
            let tidy =
                tidy_compile(&selected, &prepared, project, cwd, HostOs::Windows, bundled).unwrap();
            assert_eq!(tidy.cwd, want);
        }
        let project = Some((root, &c));
        assert!(
            probe_compile(
                &selected,
                &prepared,
                project,
                Profile::Dev,
                cwd,
                HostOs::Windows,
                bundled
            )
            .is_none()
        );
        assert!(
            tidy_compile(&selected, &prepared, project, cwd, HostOs::Windows, bundled).is_none()
        );
    }

    /// clang-tidy is told the `INCLUDE` and cl version the prepared
    /// environment and the toolset give, as compile_commands.json is.
    ///
    /// Mutation: give the context no include directories, another order,
    /// or no version; this test fails.
    #[test]
    fn the_tidy_context_is_compile_commands_json_s() {
        let tidy = tidy_compile(
            &selected(),
            &prepared(),
            None,
            Path::new("C:/here"),
            HostOs::Windows,
            Path::new("C:/home/include"),
        )
        .unwrap();
        let context = tidy.context();
        assert_eq!(context.kind, CompilerKind::Clang);
        assert_eq!(
            context.include,
            [
                PathBuf::from("C:/msvc/include"),
                PathBuf::from("C:/sdk/ucrt")
            ]
        );
        assert_eq!(
            context.msvc.map(ToString::to_string).as_deref(),
            Some("19.52.37029")
        );
    }

    /// The scratch directory is made under the temporary directory the
    /// caller gives, named for the command, and removed afterwards.
    ///
    /// Mutation: make it under `std::env::temp_dir()` instead of `temp`,
    /// leave the process id out of its name, or leave it behind; this test
    /// fails.
    #[test]
    fn the_scratch_directory_is_made_under_the_given_temporary_directory() {
        let temp = tempfile::tempdir().unwrap();
        let mut seen = None;
        in_scratch(temp.path(), "build", "clang", |dir| {
            assert!(dir.is_dir(), "{}", dir.display());
            seen = Some(dir.to_owned());
            Ok(())
        })
        .unwrap();
        let seen = seen.unwrap();
        assert_eq!(seen.parent(), Some(temp.path()));
        let name = seen.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(name, format!("cuno-build-{}-0", std::process::id()));
        assert!(!seen.exists(), "the scratch directory was left behind");
    }

    /// A directory a killed run left under the name this one would take, its
    /// process id reused, is passed over and left alone; the check still
    /// runs.
    ///
    /// Mutation: fail on a name already taken, or take it over; this test
    /// fails.
    #[test]
    fn a_name_left_by_a_killed_run_is_passed_over() {
        let temp = tempfile::tempdir().unwrap();
        let left = temp
            .path()
            .join(format!("cuno-doctor-{}-0", std::process::id()));
        std::fs::create_dir(&left).unwrap();
        std::fs::write(left.join("theirs"), "").unwrap();
        let mut seen = None;
        in_scratch(temp.path(), "doctor", "clang-tidy", |dir| {
            seen = Some(dir.to_owned());
            Ok(())
        })
        .unwrap();
        assert_ne!(seen.as_deref(), Some(left.as_path()));
        assert!(left.join("theirs").is_file());
    }

    /// A temporary directory that does not exist is a setup problem naming
    /// the check, and the check does not run.
    ///
    /// Mutation: create the directory with its parents, or run the check
    /// anyway; this test fails.
    #[test]
    fn a_missing_temporary_directory_is_a_setup_problem() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("missing");
        let err = in_scratch(&missing, "doctor", "clang-tidy", |_| {
            panic!("the check ran without a scratch directory")
        })
        .unwrap_err();
        assert_eq!(err.exit_class(), ExitClass::Setup);
        assert!(err.to_string().contains(" to check clang-tidy: "), "{err}");
        assert!(!missing.exists());
    }

    /// Answers as a GCC 10.2 (UCRT, MinGW) would, and records whether it
    /// was asked where its headers are, which only preparing its
    /// environment asks; it cannot answer that.
    struct Gcc10 {
        asked_headers: std::cell::Cell<bool>,
    }

    impl Runner for Gcc10 {
        fn run(
            &self,
            _: &Path,
            args: &[&std::ffi::OsStr],
            _: &Env,
        ) -> std::io::Result<cuno_toolchain::Output> {
            let says = |text: &str| cuno_toolchain::Output {
                success: true,
                stdout: text.as_bytes().to_vec(),
                stderr: Vec::new(),
            };
            Ok(match args[0].to_str().unwrap() {
                "-dumpfullversion" => says("10.2.0\n"),
                "--version" => says("gcc 10.2.0\n"),
                "-dumpmachine" => says("x86_64-w64-mingw32\n"),
                "-dM" => says("#define _UCRT 1\n"),
                asked => {
                    if asked == "-E" {
                        self.asked_headers.set(true);
                    }
                    cuno_toolchain::Output {
                        success: false,
                        stdout: Vec::new(),
                        stderr: Vec::new(),
                    }
                }
            })
        }
    }

    /// Records the problems, and goes on after them or not.
    struct Recording {
        go_on: bool,
        problems: usize,
    }

    impl Steps for Recording {
        fn status(&mut self, _: &'static str, _: String) {}

        fn ignored(&mut self, _: &str) {}

        fn problem(&mut self, error: Box<dyn CunoError>) -> ControlFlow<Box<dyn CunoError>> {
            self.problems += 1;
            if self.go_on {
                ControlFlow::Continue(())
            } else {
                ControlFlow::Break(error)
            }
        }
    }

    /// After a problem the sink goes on from (an edition GCC 10 cannot
    /// build), the steps that do not need the failed one still run, so
    /// doctor also sees the environment fail; a sink that stops (the
    /// build's) ends at the first problem and runs nothing after it.
    ///
    /// Mutation: return after the edition's problem whatever the sink
    /// answers, or go on after a `Break`; this test fails.
    #[test]
    fn the_steps_go_on_only_when_the_sink_does() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("gcc"), "").unwrap();
        let env = Env::from_pairs(
            [("PATH".into(), dir.path().as_os_str().to_owned())],
            NameCase::host(),
        );
        let cxx23 =
            manifest("[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"c++23\"\n");
        let choices = Choices {
            cli: Some("gcc".to_owned()),
            ..Default::default()
        };
        for go_on in [true, false] {
            let runner = Gcc10 {
                asked_headers: false.into(),
            };
            let mut sink = Recording { go_on, problems: 0 };
            let result = steps(
                ReadyFor::Doctor,
                None,
                &choices,
                Some((dir.path(), &cxx23)),
                Profile::Dev,
                dir.path(),
                &env,
                HostOs::Other,
                dir.path(),
                &runner,
                &cuno_toolchain::msvc::SystemRegistry,
                &mut sink,
            );
            match result {
                Ok(surveyed) => {
                    assert!(go_on);
                    assert!(surveyed.ready.is_none());
                    assert!(runner.asked_headers.get());
                    assert_eq!(sink.problems, 2);
                }
                Err(error) => {
                    assert!(!go_on);
                    assert!(error.to_string().contains("c++23"), "{error}");
                    assert!(!runner.asked_headers.get());
                    assert_eq!(sink.problems, 1);
                }
            }
        }
    }

    /// What each command makes ready decides whether every GCC is run to be
    /// found, and whether the temporary directory is checked for the
    /// compiler: only doctor looks for every compiler, only a build runs
    /// the compiler afterwards.
    ///
    /// Mutation: let lint look for every compiler (or doctor not), or check
    /// the temporary directory for lint or doctor; this test fails.
    #[test]
    fn what_each_command_makes_ready_for() {
        let answers: Vec<(ReadyFor, bool, bool)> =
            [ReadyFor::Build, ReadyFor::Lint, ReadyFor::Doctor]
                .into_iter()
                .map(|r| (r, r.looks_for_every_compiler(), r.runs_the_compiler()))
                .collect();
        assert_eq!(
            answers,
            [
                (ReadyFor::Build, false, true),
                (ReadyFor::Lint, false, false),
                (ReadyFor::Doctor, true, false),
            ]
        );
    }
}
