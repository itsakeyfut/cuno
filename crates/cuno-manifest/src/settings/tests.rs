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
