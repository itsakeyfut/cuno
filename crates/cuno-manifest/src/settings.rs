//! One resolution of every setting from every layer, in the precedence
//! `docs/en/specs/manifest.md` (".cuno/config.toml", "Two levels of
//! settings") and `docs/en/specs/build.md` (the output directory) give:
//! command line, environment, `.cuno/config.toml`, `cuno.toml`, the
//! user-level `config.toml`, defaults.

use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use cuno_core::{ColorChoice, CunoError, Env, NameCase, cuno_home, paths};
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
    /// `[term] color` of the configuration files. It ranks below `--color`
    /// and above `NO_COLOR`, which are the binary's (`commands.md`,
    /// "Available for all commands").
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

/// The `[term]` settings, which every command applies before its work.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Term {
    /// `[term] color`.
    pub color: Option<ColorChoice>,
    /// `[term] verbose`.
    pub verbose: bool,
}

/// The cuno home, its error as a configuration error.
fn home_dir(env: &Env) -> Result<PathBuf, ConfigError> {
    cuno_home(env).map_err(|e| ConfigError {
        message: e.to_string(),
        location: None,
        help: e.help(),
    })
}

/// What `cuno new` reads: the `[term]` of the user's
/// `<cuno home>/config.toml` alone, not any project around `cwd`, which is
/// not the new package's (`docs/en/specs/commands.md`, "Available for all
/// commands"). A file that cannot be read is an error, as for every
/// command; `cwd` only shapes how its path is shown.
pub fn resolve_user(env: &Env, cwd: &Path) -> Result<Loaded<Term>, ConfigError> {
    let home = home_dir(env)?;
    let file = home.join("config.toml");
    let user = load_config(&file, &shown_path(&file, cwd), &home)?;
    Ok(Loaded {
        value: Term {
            color: user.value.color,
            verbose: user.value.verbose.unwrap_or(false),
        },
        warnings: user.warnings,
    })
}

