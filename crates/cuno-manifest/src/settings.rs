//! One resolution of every setting from every layer, in the precedence
//! `docs/en/specs/manifest.md` (".cuno/config.toml", "Two levels of
//! settings") and `docs/en/specs/build.md` (the output directory) give:
//! command line, environment, `.cuno/config.toml`, `cuno.toml`, the
//! user-level `config.toml`, defaults.

use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use cuno_core::{ColorChoice, CunoError, Env, cuno_home};
use cuno_toolchain::llvm::ExplicitPaths;
use cuno_toolchain::select::{Choices, Constraints, ProjectChoices};

use crate::{
    CompilerName, ConfigError, ConfigWarning, Loaded, Manifest, OptLevel, Os, Profile, Project,
    Runtime, Tool, Warnings, find_project, load_config, load_manifest, resolve_path, shown_path,
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
    let home = cuno_home(env, cwd).map_err(|e| ConfigError {
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
    let user = load_config(&user_file, &shown_path(&user_file, cwd), &home)?;
    warnings.extend(user.warnings);
    let user = user.value;

    let manifest = project.as_ref().map(|(_, m, _)| m);
    let local = project.as_ref().map(|(_, _, c)| c);
    let from_env = |name| {
        env.get(name)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_owned())
    };

    // Highest first: command line, environment, project config, cuno.toml,
    // user config.
    let target_dir = cli
        .target_dir
        .clone()
        .map(|dir| (dir, cwd.to_owned()))
        .or_else(|| from_env("CUNO_TARGET_DIR").map(|dir| (dir.into(), cwd.to_owned())))
        .map(|(dir, base)| {
            resolve_path(&dir, &base).map_err(|e| ConfigError {
                message: format!("{} (the output directory)", e.message),
                ..e
            })
        })
        .transpose()?
        .or_else(|| local.and_then(|c| c.target_dir.clone()))
        .or_else(|| manifest.and_then(|m| m.target_dir.clone()))
        .or_else(|| user.target_dir.clone())
        .unwrap_or_else(|| {
            let base = project.as_ref().map_or(cwd, |(p, _, _)| &p.root);
            base.join("target")
        });

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

/// The build settings for one build, with every layer applied and the
/// defaults of `manifest.md` filled in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildSettings {
    /// `warnings`.
    pub warnings: Warnings,
    /// `warnings-as-errors`.
    pub warnings_as_errors: bool,
    /// `defines`.
    pub defines: Vec<String>,
    /// `include-dirs`.
    pub include_dirs: Vec<PathBuf>,
    /// `links`.
    pub links: Vec<String>,
    /// `lib-dirs`.
    pub lib_dirs: Vec<PathBuf>,
    /// `link-flags`.
    pub link_flags: Vec<String>,
    /// `[build.<compiler>] flags`.
    pub flags: Vec<String>,
    /// `runtime`.
    pub runtime: Runtime,
    /// `exceptions`.
    pub exceptions: bool,
    /// `rtti`.
    pub rtti: bool,
}

/// A profile's settings with the defaults of `manifest.md` filled in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProfileSettings {
    /// `opt-level`.
    pub opt_level: OptLevel,
    /// `debug-info`.
    pub debug_info: bool,
    /// `debug-assertions`.
    pub debug_assertions: bool,
    /// `lto`.
    pub lto: bool,
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
