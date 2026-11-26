//! `cuno lint`: analyse the package's code with clang-tidy.
//! See `docs/en/specs/commands.md`, "`cuno lint`", and
//! `docs/en/specs/dev-tools.md`.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::Duration;

use cuno_build::pipeline::{self, Toolchain};
use cuno_build::{bundled, sources, toolchain};
use cuno_core::paths::Place;
use cuno_core::{CunoError, Env, ExitClass, Shell};
use cuno_manifest::{ConfigError, Profile, Settings};
use cuno_toolchain::llvm::Tool;
use cuno_toolchain::msvc::SystemRegistry;
use cuno_toolchain::select;
use cuno_toolchain::{HostOs, SystemRunner};
use cuno_tools::tidy::{self, ClangTidy};

use crate::build::{boxed, in_project};

/// How long clang-tidy may analyse one source (`commands.md`, "`cuno
/// lint`"): work, not a probe, so not the 30 seconds detection allows.
const SOURCE_LIMIT: Duration = Duration::from_secs(600);

/// Analyses the package's code around `cwd`, or with `fix` fixes what
/// clang-tidy can in the sources themselves; returns the exit code: 1 for
/// an error finding or a source that could not be analysed, else 0.
#[allow(clippy::too_many_arguments)]
pub fn lint(
    fix: bool,
    resolved: Result<Settings, ConfigError>,
    env: &Env,
    cwd: &Path,
    host: HostOs,
    temp: &Path,
    shell: &mut Shell,
) -> Result<i32, Box<dyn CunoError>> {
    let settings = in_project(resolved, cwd)?;
    let (project, manifest) = settings
        .project
        .as_ref()
        .expect("in_project found the project the settings were read from");
    let root = project.root.as_path();

    if !tidy::any_check(&tidy::clang_tidy_config(&manifest.lint)) {
        shell.warning(
            "no checks are enabled ([lint] preset = \"none\" and no warn / deny); nothing was analysed",
        );
        return Ok(0);
    }

    // The compiler whose commands are analysed, made ready as for a build
    // of the dev profile, the one compile_commands.json describes.
    let runner = SystemRunner::new();
    let toolchain::Ready { selected, prepared } = toolchain::ready(
        &settings,
        (root, manifest),
        Profile::Dev,
        env,
        host,
        temp,
        &runner,
        &SystemRegistry,
        shell,
    )?;
    for warning in cuno_tools::write_editor_files(root, manifest) {
        shell.warning(warning);
    }
    let bundled = bundled::expand(&settings.home).map_err(boxed)?;
    let version = selected.version.to_string();
    let identity: Vec<OsString> = prepared
        .identity(&version)
        .into_iter()
        .map(OsStr::to_owned)
        .collect();
    let compiler = Toolchain {
        kind: selected.kind,
        executable: &selected.executable,
        version: &selected.version,
        msvc: selected.msvc.as_ref().map(|toolset| &toolset.cl_version),
        env: &prepared.env,
        identity,
    };
    pipeline::write_compile_commands(
        root,
        manifest,
        &settings.target_dir,
        host,
        &compiler,
        &bundled,
    )
    .map_err(boxed)?
    .map_err(|e| boxed(CompdbError(e.to_string())))?;

    // clang-tidy, and its pairing with the standard library, before any
    // source is analysed (toolchain.md, "Minimum compiler versions").
    let llvm = toolchain::detect(Some(&settings), env, host, &runner)
        .llvm
        .map_err(boxed)?;
    let clang_tidy = select::select_tool(Tool::ClangTidy, &llvm, host).map_err(boxed)?;
    if let Some(tidy_compile) = toolchain::tidy_compile(
        &selected,
        &prepared,
        Some((root, manifest)),
        cwd,
        host,
        &bundled,
    ) {
        toolchain::tidy_ready(&clang_tidy, &tidy_compile, temp, "lint", &runner)?;
    }
    if let Ok(known) = tidy::known_checks(&clang_tidy.path, &prepared.env, &runner) {
        for (list, name) in tidy::unknown_names(&manifest.lint, &known) {
            shell.warning(format!(
                "`{name}` in `{list}` names no check this clang-tidy has"
            ));
        }
    }

    // What is analysed: the sources compile_commands.json has commands for,
    // in the package's places and not excluded; findings are kept only in
    // those places' files.
    let case = host.name_case();
    let code = sources::code_files(root, manifest, &settings.target_dir, host).map_err(boxed)?;
    for warning in &code.warnings {
        shell.warning(warning);
    }
    let excluded = |path: &Path| {
        let relative = relative(root, path);
        manifest
            .lint
            .exclude
            .iter()
            .any(|glob| glob.matches(&relative, case))
    };
    let ours: Vec<Place> = code
        .files
        .iter()
        .filter(|file| !excluded(&file.path))
        .map(|file| Place::of(&file.path, case))
        .collect();
    let classified =
        sources::classify(root, manifest, &settings.target_dir, host).map_err(boxed)?;
    let commanded = classified
        .bins
        .iter()
        .map(|bin| &bin.entry)
        .chain(&classified.body)
        .chain(&classified.integration_tests)
        .chain(&classified.test_helpers)
        .map(|source| source.path.clone());
    let mut analysed: Vec<PathBuf> = Vec::new();
    for path in commanded {
        let place = Place::of(&path, case);
        if !ours.iter().any(|ours| ours.same(&place)) {
            continue;
        }
        let link = std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink());
        if fix && link {
            shell.warning(format!(
                "`{}` is a link; cuno lint --fix leaves it",
                relative(root, &path)
            ));
            continue;
        }
        analysed.push(path);
    }

    let scratch = cuno_core::scratch::in_temp(temp, "lint", "analyse").map_err(boxed)?;
    let config = scratch.path().join(tidy::FILE);
    let written = std::fs::write(&config, tidy::clang_tidy_file(&manifest.lint));
    let linted = written.map_err(|e| boxed(ScratchError(config.clone(), e.to_string())));
    let linted = linted.map(|()| {
        let jobs = settings
            .jobs
            .map(|n| n.get() as usize)
            .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()));
        let tool = ClangTidy {
            program: &clang_tidy.path,
            config: &config,
            root,
            env: &prepared.env,
            jobs,
            fix,
            scratch: scratch.path(),
        };
        let is_ours = |path: &Path| {
            let place = Place::of(path, case);
            ours.iter().any(|ours| ours.same(&place))
        };
        tidy::lint(
            &analysed,
            &tool,
            &is_ours,
            &SystemRunner::with_limit(SOURCE_LIMIT),
        )
    });
    drop(scratch);
    let linted = linted?;

    shell.raw(&linted.shown);
    for (source, target) in &linted.not_fixed {
        shell.warning(format!(
            "`{}` is not fixed: a fix would change `{}`, which is not the package's",
            relative(root, source),
            cuno_core::paths::lexical(&root.join(target)).display()
        ));
    }
    for (source, why) in &linted.failed {
        shell.error(format!(
            "could not lint `{}`: {why}",
            relative(root, source)
        ));
    }
    let plural =
        |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    shell.status(
        "Linted",
        format!(
            "{}: {}, {}",
            plural(analysed.len(), "file", "files"),
            plural(linted.warnings, "warning", "warnings"),
            plural(linted.errors, "error", "errors"),
        ),
    );
    Ok(i32::from(linted.errors > 0 || !linted.failed.is_empty()))
}

/// `path` from the project root, with `/` between its names, as `exclude`
/// patterns are written.
fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// The scratch copy of the configuration could not be written: an
/// environment problem.
#[derive(Debug)]
struct ScratchError(PathBuf, String);

impl std::fmt::Display for ScratchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cannot write {}: {}", self.0.display(), self.1)
    }
}

impl std::error::Error for ScratchError {}

impl CunoError for ScratchError {
    fn exit_class(&self) -> ExitClass {
        ExitClass::Setup
    }
}

/// compile_commands.json, which clang-tidy reads the commands from, could
/// not be written: lint cannot go on without it.
#[derive(Debug)]
struct CompdbError(String);

impl std::fmt::Display for CompdbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "could not write compile_commands.json: {}", self.0)
    }
}

impl std::error::Error for CompdbError {}

impl CunoError for CompdbError {
    fn exit_class(&self) -> ExitClass {
        ExitClass::Setup
    }
}