/// Reads every layer and resolves every setting. Warnings from all files
/// come back together: `cuno.toml`'s, then `.cuno/config.toml`'s, then the
/// user-level file's.
pub fn resolve(inputs: Inputs<'_>) -> Result<Loaded<Settings>, ConfigError> {
    let Inputs { cwd, env, cli } = inputs;
    let case = env.case();
    let home = home_dir(env)?;
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
        .is_some_and(|(p, _, _)| paths::same(&p.config(), &user_file, case));
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
        check_output_dir(&resolved, name, None, guarded, project.is_some(), case)?;
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
            case,
        )?;
        // A file a cloned project carries must not make `cuno clean`
        // delete what lies outside it (build.md, "Output directory"). The
        // user's own config, the environment and the command line may.
        if carried_by_project && !paths::within(&dir.path, guarded, case) {
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
    case: NameCase,
) -> Result<(), ConfigError> {
    if !paths::within(guarded, dir, case) {
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
mod tests {
    use std::path::Path;

    use cuno_core::NameCase;

    use super::*;

    const MANIFEST: &str = "[package]\nname = \"app\"\nversion = \"0.1.0\"\n";

    /// A temporary project, a cuno home beside it, and a directory outside both.
    struct Fixture {
        _dir: tempfile::TempDir,
        root: PathBuf,
        home: PathBuf,
        outside: PathBuf,
    }

    impl Fixture {
        fn new() -> Fixture {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().join("project");
            let home = dir.path().join("home");
            let outside = dir.path().join("outside");
            for d in [&root, &home, &outside] {
                std::fs::create_dir_all(d).unwrap();
            }
            let fixture = Fixture {
                _dir: dir,
                root,
                home,
                outside,
            };
            fixture.write("project/cuno.toml", MANIFEST);
            fixture
        }

        fn write(&self, relative: &str, text: &str) {
            let path = self.root.parent().unwrap().join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }

        fn env(&self, extra: &[(&str, &str)]) -> Env {
            Env::from_pairs(
                [("CUNO_HOME".into(), self.home.clone().into_os_string())]
                    .into_iter()
                    .chain(extra.iter().map(|(k, v)| ((*k).into(), (*v).into()))),
                NameCase::Insensitive,
            )
        }

        fn resolve(&self, cwd: &Path, env: &Env, cli: &CliSettings) -> Loaded<Settings> {
            resolve(Inputs { cwd, env, cli }).unwrap()
        }
    }

    /// Mutation: swap any two levels of the output directory's precedence,
    /// resolve the command line's or `CUNO_TARGET_DIR`'s relative path against
    /// anything but the current directory, or change the default; this test
    /// fails.
    #[test]
    fn each_output_directory_level_beats_every_level_below_it() {
        // Highest first. Each level gives its own name; whichever wins tells
        // which level won.
        let levels = ["cli", "env", "local", "manifest", "user"];
        for top in 0..=levels.len() {
            let f = Fixture::new();
            let cwd = f.root.join("src");
            std::fs::create_dir_all(&cwd).unwrap();
            let set = |i: usize| i >= top;
            let cli = CliSettings {
                target_dir: set(0).then(|| "cli".into()),
                ..Default::default()
            };
            let env_pairs: Vec<(&str, &str)> = if set(1) {
                vec![("CUNO_TARGET_DIR", "env")]
            } else {
                vec![]
            };
            if set(2) {
                f.write(
                    "project/.cuno/config.toml",
                    "[build]\ntarget-dir = \"local\"\n",
                );
            }
            f.write(
                "project/cuno.toml",
                &if set(3) {
                    format!("{MANIFEST}[build]\ntarget-dir = \"manifest\"\n")
                } else {
                    MANIFEST.to_owned()
                },
            );
            if set(4) {
                f.write("home/config.toml", "[build]\ntarget-dir = \"user\"\n");
            }
            let settings = f.resolve(&cwd, &f.env(&env_pairs), &cli).value;
            let expected = match levels.get(top) {
                Some(&"cli") => cwd.join("cli"),
                Some(&"env") => cwd.join("env"),
                Some(&"local") => f.root.join("local"),
                Some(&"manifest") => f.root.join("manifest"),
                Some(&"user") => f.home.join("user"),
                _ => f.root.join("target"),
            };
            assert_eq!(settings.target_dir, expected, "top level {top}");
        }
    }

    /// Mutation: swap the precedence of `jobs`, `color`, `verbose` or a tool
    /// path, merge `[toolchain.paths]` as a whole table instead of per tool, or
    /// change a default; this test fails.
    #[test]
    fn the_other_settings_follow_the_same_order() {
        let f = Fixture::new();
        f.write(
            "project/.cuno/config.toml",
            "[build]\njobs = 4\n[term]\ncolor = \"never\"\n[toolchain.paths]\nclang = \"local-clang\"\n",
        );
        f.write(
            "home/config.toml",
            "[build]\njobs = 8\n[term]\ncolor = \"always\"\nverbose = true\n[toolchain.paths]\nclang = \"user-clang\"\nclang-tidy = \"user-tidy\"\nmsvc = \"vs\"\n",
        );
        let s = f
            .resolve(&f.root, &f.env(&[]), &CliSettings::default())
            .value;
        assert_eq!(s.jobs, NonZeroU32::new(4));
        assert_eq!(s.color, Some(ColorChoice::Never));
        assert!(s.verbose);
        let llvm = s.llvm_paths();
        assert_eq!(llvm.clang, Some(f.root.join("local-clang")));
        assert_eq!(llvm.clang_tidy, Some(f.home.join("user-tidy")));
        assert_eq!(llvm.clang_cl, None);
        assert_eq!(s.msvc_path(), Some(f.home.join("vs").as_path()));

        let cli = CliSettings {
            jobs: NonZeroU32::new(2),
            ..Default::default()
        };
        assert_eq!(
            f.resolve(&f.root, &f.env(&[]), &cli).value.jobs,
            NonZeroU32::new(2)
        );

        let bare = Fixture::new();
        let s = bare
            .resolve(&bare.root, &bare.env(&[]), &CliSettings::default())
            .value;
        assert_eq!((s.jobs, s.color, s.verbose), (None, None, false));
        assert!(s.paths.is_empty());
    }

    /// Mutation: put any layer's compiler in another layer's place, read an
    /// empty `CUNO_COMPILER` as a name, or take `[toolchain.version]` from
    /// anywhere but `cuno.toml`; this test fails.
    #[test]
    fn every_layer_gives_its_compiler_to_the_selector() {
        let f = Fixture::new();
        f.write(
            "project/cuno.toml",
            &format!(
                "{MANIFEST}[toolchain]\ncompiler = \"msvc\"\n[toolchain.version]\nclang = \">=18\"\n"
            ),
        );
        f.write(
            "project/.cuno/config.toml",
            "[toolchain]\ncompiler = \"clang-cl\"\n",
        );
        f.write("home/config.toml", "[toolchain]\ncompiler = \"clang\"\n");
        let cli = CliSettings {
            compiler: Some("gcc".into()),
            ..Default::default()
        };
        let s = f
            .resolve(&f.root, &f.env(&[("CUNO_COMPILER", "safec")]), &cli)
            .value;
        let project = s.compiler.project.as_ref().unwrap();
        assert_eq!(
            (
                s.compiler.cli.as_deref(),
                s.compiler.env.as_deref(),
                project.config.as_deref(),
                project.manifest.as_deref(),
                s.compiler.user.as_deref(),
            ),
            (
                Some("gcc"),
                Some("safec"),
                Some("clang-cl"),
                Some("msvc"),
                Some("clang")
            )
        );
        assert_eq!(s.constraints.clang.unwrap().to_string(), ">=18");
        let s = f
            .resolve(
                &f.root,
                &f.env(&[("CUNO_COMPILER", "")]),
                &CliSettings::default(),
            )
            .value;
        assert_eq!(s.compiler.env, None);
    }

    /// Mutation: read a project's files from outside it, or base the default
    /// output directory on anything but the current directory there; this test
    /// fails.
    #[test]
    fn outside_a_project_only_the_user_layer_and_above_apply() {
        let f = Fixture::new();
        f.write(
            "outside/.cuno/config.toml",
            "[toolchain]\ncompiler = \"clang-cl\"\n",
        );
        f.write("home/config.toml", "[toolchain]\ncompiler = \"clang\"\n");
        let s = f
            .resolve(&f.outside, &f.env(&[]), &CliSettings::default())
            .value;
        assert!(s.project.is_none());
        assert!(s.compiler.project.is_none());
        assert_eq!(s.compiler.user.as_deref(), Some("clang"));
        assert_eq!(s.target_dir, f.outside.join("target"));
        assert_eq!(s.constraints, Constraints::default());
    }

    /// Mutation: drop any file's warnings, or report them in another order;
    /// this test fails.
    #[test]
    fn warnings_from_every_file_come_back_in_order() {
        let f = Fixture::new();
        f.write("project/cuno.toml", &format!("{MANIFEST}a = 1\n"));
        f.write("project/.cuno/config.toml", "b = 1\n");
        f.write("home/config.toml", "c = 1\n");
        let warnings = f
            .resolve(&f.root, &f.env(&[]), &CliSettings::default())
            .warnings;
        let keys: Vec<&str> = warnings
            .iter()
            .map(|w| w.message.split('`').nth(1).unwrap())
            .collect();
        assert_eq!(keys, ["package.a", "b", "c"]);
        assert!(warnings[1].to_string().contains(".cuno"), "{}", warnings[1]);
    }

    /// Mutation: invent a home when none can be found; this test fails.
    #[test]
    fn without_a_home_resolution_is_a_setup_error() {
        let f = Fixture::new();
        let env = Env::from_pairs([], NameCase::Insensitive);
        let err = resolve(Inputs {
            cwd: &f.root,
            env: &env,
            cli: &CliSettings::default(),
        })
        .unwrap_err();
        assert!(err.help.unwrap().contains("CUNO_HOME"));
    }

    fn layered() -> Manifest {
        let text = concat!(
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
            "[build]\nwarnings = \"pedantic\"\ndefines = [\"B\"]\nrtti = false\n",
            "[build.clang]\nflags = [\"-fc\"]\n",
            "[build.msvc]\nflags = [\"/m\"]\n",
            "[platform.windows.build]\ndefines = [\"W\"]\nwarnings = \"default\"\nruntime = \"static\"\n",
            "[profile.release.build]\ndefines = [\"R\"]\nwarnings = \"all\"\n",
            "[profile.release]\nopt-level = 2\nlto = true\n",
        );
        crate::parse_manifest(text, Path::new("cuno.toml"), Path::new("r"))
            .unwrap()
            .value
    }

    /// Mutation: override a list instead of appending it, append a single value
    /// instead of overriding it, take the layers in another order, take another
    /// compiler's flags, or change a default; this test fails.
    #[test]
    fn build_settings_are_layered_in_the_documented_order() {
        let m = layered();
        let release = m.build_settings(Os::Windows, CompilerName::Clang, Profile::Release);
        assert_eq!(release.defines, ["B", "W", "R"]);
        assert_eq!(release.warnings, Warnings::All);
        assert_eq!(release.runtime, Runtime::Static);
        assert_eq!(release.flags, ["-fc"]);
        assert!(!release.rtti);
        assert!(release.exceptions);
        let dev = m.build_settings(Os::Linux, CompilerName::Msvc, Profile::Dev);
        assert_eq!(dev.defines, ["B"]);
        assert_eq!(dev.warnings, Warnings::Pedantic);
        assert_eq!(dev.runtime, Runtime::Dynamic);
        assert_eq!(dev.flags, ["/m"]);
        let windows_dev = m.build_settings(Os::Windows, CompilerName::ClangCl, Profile::Dev);
        assert_eq!(windows_dev.warnings, Warnings::Default);
        assert!(windows_dev.flags.is_empty());

        let bare = crate::parse_manifest(MANIFEST, Path::new("cuno.toml"), Path::new("r"))
            .unwrap()
            .value;
        assert_eq!(
            bare.build_settings(Os::Windows, CompilerName::Msvc, Profile::Dev),
            BuildSettings {
                warnings: Warnings::All,
                warnings_as_errors: false,
                defines: vec![],
                include_dirs: vec![],
                links: vec![],
                lib_dirs: vec![],
                link_flags: vec![],
                flags: vec![],
                runtime: Runtime::Dynamic,
                exceptions: true,
                rtti: true,
            }
        );
    }

    /// Mutation: change a profile's default, or ignore what `[profile.<name>]`
    /// says; this test fails.
    #[test]
    fn profiles_start_from_their_defaults() {
        let m = layered();
        assert_eq!(
            m.profile_settings(Profile::Dev),
            ProfileSettings {
                opt_level: OptLevel::O0,
                debug_info: true,
                debug_assertions: true,
                lto: false,
            }
        );
        assert_eq!(
            m.profile_settings(Profile::Release),
            ProfileSettings {
                opt_level: OptLevel::O2,
                debug_info: false,
                debug_assertions: false,
                lto: true,
            }
        );
        let bare = crate::parse_manifest(MANIFEST, Path::new("cuno.toml"), Path::new("r"))
            .unwrap()
            .value;
        assert_eq!(
            bare.profile_settings(Profile::Release),
            ProfileSettings {
                opt_level: OptLevel::O3,
                debug_info: false,
                debug_assertions: false,
                lto: false,
            }
        );
    }

    fn resolve_err(_f: &Fixture, cwd: &Path, env: &Env, cli: &CliSettings) -> ConfigError {
        resolve(Inputs { cwd, env, cli }).unwrap_err()
    }

    /// Mutation: let the output directory be the project root or above it, by
    /// any layer or spelling, or name another source in the message; this test
    /// fails.
    #[test]
    fn the_output_directory_never_holds_the_project() {
        let f = Fixture::new();
        let src = f.root.join("src");
        std::fs::create_dir_all(&src).unwrap();
        let none = CliSettings::default();

        f.write(
            "project/cuno.toml",
            &format!("{MANIFEST}[build]\ntarget-dir = \".\"\n"),
        );
        let err = resolve_err(&f, &f.root, &f.env(&[]), &none);
        assert!(
            err.message
                .contains("(`build.target-dir` in cuno.toml) is the project root"),
            "{err}"
        );
        assert!(err.help.unwrap().contains("cuno clean"));
        f.write("project/cuno.toml", MANIFEST);

        f.write(
            "project/.cuno/config.toml",
            "[build]\ntarget-dir = \"a/../..\"\n",
        );
        let err = resolve_err(&f, &f.root, &f.env(&[]), &none);
        assert!(err.message.contains(".cuno"), "{err}");
        std::fs::remove_file(f.root.join(".cuno/config.toml")).unwrap();

        let cli = |dir: &str| CliSettings {
            target_dir: Some(dir.into()),
            ..Default::default()
        };
        let err = resolve_err(&f, &src, &f.env(&[]), &cli(".."));
        assert!(
            err.message.contains("(--target-dir) is the project root"),
            "{err}"
        );
        let err = resolve_err(&f, &f.outside, &f.env(&[]), &cli("."));
        assert!(err.message.contains("is the current directory"), "{err}");
        let err = resolve_err(&f, &src, &f.env(&[("CUNO_TARGET_DIR", "..")]), &none);
        assert!(err.message.contains("(CUNO_TARGET_DIR)"), "{err}");

        // Inside the project, below its root, is fine.
        let s = f.resolve(&src, &f.env(&[]), &cli("out")).value;
        assert_eq!(s.target_dir, src.join("out"));
    }

    /// Mutation: accept an empty output directory from the command line or a
    /// file, or describe a bad command-line path as written in a file; this
    /// test fails.
    #[test]
    fn an_empty_or_drive_dependent_output_directory_is_an_error() {
        let f = Fixture::new();
        let src = f.root.join("src");
        std::fs::create_dir_all(&src).unwrap();
        let cli = CliSettings {
            target_dir: Some("".into()),
            ..Default::default()
        };
        let err = resolve_err(&f, &src, &f.env(&[]), &cli);
        assert_eq!(err.message, "--target-dir is ``, which is empty");
        assert!(
            err.help
                .unwrap()
                .contains("relative to the current directory")
        );

        f.write(
            "project/.cuno/config.toml",
            "\n[build]\ntarget-dir = \"\"\n",
        );
        let err = resolve_err(&f, &f.root, &f.env(&[]), &CliSettings::default());
        assert!(err.message.contains("is empty"), "{err}");
        assert_eq!(err.location.unwrap().line, 3);
        std::fs::remove_file(f.root.join(".cuno/config.toml")).unwrap();

        f.write(
            "project/cuno.toml",
            &format!("{MANIFEST}[build]\ntarget-dir = \"\"\n"),
        );
        let err = resolve_err(&f, &f.root, &f.env(&[]), &CliSettings::default());
        assert_eq!(err.location.unwrap().line, 5);
        f.write("project/cuno.toml", MANIFEST);

        if cfg!(windows) {
            let err = resolve_err(
                &f,
                &src,
                &f.env(&[("CUNO_TARGET_DIR", "C:x")]),
                &CliSettings::default(),
            );
            assert_eq!(
                err.message,
                "CUNO_TARGET_DIR is `C:x`, which depends on the current drive"
            );
        }
    }

    /// Mutation: read the project's `.cuno/config.toml` again as the user-level
    /// file when the cuno home is that `.cuno`; this test fails.
    #[test]
    fn one_file_serving_both_layers_is_read_once() {
        let f = Fixture::new();
        f.write("project/.cuno/config.toml", "[build]\nnope = 1\n");
        let home = f.root.join(".cuno");
        let env = Env::from_pairs(
            [("CUNO_HOME".into(), home.into_os_string())],
            NameCase::Insensitive,
        );
        let loaded = f.resolve(&f.root, &env, &CliSettings::default());
        assert_eq!(loaded.warnings.len(), 1, "{:?}", loaded.warnings);
    }

    /// Mutation: lose why the cuno home cannot be used; this test fails.
    #[test]
    fn a_relative_cuno_home_stops_resolution() {
        let f = Fixture::new();
        let env = Env::from_pairs([("CUNO_HOME".into(), "rel".into())], NameCase::Insensitive);
        let err = resolve_err(&f, &f.root, &env, &CliSettings::default());
        assert_eq!(
            err.message,
            "CUNO_HOME is `rel`, which is not an absolute path"
        );
        assert!(err.help.unwrap().contains("absolute path"));
    }

    /// Mutation: drop the project's `verbose` in favour of the user's, or leave
    /// out the clang-format path; this test fails.
    #[test]
    fn the_project_config_wins_for_verbose_and_every_tool() {
        let f = Fixture::new();
        f.write(
            "project/.cuno/config.toml",
            "[term]\nverbose = false\n[toolchain.paths]\nclang-format = \"fmt\"\n",
        );
        f.write("home/config.toml", "[term]\nverbose = true\n");
        let s = f
            .resolve(&f.root, &f.env(&[]), &CliSettings::default())
            .value;
        assert!(!s.verbose);
        assert_eq!(s.llvm_paths().clang_format, Some(f.root.join("fmt")));
    }

    /// Mutation: drop any list from the layering, or let a layer's boolean be
    /// ignored; this test fails.
    #[test]
    fn every_build_key_is_layered() {
        let text = concat!(
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
            "[build]\ninclude-dirs = [\"i1\"]\nlinks = [\"l1\"]\nlib-dirs = [\"d1\"]\nlink-flags = [\"f1\"]\nwarnings-as-errors = true\nexceptions = false\n",
            "[platform.windows.build]\ninclude-dirs = [\"i2\"]\nlinks = [\"l2\"]\nlib-dirs = [\"d2\"]\nlink-flags = [\"f2\"]\n",
            "[profile.dev.build]\ninclude-dirs = [\"i3\"]\nwarnings-as-errors = false\nexceptions = true\n",
        );
        let m = crate::parse_manifest(text, Path::new("cuno.toml"), Path::new("r"))
            .unwrap()
            .value;
        let r = Path::new("r");
        let dev = m.build_settings(Os::Windows, CompilerName::Msvc, Profile::Dev);
        assert_eq!(dev.include_dirs, [r.join("i1"), r.join("i2"), r.join("i3")]);
        assert_eq!(dev.links, ["l1", "l2"]);
        assert_eq!(dev.lib_dirs, [r.join("d1"), r.join("d2")]);
        assert_eq!(dev.link_flags, ["f1", "f2"]);
        assert!(!dev.warnings_as_errors);
        assert!(dev.exceptions);
        let release = m.build_settings(Os::Linux, CompilerName::Msvc, Profile::Release);
        assert!(release.warnings_as_errors);
        assert!(!release.exceptions);
        assert_eq!(release.include_dirs, [r.join("i1")]);
    }

    /// Mutation: ignore `debug-info` or `debug-assertions` in either profile;
    /// this test fails.
    #[test]
    fn every_profile_key_overrides_its_default() {
        let text = concat!(
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
            "[profile.dev]\nopt-level = 3\ndebug-info = false\ndebug-assertions = false\n",
            "[profile.release]\ndebug-info = true\ndebug-assertions = true\n",
        );
        let m = crate::parse_manifest(text, Path::new("cuno.toml"), Path::new("r"))
            .unwrap()
            .value;
        let dev = m.profile_settings(Profile::Dev);
        assert_eq!(
            (dev.opt_level, dev.debug_info, dev.debug_assertions),
            (OptLevel::O3, false, false)
        );
        let release = m.profile_settings(Profile::Release);
        assert_eq!((release.debug_info, release.debug_assertions), (true, true));
    }

    /// Every layer that sets an output directory is checked, winning or not:
    /// `"."` in `cuno.toml` and `""` in `.cuno/config.toml` are errors even
    /// when the command line or the environment wins.
    ///
    /// Mutation: check only the winning layer, or stop at the first layer
    /// that sets one; this test fails.
    #[test]
    fn a_losing_layer_is_checked_too() {
        let f = Fixture::new();
        let cli = CliSettings {
            target_dir: Some("out".into()),
            ..Default::default()
        };
        f.write(
            "project/cuno.toml",
            &format!("{MANIFEST}[build]\ntarget-dir = \".\"\n"),
        );
        let err = resolve_err(&f, &f.root, &f.env(&[]), &cli);
        assert!(
            err.message
                .contains("(`build.target-dir` in cuno.toml) is the project root"),
            "{err}"
        );
        assert_eq!(err.location.unwrap().line, 5);
        f.write("project/cuno.toml", MANIFEST);

        f.write("project/.cuno/config.toml", "[build]\ntarget-dir = \"\"\n");
        let err = resolve_err(
            &f,
            &f.root,
            &f.env(&[("CUNO_TARGET_DIR", "out")]),
            &CliSettings::default(),
        );
        assert!(err.message.ends_with("is empty"), "{err}");
        assert_eq!(err.location.unwrap().line, 2);
    }

    /// A file the project carries may not put the output outside the project,
    /// from `cuno.toml` or `.cuno/config.toml`, winning or not; the user's own
    /// config, the environment and the command line may.
    ///
    /// Mutation: accept a project file's directory outside the project, check
    /// only `.cuno/config.toml`, or refuse the user config; this test fails.
    #[test]
    fn only_the_users_own_layers_may_point_outside_the_project() {
        let f = Fixture::new();
        let none = CliSettings::default();
        for (file, text) in [
            (
                "project/cuno.toml",
                format!("{MANIFEST}[build]\ntarget-dir = \"../out\"\n"),
            ),
            (
                "project/.cuno/config.toml",
                "[build]\ntarget-dir = \"../out\"\n".to_owned(),
            ),
        ] {
            f.write(file, &text);
            let err = resolve_err(&f, &f.root, &f.env(&[("CUNO_TARGET_DIR", "out")]), &none);
            assert!(
                err.message.contains("points outside the project: "),
                "{file}: {err}"
            );
            assert!(err.location.is_some(), "{file}");
            assert!(
                err.help
                    .as_deref()
                    .unwrap()
                    .contains("user-level config, CUNO_TARGET_DIR or --target-dir"),
                "{file}"
            );
            if file.ends_with("cuno.toml") {
                f.write(file, MANIFEST);
            } else {
                std::fs::remove_file(f.root.join(".cuno/config.toml")).unwrap();
            }
        }

        let outside = f.outside.join("out");
        let written = outside.display().to_string().replace('\\', "/");
        f.write(
            "home/config.toml",
            &format!("[build]\ntarget-dir = \"{written}\"\n"),
        );
        assert_eq!(
            f.resolve(&f.root, &f.env(&[]), &none).value.target_dir,
            outside
        );
        std::fs::remove_file(f.home.join("config.toml")).unwrap();
        let s = f
            .resolve(&f.root, &f.env(&[("CUNO_TARGET_DIR", &written)]), &none)
            .value;
        assert_eq!(s.target_dir, outside);
        let cli = CliSettings {
            target_dir: Some(outside.clone()),
            ..Default::default()
        };
        assert_eq!(
            f.resolve(&f.root, &f.env(&[]), &cli).value.target_dir,
            outside
        );
    }

    /// The user's config and the environment are checked as the project's
    /// files are, winning or not: a user config naming a directory above the
    /// project is refused with its file and line, and so is `CUNO_TARGET_DIR`
    /// at the root when the command line wins. An empty `target-dir` in
    /// `cuno.toml` says it is empty.
    ///
    /// Mutation: check only the project's files, skip the environment when the
    /// command line is given, name no file for the user config, or let an
    /// empty `cuno.toml` value fall through to the root check; this test fails.
    #[test]
    fn the_users_layers_are_checked_too() {
        let f = Fixture::new();
        let above = f
            .root
            .parent()
            .unwrap()
            .display()
            .to_string()
            .replace('\\', "/");
        f.write(
            "home/config.toml",
            &format!("[build]\ntarget-dir = \"{above}\"\n"),
        );
        let err = resolve_err(&f, &f.root, &f.env(&[]), &CliSettings::default());
        assert!(
            err.message.contains("config.toml) is the project root"),
            "{err}"
        );
        assert_eq!(err.location.unwrap().line, 2);
        std::fs::remove_file(f.home.join("config.toml")).unwrap();

        let cli = CliSettings {
            target_dir: Some("out".into()),
            ..Default::default()
        };
        let err = resolve_err(&f, &f.root, &f.env(&[("CUNO_TARGET_DIR", ".")]), &cli);
        assert!(err.message.contains("(CUNO_TARGET_DIR)"), "{err}");

        f.write(
            "project/cuno.toml",
            &format!("{MANIFEST}[build]\ntarget-dir = \"\"\n"),
        );
        let err = resolve_err(&f, &f.root, &f.env(&[]), &cli);
        assert_eq!(err.message, "`build.target-dir` in cuno.toml is empty");
    }

    /// A directory inside the project, spelled in another case or not made
    /// yet, is inside it: the comparison is the file system's, not the
    /// spelling's.
    ///
    /// Mutation: compare the part that does not exist yet as written, with
    /// case; this test fails.
    #[cfg(windows)]
    #[test]
    fn a_directory_inside_the_project_in_another_case_is_inside() {
        let f = Fixture::new();
        let upper = f
            .root
            .display()
            .to_string()
            .replace('\\', "/")
            .to_uppercase();
        f.write(
            "project/cuno.toml",
            &format!("{MANIFEST}[build]\ntarget-dir = \"{upper}/out\"\n"),
        );
        let s = f.resolve(&f.root, &f.env(&[]), &CliSettings::default());
        assert!(!f.root.join("out").exists());
        assert!(
            s.value.target_dir.to_string_lossy().ends_with("out"),
            "{}",
            s.value.target_dir.display()
        );
    }

    /// Makes `link` a junction to `target`, which needs no privilege on Windows.
    #[cfg(windows)]
    fn junction(link: &Path, target: &Path) {
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .unwrap();
        assert!(made.status.success(), "{made:?}");
    }

    /// An output directory written inside the project that reaches outside it
    /// through a junction is outside the project, and a project file may not
    /// set it (`project-layout.md`, "Source collection and linking").
    ///
    /// Mutation: compare the output directory as written, without following
    /// links; this test fails.
    #[cfg(windows)]
    #[test]
    fn a_junction_inside_the_project_to_outside_it_is_outside() {
        let f = Fixture::new();
        junction(&f.root.join("link"), &f.outside);
        f.write(
            "project/cuno.toml",
            &format!(
                "{MANIFEST}[build]
target-dir = \"link/out\"
"
            ),
        );
        let err = resolve_err(&f, &f.root, &f.env(&[]), &CliSettings::default());
        assert!(
            err.message.contains("points outside the project: "),
            "{err}"
        );
    }
}
