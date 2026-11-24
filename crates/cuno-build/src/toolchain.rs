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

use cuno_core::{CunoError, Env, ExitClass, Shell};
use cuno_manifest::{Lang, Manifest, Profile, Settings};
use cuno_toolchain::flags::{Mode, check_edition};
use cuno_toolchain::msvc::Registry;
use cuno_toolchain::prepare::{self, Detected, Prepared};
use cuno_toolchain::probe::{self as stl, ProbeInputs};
use cuno_toolchain::select::{self, Found, Selected};
use cuno_toolchain::{HostOs, Runner, ToolchainError};

use crate::bundled;
use crate::input::{CompileSettings, compile_settings};

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
/// under `temp`, the temporary directory, whose name carries `command`
/// (`build`, `doctor`). Ok for a compiler it does not apply to
/// (`cuno_toolchain::probe::check` decides), so a caller need not ask
/// first; doctor still does, to print `Accepted` only for one it checked.
pub fn probe(
    selected: &Selected,
    compile: &ProbeCompile,
    temp: &Path,
    command: &str,
    runner: &dyn Runner,
) -> Result<(), Box<dyn CunoError>> {
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
    // `create_dir`, not `create_dir_all`: a temporary directory that does not
    // exist is a problem to report, not a tree to make.
    // The process id keeps two cuno processes apart. It is the process's
    // identity, not its environment, so a library may ask for it.
    let scratch = temp.join(format!("cuno-{command}-{}", std::process::id()));
    std::fs::create_dir(&scratch).map_err(|e| {
        boxed(ScratchError {
            what,
            dir: scratch.clone(),
            reason: e.to_string(),
        })
    })?;
    let result = check(&scratch);
    let _ = std::fs::remove_dir_all(&scratch);
    result.map_err(boxed)
}

/// The probe's scratch directory cannot be made in the temporary
/// directory (`TEMP` names one that does not exist or cannot be written).
#[derive(Debug)]
struct ScratchError {
    what: &'static str,
    dir: PathBuf,
    reason: String,
}

impl std::fmt::Display for ScratchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "cannot create {} to check {}: {}",
            self.dir.display(),
            self.what,
            self.reason
        )
    }
}

impl std::error::Error for ScratchError {}

impl CunoError for ScratchError {
    fn exit_class(&self) -> ExitClass {
        ExitClass::Setup
    }
    fn help(&self) -> Option<String> {
        Some("set TEMP to a directory that exists and can be written".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The scratch directory is made under the temporary directory the
    /// caller gives, named for the command, and removed afterwards.
    ///
    /// Mutation: make it under `std::env::temp_dir()` instead of `temp`, or
    /// leave it behind; this test fails.
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
        assert!(name.starts_with("cuno-build-"), "{name}");
        assert!(!seen.exists(), "the scratch directory was left behind");
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
