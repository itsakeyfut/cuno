//! One resolution of every setting from every layer, in the precedence
//! `docs/en/specs/manifest.md` (".cuno/config.toml", "Two levels of
//! settings") and `docs/en/specs/build.md` (the output directory) give:
//! command line, environment, `.cuno/config.toml`, `cuno.toml`, the
//! user-level `config.toml`, defaults.

use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use cuno_core::{ColorChoice, CunoError, Env, cuno_home};
use cuno_toolchain::flags::{BuildSettings, ProfileSettings};
use cuno_toolchain::llvm::ExplicitPaths;
use cuno_toolchain::select::{Choices, Constraints, ProjectChoices};

use crate::{
    CompilerName, Config, ConfigError, ConfigWarning, Loaded, Location, Manifest, OptLevel, Os,
    Profile, Project, Runtime, Tool, Warnings, find_project, load_config, load_manifest,
    resolve_path, shown_path,
};

/// What the binary gathers from the process for [`resolve`].
#[derive(Clone, Copy, Debug)]
pub struct Inputs<'a> {
    /// The current directory.
    pub cwd: &'a Path,
    /// The environment snapshot.
    pub env: &'a Env,
    /// The command-line options that are settings.
    pub cli: &'a CliSettings,
}

/// The command-line options that are also settings.
#[derive(Clone, Debug, Default)]
pub struct CliSettings {
    /// `--compiler`.
    pub compiler: Option<String>,
    /// `--target-dir`, as written (relative to the current directory).
    pub target_dir: Option<PathBuf>,
    /// `-j, --jobs`.
    pub jobs: Option<NonZeroU32>,
}

/// Every setting, resolved once for the commands to share.
#[derive(Clone, Debug)]
pub struct Settings {
    /// The project and its `cuno.toml`; `None` outside a project.
    pub project: Option<(Project, Manifest)>,
    /// The cuno home.
    pub home: PathBuf,
    /// The compiler named by each layer, for the selector to rank.
    pub compiler: Choices,
    /// `[toolchain.version]` of `cuno.toml`.
    pub constraints: Constraints,
    /// `[toolchain.paths]`, merged per tool: the project's over the user's.
    pub paths: BTreeMap<Tool, PathBuf>,
    /// The output directory.
    pub target_dir: PathBuf,
    /// `-j` or `[build] jobs`; `None` means the number of CPU cores.
    pub jobs: Option<NonZeroU32>,
    /// `[term] color` of the configuration files. `--color` and `NO_COLOR`
    /// rank above it and are the binary's (`commands.md`).
    pub color: Option<ColorChoice>,
    /// `[term] verbose` of the configuration files.
    pub verbose: bool,
}

impl Settings {
    /// The LLVM tool paths, for detection.
    pub fn llvm_paths(&self) -> ExplicitPaths {
        let path = |tool| self.paths.get(&tool).cloned();
        ExplicitPaths {
            clang: path(Tool::Clang),
            clang_cl: path(Tool::ClangCl),
            clang_format: path(Tool::ClangFormat),
            clang_tidy: path(Tool::ClangTidy),
        }
    }

    /// `[toolchain.paths] msvc`: the only Visual Studio to use, when written.
    pub fn msvc_path(&self) -> Option<&Path> {
        self.paths.get(&Tool::Msvc).map(PathBuf::as_path)
    }
}

