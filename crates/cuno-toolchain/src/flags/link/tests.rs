use super::*;
use crate::flags::OptLevel;

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

/// Mutation: drop or reorder any default link argument of any driver, or
/// put the output after `/link`; this test fails.
#[test]
fn the_defaults_give_each_drivers_link_line() {
    let (b, p) = (build(), profile());
    assert_eq!(
        args(CompilerKind::Msvc, &b, &p, Subsystem::Console),
        ["/nologo", "a.obj", "b.obj", "/Fe:app.exe", "/link"]
    );
    assert_eq!(
        args(CompilerKind::ClangCl, &b, &p, Subsystem::Console),
        [
            "/nologo",
            "-fuse-ld=lld",
            "a.obj",
            "b.obj",
            "/Fe:app.exe",
            "/link"
        ]
    );
    assert_eq!(
        args(CompilerKind::Clang, &b, &p, Subsystem::Console),
        [
            "--target=x86_64-pc-windows-msvc",
            "-fuse-ld=lld",
            "-fms-runtime-lib=dll",
            "a.obj",
            "b.obj",
            "-o",
            "app.exe"
        ]
    );
}

/// Mutation: change or drop any row's link arguments for one driver
/// (`docs/en/specs/build.md`, "Settings and flags"), or put a linker
/// argument before `/link`; this test fails.
#[test]
fn every_setting_gives_the_tables_link_arguments() {
    let full_build = BuildSettings {
        links: vec!["user32".into()],
        lib_dirs: vec!["libs".into()],
        link_flags: vec!["/OPT:REF".into()],
        runtime: Runtime::Static,
        ..build()
    };
    let full_profile = ProfileSettings {
        debug_info: true,
        lto: true,
        ..profile()
    };
    assert_eq!(
        args(
            CompilerKind::Msvc,
            &full_build,
            &full_profile,
            Subsystem::Windows
        ),
        [
            "/nologo",
            "a.obj",
            "b.obj",
            "/Fe:app.exe",
            "/link",
            "user32.lib",
            "/LIBPATH:libs",
            "/SUBSYSTEM:WINDOWS",
            "/ENTRY:mainCRTStartup",
            "/DEBUG",
            "/LTCG",
            "/OPT:REF"
        ]
    );
    assert_eq!(
        args(
            CompilerKind::ClangCl,
            &full_build,
            &full_profile,
            Subsystem::Windows
        ),
        [
            "/nologo",
            "-fuse-ld=lld",
            "-flto",
            "a.obj",
            "b.obj",
            "/Fe:app.exe",
            "/link",
            "user32.lib",
            "/LIBPATH:libs",
            "/SUBSYSTEM:WINDOWS",
            "/ENTRY:mainCRTStartup",
            "/DEBUG",
            "/OPT:REF"
        ]
    );
    assert_eq!(
        args(
            CompilerKind::Clang,
            &full_build,
            &full_profile,
            Subsystem::Windows
        ),
        [
            "--target=x86_64-pc-windows-msvc",
            "-fuse-ld=lld",
            "-fms-runtime-lib=static",
            "-g",
            "-flto",
            "a.obj",
            "b.obj",
            "-o",
            "app.exe",
            "-luser32",
            "-Llibs",
            "-Wl,/subsystem:windows",
            "-Wl,/entry:mainCRTStartup",
            "-Wl,/OPT:REF"
        ]
    );
}
