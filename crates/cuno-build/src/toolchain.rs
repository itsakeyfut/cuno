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

use std::path::{Path, PathBuf};

use cuno_core::scratch;
use cuno_core::{CunoError, Env, Shell};
use cuno_manifest::{Lang, Manifest, Profile, Settings};
use cuno_toolchain::compiler::CompilerKind;
use cuno_toolchain::flags::{Mode, ToolContext, check_edition, include_dirs};
use cuno_toolchain::msvc::Registry;
use cuno_toolchain::prepare::{self, Detected, Prepared};
use cuno_toolchain::probe::{self as stl, ProbeInputs};
use cuno_toolchain::select::{self, Found, LlvmChoice, Selected};
use cuno_toolchain::{HostOs, Runner, ToolchainError, Version};

use crate::bundled;
use crate::input::{CompileSettings, compile_settings, editor_settings};

/// The steps fail with errors of several crates, each a `CunoError`, and
/// return them as one type. The command code above this crate has a helper
/// of its own for the same; a crate cannot share one with the code that
/// depends on it.
fn boxed(e: impl CunoError + 'static) -> Box<dyn CunoError> {
    Box::new(e)
}

/// Detects Visual Studio and the LLVM tools, the copies `[toolchain.paths]`
/// names first; none are named when the settings could not be read.
pub fn detect(
    settings: Option<&Settings>,
    env: &Env,
    host: HostOs,
    runner: &dyn Runner,
) -> Detected {
    let explicit = settings.map(Settings::llvm_paths).unwrap_or_default();
    prepare::detect(env, host, &explicit, runner)
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
/// clang-tidy with ([`editor_settings`]), and what clang-tidy is told of
/// the compiler (`toolchain.md`, "Minimum compiler versions").
pub struct TidyCompile {
    /// The resolved Windows environment.
    pub env: Env,
    /// compile_commands.json's settings for a C++ input.
    pub settings: CompileSettings,
    /// Where the build runs, as for [`ProbeCompile`].
    pub cwd: PathBuf,
    kind: CompilerKind,
    include: Vec<PathBuf>,
    msvc: Option<Version>,
}

impl TidyCompile {
    /// What clang-tidy is told of the compiler, as compile_commands.json
    /// tells it.
    pub fn context(&self) -> ToolContext<'_> {
        ToolContext {
            kind: self.kind,
            include: &self.include,
            msvc: self.msvc.as_ref(),
        }
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
    if project.is_some_and(|(_, manifest)| manifest.package.lang == Lang::C) {
        return None;
    }
    Some(TidyCompile {
        env: prepared.env.clone(),
        settings: editor_settings(project, selected.kind, Lang::Cxx, host, bundled),
        cwd: project.map_or_else(|| cwd.to_owned(), |(root, _)| root.to_owned()),
        kind: selected.kind,
        include: include_dirs(prepared.env.get("INCLUDE")),
        msvc: selected.msvc.as_ref().map(|toolset| toolset.cl_version),
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
}

/// Every step a build takes before compiling: detect, select, check the
/// edition, prepare the environment, and probe the standard library. Each
/// variable left out of the environment is warned about here, as the build
/// prepares it; the first problem is returned for the caller to report,
/// and no later step runs. The probe's scratch directory is made under
/// `temp`, and the environment is resolved through `registry`.
// Each argument answers its own question; a struct would only rename them.
#[allow(clippy::too_many_arguments)]
pub fn ready(
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
    let detected = detect(Some(settings), env, host, runner);
    let installs = detected.installs.map_err(boxed)?;
    let llvm = detected.llvm.map_err(boxed)?;
    let found = Found {
        installs: &installs,
        llvm: &llvm,
        explicit_msvc: settings.msvc_path(),
    };
    let selected =
        select::select(&settings.compiler, found, &settings.constraints, host).map_err(boxed)?;
    check_edition(selected.kind, selected.version, project.1.package.edition).map_err(boxed)?;
    let prepared = prepare::prepare(&selected, env, registry).map_err(boxed)?;
    // Left out of the build's environment, since they add flags no command
    // line shows (toolchain.md, "How it is resolved").
    for name in &prepared.ignored {
        shell.warning(format!("{name} is set in the environment and is not used"));
        shell.help("write the flags in cuno.toml");
    }
    if let Some(compile) = probe_compile(
        &selected,
        &prepared,
        Some(project),
        profile,
        project.0,
        host,
        // The build's include path, so the probe's command line is the
        // build's. The headers are written there only later, by the build,
        // and need not exist yet: the probe includes none of them, and a
        // missing include directory is no error to any compiler.
        &bundled::include_dir(&settings.home),
    ) {
        probe(&selected, &compile, temp, "build", runner)?;
    }
    // A compiler with no check still writes in the temporary directory (cl
    // stops with D8050 when it cannot), so the build asks whether it can
    // before compiling, and says how to fix it in cuno's words, not the
    // compiler's (`architecture.md`, "What goes in `cuno-core`").
    if !stl::applies(&selected) {
        scratch::in_temp(temp, "build", "compile").map_err(boxed)?;
    }
    Ok(Ready { selected, prepared })
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
            sdk: None,
            ignored: Vec::new(),
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
}