/// Reads every layer and resolves every setting. Warnings from all files
/// come back together: `cuno.toml`'s, then `.cuno/config.toml`'s, then the
/// user-level file's.
pub fn resolve(inputs: Inputs<'_>) -> Result<Loaded<Settings>, ConfigError> {
    let Inputs { cwd, env, cli } = inputs;
    let home = cuno_home(env).map_err(|e| ConfigError {
        message: e.to_string(),
        location: None,
        help: e.help(),
    })?;
    let mut warnings: Vec<ConfigWarning> = Vec::new();

    let project = match find_project(cwd) {
        Some(project) => {
            let manifest = load_manifest(&project, cwd)?;
            warnings.extend(manifest.warnings);
            let config = project.config();
            let config = load_config(&config, &shown_path(&config, cwd), &project.root)?;
            warnings.extend(config.warnings);
            Some((project, manifest.value, config.value))
        }
        None => None,
    };
    let user_file = home.join("config.toml");
    // A project whose `.cuno` is the cuno home (one at the user's home
    // directory) has one file for both layers: it is read once, as the
    // project's.
    let same_file = project
        .as_ref()
        .is_some_and(|(p, _, _)| same_path(&p.config(), &user_file));
    let user = if same_file {
        Config::default()
    } else {
        let user = load_config(&user_file, &shown_path(&user_file, cwd), &home)?;
        warnings.extend(user.warnings);
        user.value
    };

    let manifest = project.as_ref().map(|(_, m, _)| m);
    let local = project.as_ref().map(|(_, _, c)| c);
    let from_env = |name| {
        env.get(name)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_owned())
    };

    // Every layer that sets an output directory, highest first: command
    // line, environment, project config, cuno.toml, user config. Each is
    // checked, winning or not, and the first wins (build.md, "Output
    // directory"): the order they are pushed in is the precedence.
    let guarded = project.as_ref().map_or(cwd, |(p, _, _)| &p.root);
    let mut layers: Vec<(PathBuf, String)> = Vec::new();
    let from_command = [
        ("--target-dir", cli.target_dir.clone()),
        (
            "CUNO_TARGET_DIR",
            from_env("CUNO_TARGET_DIR").map(PathBuf::from),
        ),
    ];
    for (name, dir) in from_command {
        let Some(dir) = dir else { continue };
        let invalid = |why: String| ConfigError {
            message: format!("{name} is `{}`, {why}", dir.display()),
            location: None,
            help: Some(
                "write an absolute path, or a path relative to the current directory".to_owned(),
            ),
        };
        if dir.as_os_str().is_empty() {
            return Err(invalid("which is empty".to_owned()));
        }
        let resolved = resolve_path(&dir, cwd)
            .map_err(|_| invalid("which depends on the current drive".to_owned()))?;
        check_output_dir(&resolved, name, None, guarded, project.is_some())?;
        layers.push((resolved, name.to_owned()));
    }
    let project_files = project.as_ref().map(|(p, m, c)| {
        [
            (c.target_dir.clone(), p.config()),
            (m.target_dir.clone(), p.manifest()),
        ]
    });
    let files = project_files
        .into_iter()
        .flatten()
        .map(|(dir, file)| (dir, file, true))
        .chain([(user.target_dir.clone(), user_file.clone(), false)]);
    for (dir, file, carried_by_project) in files {
        let Some(dir) = dir else { continue };
        let source = format!("`build.target-dir` in {}", shown_path(&file, cwd).display());
        if dir.written.as_os_str().is_empty() {
            return Err(ConfigError {
                message: format!("{source} is empty"),
                location: Some(dir.location),
                help: Some("write the output directory, such as \"target\"".to_owned()),
            });
        }
        check_output_dir(
            &dir.path,
            &source,
            Some(&dir.location),
            guarded,
            project.is_some(),
        )?;
        // A file a cloned project carries must not make `cuno clean`
        // delete what lies outside it (build.md, "Output directory"). The
        // user's own config, the environment and the command line may.
        if carried_by_project && !holds(guarded, &dir.path) {
            return Err(ConfigError {
                message: format!(
                    "{source} points outside the project: {}",
                    dir.path.display()
                ),
                location: Some(dir.location),
                help: Some(
                    "set it in the user-level config, CUNO_TARGET_DIR or --target-dir to put the output outside the project".to_owned(),
                ),
            });
        }
        layers.push((dir.path, source));
    }
    let target_dir = layers
        .into_iter()
        .next()
        .map_or_else(|| guarded.join("target"), |(dir, _)| dir);

    let mut paths = user.paths.clone();
    if let Some(local) = local {
        paths.extend(local.paths.clone());
    }

    let compiler = Choices {
        cli: cli.compiler.clone(),
        env: from_env("CUNO_COMPILER").map(|v| v.to_string_lossy().into_owned()),
        project: project.as_ref().map(|(_, m, c)| ProjectChoices {
            config: c.compiler.clone(),
            manifest: m.compiler.clone(),
        }),
        user: user.compiler.clone(),
    };

    let settings = Settings {
        home,
        compiler,
        constraints: manifest.map(|m| m.constraints.clone()).unwrap_or_default(),
        paths,
        target_dir,
        jobs: cli
            .jobs
            .or_else(|| local.and_then(|c| c.jobs))
            .or(user.jobs),
        color: local.and_then(|c| c.color).or(user.color),
        verbose: local
            .and_then(|c| c.verbose)
            .or(user.verbose)
            .unwrap_or(false),
        project: project.map(|(p, m, _)| (p, m)),
    };
    Ok(Loaded {
        value: settings,
        warnings,
    })
}

