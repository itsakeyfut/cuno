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

/// Mutation: compare paths that do not exist as written, without resolving
/// `.` and `..`; this test fails.
#[test]
fn paths_that_do_not_exist_are_compared_after_resolving_dots() {
    let base = Path::new("no-such-dir-for-cuno");
    assert!(holds(&base.join("y").join(".."), &base.join("y").join("z")));
    assert!(holds(&base.join("."), base));
    assert!(!holds(&base.join("y").join("out"), base));
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
