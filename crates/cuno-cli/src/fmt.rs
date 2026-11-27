//! `cuno fmt`: format the package's code with clang-format.
//! See `docs/en/specs/commands.md`, "`cuno fmt`", and
//! `docs/en/specs/dev-tools.md`.

use std::path::{Path, PathBuf};

use cuno_build::sources;
use cuno_build::toolchain;
use cuno_core::{CunoError, Env, Shell};
use cuno_manifest::{ConfigError, Settings};
use cuno_toolchain::llvm::Tool;
use cuno_toolchain::select;
use cuno_toolchain::{HostOs, SystemRunner};
use cuno_tools::format::{self, ClangFormat};
use cuno_tools::targets::{self, Targets, relative};

use crate::build::{boxed, in_project};

/// Formats the package's code around `cwd`, or with `check` only says
/// which files are not formatted; returns the exit code: 1 when `check`
/// found one, else 0. No compiler is detected or selected, only
/// clang-format (`dev-tools.md`, "Target files").
pub fn fmt(
    check: bool,
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

    let code = sources::code_files(root, manifest, &settings.target_dir, host).map_err(boxed)?;
    for warning in &code.warnings {
        shell.warning(warning);
    }
    // A link can lead outside the project; it is read by the build, never
    // written by fmt (dev-tools.md, "Target files").
    let Targets { files, links } = targets::select(
        root,
        code.files.into_iter().map(|file| file.path).collect(),
        &manifest.fmt.exclude,
        host.name_case(),
    );
    for link in &links {
        shell.warning(format!(
            "`{}` is a link; cuno fmt leaves it",
            relative(root, link)
        ));
    }

    let runner = SystemRunner::new();
    let llvm = toolchain::detect(Some(&settings), env, host, &runner, false)
        .llvm
        .map_err(boxed)?;
    let clang_format = select::select_tool(Tool::ClangFormat, &llvm, host).map_err(boxed)?;

    if !check {
        for warning in cuno_tools::write_editor_files(root, manifest) {
            shell.warning(warning);
        }
    }
    // Formatted with cuno.toml's configuration whatever `.clang-format` the
    // root holds (a hand-written one is kept, not used), from a scratch
    // directory, since `--check` writes nothing in the project.
    let scratch = cuno_core::scratch::in_temp(temp, "fmt", "format").map_err(boxed)?;
    let style = scratch.path().join(format::FILE);
    let written = std::fs::write(
        &style,
        format::clang_format_config(&manifest.fmt, manifest.package.lang),
    );
    let result = written
        .map_err(|e| boxed(scratch_error(&style, e)))
        .and_then(|()| {
            let jobs = settings.jobs.get() as usize;
            let tool = ClangFormat {
                program: &clang_format.path,
                style: &style,
                env,
                jobs,
            };
            format::check_style(&tool, &runner).map_err(boxed)?;
            Ok(format::format(&files, &tool, check, &runner))
        });
    drop(scratch);
    let formatted = result?;

    // Every file that could not be formatted, each left as it was.
    for failure in &formatted.failed {
        shell.error(format!(
            "could not format `{}`: {}",
            relative(root, &failure.file),
            failure.reason
        ));
    }
    let failed = !formatted.failed.is_empty();
    let changed = formatted.changed;
    if check {
        for file in &changed {
            shell.error(format!("`{}` is not formatted", relative(root, file)));
        }
        if !changed.is_empty() {
            shell.help("run cuno fmt to format them");
        }
        return Ok(i32::from(failed || !changed.is_empty()));
    }
    // Said even when another file failed: these were rewritten.
    let noun = if changed.len() == 1 { "file" } else { "files" };
    shell.status("Formatted", format!("{} {noun}", changed.len()));
    Ok(i32::from(failed))
}

/// The configuration could not be written in the scratch directory already
/// made: an environment problem (a full disk, a policy on the directory).
fn scratch_error(path: &Path, e: std::io::Error) -> ScratchError {
    ScratchError {
        path: path.to_owned(),
        reason: e.to_string(),
    }
}

/// The scratch copy of the configuration could not be written.
#[derive(Debug)]
struct ScratchError {
    path: PathBuf,
    reason: String,
}

impl std::fmt::Display for ScratchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cannot write {}: {}", self.path.display(), self.reason)
    }
}

impl std::error::Error for ScratchError {}

impl CunoError for ScratchError {
    fn exit_class(&self) -> cuno_core::ExitClass {
        cuno_core::ExitClass::Setup
    }
}