/// Refuses an output directory that is the project root, or outside a
/// project the current directory, or a directory above it: `cuno clean`
/// deletes the output directory (build.md, "Output directory").
fn check_output_dir(
    dir: &Path,
    source: &str,
    location: Option<&Location>,
    guarded: &Path,
    has_project: bool,
) -> Result<(), ConfigError> {
    if !holds(dir, guarded) {
        return Ok(());
    }
    Err(ConfigError {
        message: format!(
            "the output directory `{}` ({source}) is {} or a directory above it",
            dir.display(),
            if has_project { "the project root" } else { "the current directory" },
        ),
        location: location.cloned(),
        help: Some(
            "choose a directory inside the project, such as `target`; `cuno clean` deletes the output directory".to_owned(),
        ),
    })
}

/// Whether `outer` is `inner` or one of its ancestors, as the file system
/// sees them: see [`real`].
fn holds(outer: &Path, inner: &Path) -> bool {
    real(inner).starts_with(real(outer))
}

/// `path` as the file system names it, whether or not it exists yet: `.`
/// and `..` resolved, and its longest existing part canonicalized, so a
/// path spelled in another case on Windows, or through a link, compares
/// as the directory it is. The part that does not exist yet is kept as
/// written.
fn real(path: &Path) -> PathBuf {
    let path = lexical(path);
    let mut existing = path.as_path();
    let mut rest = Vec::new();
    loop {
        if let Ok(canonical) = std::fs::canonicalize(existing) {
            return rest.iter().rev().fold(canonical, |p, part| p.join(part));
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_owned());
                existing = parent;
            }
            _ => return path,
        }
    }
}

fn same_path(a: &Path, b: &Path) -> bool {
    let canonical = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| lexical(p));
    canonical(a) == canonical(b)
}

/// `path` with `.` dropped and each `..` taking the part before it away.
fn lexical(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

impl Manifest {
    /// `[build]`, then `[platform.<os>.build]`, `[build.<compiler>]` and
    /// `[profile.<profile>.build]`: lists appended, single values overridden
    /// (`manifest.md`, "Settings layering"), then the defaults.
    pub fn build_settings(
        &self,
        os: Os,
        compiler: CompilerName,
        profile: Profile,
    ) -> BuildSettings {
        let mut out = BuildSettings {
            warnings: Warnings::All,
            warnings_as_errors: false,
            defines: Vec::new(),
            include_dirs: Vec::new(),
            links: Vec::new(),
            lib_dirs: Vec::new(),
            link_flags: Vec::new(),
            flags: Vec::new(),
            runtime: Runtime::Dynamic,
            exceptions: true,
            rtti: true,
        };
        let layers = [
            Some(&self.build),
            self.platform.get(&os),
            self.profile_build.get(&profile),
        ];
        for (index, table) in layers.into_iter().enumerate() {
            // `[build.<compiler>]` sits between the platform and the profile.
            if index == 2
                && let Some(flags) = self.compiler_flags.get(&compiler)
            {
                out.flags.extend(flags.iter().cloned());
            }
            let Some(table) = table else { continue };
            if let Some(v) = table.warnings {
                out.warnings = v;
            }
            if let Some(v) = table.warnings_as_errors {
                out.warnings_as_errors = v;
            }
            out.defines.extend(table.defines.iter().flatten().cloned());
            out.include_dirs
                .extend(table.include_dirs.iter().flatten().cloned());
            out.links.extend(table.links.iter().flatten().cloned());
            out.lib_dirs
                .extend(table.lib_dirs.iter().flatten().cloned());
            out.link_flags
                .extend(table.link_flags.iter().flatten().cloned());
            if let Some(v) = table.runtime {
                out.runtime = v;
            }
            if let Some(v) = table.exceptions {
                out.exceptions = v;
            }
            if let Some(v) = table.rtti {
                out.rtti = v;
            }
        }
        out
    }

    /// `[profile.<profile>]` over the built-in defaults of `dev` and
    /// `release`.
    pub fn profile_settings(&self, profile: Profile) -> ProfileSettings {
        let default = match profile {
            Profile::Dev => ProfileSettings {
                opt_level: OptLevel::O0,
                debug_info: true,
                debug_assertions: true,
                lto: false,
            },
            Profile::Release => ProfileSettings {
                opt_level: OptLevel::O3,
                debug_info: false,
                debug_assertions: false,
                lto: false,
            },
        };
        let Some(table) = self.profiles.get(&profile) else {
            return default;
        };
        ProfileSettings {
            opt_level: table.opt_level.unwrap_or(default.opt_level),
            debug_info: table.debug_info.unwrap_or(default.debug_info),
            debug_assertions: table.debug_assertions.unwrap_or(default.debug_assertions),
            lto: table.lto.unwrap_or(default.lto),
        }
    }
}

#[cfg(test)]
mod tests;
