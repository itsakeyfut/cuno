use super::*;
use crate::flags::Runtime;

fn build() -> BuildSettings {
    BuildSettings {
        warnings: crate::flags::Warnings::Default,
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
}

fn profile() -> ProfileSettings {
    ProfileSettings {
        opt_level: OptLevel::O0,
        debug_info: false,
        debug_assertions: true,
        lto: false,
    }
}

fn args(
    kind: CompilerKind,
    build: &BuildSettings,
    profile: &ProfileSettings,
    subsystem: Subsystem,
) -> Vec<String> {
    let input = LinkInput {
        kind,
        build,
        profile,
        subsystem,
    };
    link_args(
        &input,
        &[PathBuf::from("a.obj"), PathBuf::from("b.obj")],
        Path::new("app.exe"),
    )
    .into_iter()
    .map(|a| a.into_string().unwrap())
    .collect()
}

/// Mutation: drop or reorder any default link argument of any driver, leave
/// the console subsystem unstated, pass the C runtime at link time, or put
/// the output after `/link`; this test fails.
#[test]
fn the_defaults_give_each_drivers_link_line() {
    let (b, p) = (build(), profile());
    assert_eq!(
        args(CompilerKind::Msvc, &b, &p, Subsystem::Console),
        [
            "/nologo",
            "a.obj",
            "b.obj",
            "/Fe:app.exe",
            "/link",
            "/SUBSYSTEM:CONSOLE"
        ]
    );
    assert_eq!(
        args(CompilerKind::ClangCl, &b, &p, Subsystem::Console),
        [
            "/nologo",
            "-fuse-ld=lld",
            "a.obj",
            "b.obj",
            "/Fe:app.exe",
            "/link",
            "/SUBSYSTEM:CONSOLE"
        ]
    );
    let static_runtime = BuildSettings {
        runtime: Runtime::Static,
        ..build()
    };
    for b in [&b, &static_runtime] {
        assert_eq!(
            args(CompilerKind::Clang, b, &p, Subsystem::Console),
            [
                "--target=x86_64-pc-windows-msvc",
                "-fuse-ld=lld",
                "a.obj",
                "b.obj",
                "-o",
                "app.exe",
                "-Wl,/subsystem:console"
            ]
        );
    }
}

/// Mutation: change or drop any row's link arguments for one driver
/// (`docs/en/specs/build.md`, "Settings and flags"), reorder several
/// libraries, paths or flags, split a flag at its comma, or put a linker
/// argument before `/link`; this test fails.
#[test]
fn every_setting_gives_the_tables_link_arguments() {
    let full_build = BuildSettings {
        links: vec!["user32".into(), "gdi32".into()],
        lib_dirs: vec!["libs".into(), "more".into()],
        link_flags: vec!["/OPT:REF,ICF".into(), "/STACK:2097152".into()],
        ..build()
    };
    let full_profile = ProfileSettings {
        opt_level: OptLevel::O3,
        debug_info: true,
        lto: true,
        ..profile()
    };
    let w = Subsystem::Windows;
    assert_eq!(
        args(CompilerKind::Msvc, &full_build, &full_profile, w),
        [
            "/nologo",
            "a.obj",
            "b.obj",
            "/Fe:app.exe",
            "/link",
            "user32.lib",
            "gdi32.lib",
            "/LIBPATH:libs",
            "/LIBPATH:more",
            "/SUBSYSTEM:WINDOWS",
            "/ENTRY:mainCRTStartup",
            "/DEBUG",
            "/INCREMENTAL:NO",
            "/LTCG",
            "/OPT:REF,ICF",
            "/STACK:2097152"
        ]
    );
    assert_eq!(
        args(CompilerKind::ClangCl, &full_build, &full_profile, w),
        [
            "/nologo",
            "-fuse-ld=lld",
            "-flto",
            "a.obj",
            "b.obj",
            "/Fe:app.exe",
            "/link",
            "user32.lib",
            "gdi32.lib",
            "/LIBPATH:libs",
            "/LIBPATH:more",
            "/SUBSYSTEM:WINDOWS",
            "/ENTRY:mainCRTStartup",
            "/DEBUG",
            "/opt:lldlto=3",
            "/OPT:REF,ICF",
            "/STACK:2097152"
        ]
    );
    assert_eq!(
        args(CompilerKind::Clang, &full_build, &full_profile, w),
        [
            "--target=x86_64-pc-windows-msvc",
            "-fuse-ld=lld",
            "-g",
            "-flto",
            "a.obj",
            "b.obj",
            "-o",
            "app.exe",
            "-luser32",
            "-lgdi32",
            "-Llibs",
            "-Lmore",
            "-Wl,/subsystem:windows",
            "-Wl,/entry:mainCRTStartup",
            "-Wl,/opt:lldlto=3",
            "-Xlinker",
            "/OPT:REF,ICF",
            "-Xlinker",
            "/STACK:2097152"
        ]
    );
}

/// Mutation: map any `opt-level` to another LTO level; this test fails.
#[test]
fn lto_optimises_at_the_profiles_level() {
    for (level, n) in [
        (OptLevel::O0, "0"),
        (OptLevel::O1, "1"),
        (OptLevel::O2, "2"),
        (OptLevel::O3, "3"),
        (OptLevel::Size, "2"),
    ] {
        let p = ProfileSettings {
            opt_level: level,
            lto: true,
            ..profile()
        };
        let clang_cl = args(CompilerKind::ClangCl, &build(), &p, Subsystem::Console);
        assert!(
            clang_cl.contains(&format!("/opt:lldlto={n}")),
            "{level:?} {clang_cl:?}"
        );
        let clang = args(CompilerKind::Clang, &build(), &p, Subsystem::Console);
        assert!(
            clang.contains(&format!("-Wl,/opt:lldlto={n}")),
            "{level:?} {clang:?}"
        );
        let msvc = args(CompilerKind::Msvc, &build(), &p, Subsystem::Console);
        assert!(!msvc.iter().any(|a| a.contains("lldlto")), "{msvc:?}");
    }
}
