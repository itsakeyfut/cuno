use super::*;
use cuno_toolchain::flags::compile_flags;

fn manifest(toml: &str) -> Manifest {
    cuno_manifest::parse_manifest(toml, Path::new("cuno.toml"), Path::new("C:/p"))
        .unwrap()
        .value
}

const LAYERED: &str = r#"
[package]
name = "app"
version = "0.1.0"
edition = "c++17"

[build]
defines = ["SHARED"]

[platform.windows.build]
defines = ["ON_WINDOWS"]

[platform.linux.build]
defines = ["ON_LINUX"]

[build.clang-cl]
flags = ["/DCLANG_CL_ONLY"]

[build.msvc]
flags = ["/DMSVC_ONLY"]

[profile.release.build]
defines = ["RELEASE_ONLY"]
"#;

fn settings(m: &Manifest, kind: CompilerKind, profile: Profile, lang: Lang) -> CompileSettings {
    compile_settings(
        Some((Path::new("C:/p"), m)),
        kind,
        profile,
        lang,
        HostOs::Windows,
        false,
        Mode::Compile,
    )
}

/// Mutation: drop any layer (`[build]`, the host's `[platform]`, the
/// compiler's own table, the profile's), take another OS's or another
/// compiler's table, or leave `src/` off the include path; this test fails.
#[test]
fn every_layer_for_this_host_compiler_and_profile_applies() {
    let m = manifest(LAYERED);
    let dev = settings(&m, CompilerKind::ClangCl, Profile::Dev, Lang::Cxx);
    assert_eq!(dev.build.defines, ["SHARED", "ON_WINDOWS"]);
    assert_eq!(dev.build.flags, ["/DCLANG_CL_ONLY"]);
    assert_eq!(dev.include_dirs, [PathBuf::from("C:/p/src")]);
    assert_eq!(dev.profile, m.profile_settings(Profile::Dev));

    let release = settings(&m, CompilerKind::Msvc, Profile::Release, Lang::Cxx);
    assert_eq!(
        release.build.defines,
        ["SHARED", "ON_WINDOWS", "RELEASE_ONLY"]
    );
    assert_eq!(release.build.flags, ["/DMSVC_ONLY"]);
    assert_eq!(release.profile, m.profile_settings(Profile::Release));
}

/// A source in the package's language gets the package's edition; one in
/// the other language gets that language's default and none of the C++-only
/// flags (`project-layout.md`, "Target files").
///
/// Mutation: give every source the package's edition, or its language;
/// this test fails.
#[test]
fn each_language_gets_its_own_standard() {
    let m = manifest(LAYERED);
    let cxx = settings(&m, CompilerKind::Msvc, Profile::Dev, Lang::Cxx);
    assert_eq!((cxx.lang, cxx.edition), (Lang::Cxx, Edition::Cxx17));
    let c = settings(&m, CompilerKind::Msvc, Profile::Dev, Lang::C);
    assert_eq!((c.lang, c.edition), (Lang::C, Edition::C17));
    let flags: Vec<_> = compile_flags(&c.input())
        .into_iter()
        .map(|f| f.into_string().unwrap())
        .collect();
    assert!(flags.contains(&"/std:c17".to_owned()), "{flags:?}");
    assert!(!flags.contains(&"/EHsc".to_owned()), "{flags:?}");
}

/// Outside a project there is nothing to layer and no `src/`: the defaults,
/// C++20. `test` and the mode reach the command line.
///
/// Mutation: add an include directory, or another standard, outside a
/// project, or drop `test` or the mode in `input()`; this test fails.
#[test]
fn outside_a_project_the_defaults_apply() {
    let s = compile_settings(
        None,
        CompilerKind::Clang,
        Profile::Dev,
        Lang::Cxx,
        HostOs::Windows,
        true,
        Mode::Check,
    );
    assert_eq!((s.lang, s.edition), (Lang::Cxx, Edition::Cxx20));
    assert!(s.include_dirs.is_empty());
    assert!(s.build.defines.is_empty());
    assert_eq!((s.test, s.mode), (true, Mode::Check));
    // ... and reach the command line through `input()`.
    let args: Vec<_> = cuno_toolchain::flags::compile_args(&s.input(), Path::new("a.cpp"), None)
        .into_iter()
        .map(|a| a.into_string().unwrap())
        .collect();
    assert!(args.contains(&"-DCUNO_TEST".to_owned()), "{args:?}");
    assert!(args.contains(&"-fsyntax-only".to_owned()), "{args:?}");
}

/// Each host builds for its own OS, which picks its `[platform]` layer.
///
/// Mutation: map either host to another OS; this test fails.
#[test]
fn a_host_builds_for_its_own_os() {
    assert_eq!(target_os(HostOs::Windows), Os::Windows);
    let m = manifest(LAYERED);
    let other = compile_settings(
        Some((Path::new("C:/p"), &m)),
        CompilerKind::Clang,
        Profile::Dev,
        Lang::Cxx,
        HostOs::Other,
        false,
        Mode::Compile,
    );
    assert_eq!(other.build.defines, ["SHARED", "ON_LINUX"]);
}
