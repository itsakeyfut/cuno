//! Getting a compiler ready to build with: each step once, for every
//! command that compiles. `cuno build` (and the other build commands) take
//! the steps in order through [`ready`], stopping at the first problem;
//! `cuno doctor` calls the same steps one at a time and goes on after one.
//! See `docs/en/specs/toolchain.md`.

use std::path::{Path, PathBuf};

use cuno_build::input::{CompileSettings, compile_settings};
use cuno_core::{CunoError, Env, Shell};
use cuno_manifest::{Lang, Manifest, Profile, Settings};
use cuno_toolchain::flags::{Mode, check_edition};
use cuno_toolchain::msvc::SystemRegistry;
use cuno_toolchain::prepare::{self, Detected, Prepared};
use cuno_toolchain::probe::{self as stl, ProbeInputs};
use cuno_toolchain::select::{self, Found, Selected};
use cuno_toolchain::{HostOs, Runner};

use crate::build::boxed;
use crate::bundled;
use crate::scratch;

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
/// runs. Doctor's clang-tidy check reads the same compile.
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

/// Runs the standard library probe of `selected`, in a scratch directory
/// whose name carries `command` (`build`, `doctor`). Ok for a compiler it
/// does not apply to (`cuno_toolchain::probe::check` decides), so a caller need not ask
/// first; doctor still does, to print `Accepted` only for one it checked.
pub fn probe(
    selected: &Selected,
    compile: &ProbeCompile,
    command: &str,
    runner: &dyn Runner,
) -> Result<(), Box<dyn CunoError>> {
    scratch::in_scratch(command, selected.kind.name(), |dir| {
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
/// and no later step runs.
pub fn ready(
    settings: &Settings,
    project: (&Path, &Manifest),
    profile: Profile,
    env: &Env,
    host: HostOs,
    runner: &dyn Runner,
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
    let prepared = prepare::prepare(&selected, env, &SystemRegistry).map_err(boxed)?;
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
        probe(&selected, &compile, "build", runner)?;
    }
    Ok(Ready { selected, prepared })
}
