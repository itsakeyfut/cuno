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
    let case = host.name_case();
    let files: Vec<PathBuf> = code
        .files
        .into_iter()
        .map(|file| file.path)
        .filter(|path| {
            let relative = relative(root, path);
            !manifest
                .fmt
                .exclude
                .iter()
                .any(|glob| glob.matches(&relative, case))
        })
        .collect();
    // A link can lead outside the project; it is read by the build, never
    // written by fmt (dev-tools.md, "Target files").
    let (links, files): (Vec<PathBuf>, Vec<PathBuf>) = files.into_iter().partition(|path| {
        std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
    });
    for link in &links {
        shell.warning(format!(
            "`{}` is a link; cuno fmt leaves it",
            relative(root, link)
        ));
    }

    let runner = SystemRunner::new();
    let llvm = toolchain::detect(Some(&settings), env, host, &runner)
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
    let scratch = scratch_dir(temp).map_err(boxed)?;
    let style = scratch.join(format::FILE);
    let written = std::fs::write(
        &style,
        format::clang_format_config(&manifest.fmt, manifest.package.lang),
    );
    let result = written
        .map_err(|e| boxed(scratch_error(&style, e)))
        .and_then(|()| {
            let jobs = settings
                .jobs
                .map(|n| n.get() as usize)
                .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()));
            let tool = ClangFormat {
                program: &clang_format.path,
                style: &style,
                env,
                jobs,
            };
            format::format(&files, &tool, check, &runner).map_err(boxed)
        });
    let _ = std::fs::remove_dir_all(&scratch);
    let changed = result?;

    if check {
        if changed.is_empty() {
            return Ok(0);
        }
        for file in &changed {
            shell.error(format!("`{}` is not formatted", relative(root, file)));
        }
        shell.help("run cuno fmt to format them");
        return Ok(1);
    }
    let noun = if changed.len() == 1 { "file" } else { "files" };
    shell.status("Formatted", format!("{} {noun}", changed.len()));
    Ok(0)
}

/// `path` from the project root, with `/` between its names, as `exclude`
/// patterns are written.
fn relative(root: &Path, path: &Path) -> String {
    let from_root = path
        .strip_prefix(root)
        .or_else(|_| path.strip_prefix(cuno_core::paths::lexical(root)))
        .unwrap_or(path);
    from_root
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// A directory of its own under `temp`, made here and not found: one that
/// already exists, whoever made it, is never used.
fn scratch_dir(temp: &Path) -> Result<PathBuf, ScratchError> {
    for n in 0..100 {
        let dir = temp.join(format!("cuno-fmt-{}-{n}", std::process::id()));
        match std::fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(scratch_error(&dir, e)),
        }
    }
    Err(scratch_error(
        temp,
        std::io::Error::other("every scratch name is taken"),
    ))
}

/// A scratch file that could not be made: an environment problem
/// (`TEMP` names a directory that does not exist or cannot be written).
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
