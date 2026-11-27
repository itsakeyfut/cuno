//! The link step's arguments, row by row from `docs/en/specs/build.md`,
//! "Settings and flags". Linking goes through the compiler driver, so each
//! driver adds its own C runtime libraries.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::{BuildSettings, OptLevel, ProfileSettings, Subsystem, TARGET_SWITCH};
use crate::Version;
use crate::compiler::{CompilerKind, Family};

/// What one link of the build is made from.
#[derive(Clone, Copy, Debug)]
pub struct LinkInput<'a> {
    /// The driver.
    pub kind: CompilerKind,
    /// Its version: GCC links `libstdc++exp` from 14 (`build.md`, "Settings
    /// and flags").
    pub version: Version,
    /// The resolved build settings (`links`, `lib-dirs`, `link-flags`).
    pub build: &'a BuildSettings,
    /// The resolved profile (`debug-info`, `lto`, and `opt-level` for LTO).
    pub profile: &'a ProfileSettings,
    /// The executable's `[[bin]] subsystem`.
    pub subsystem: Subsystem,
    /// Whether any of its objects is C++, which GCC links with `g++` and
    /// its C++ runtime.
    pub cxx: bool,
}

/// The files the linker may take `lib` from, for a `links` entry built
/// with `kind`, in the order it tries them in each directory: `<lib>.lib`
/// for the compilers that build for the MSVC ABI (the MSVC and clang-cl
/// link commands name it, clang's `-l<lib>` is found as it by lld), and
/// the seven names MinGW's `ld` tries for `-l<lib>` (as `-Wl,--verbose`
/// lists its attempts). The
/// build looks for them in `lib-dirs` to relink when one changes
/// (`docs/en/specs/build.md`, "Deciding what changed"), so a compiler
/// naming libraries otherwise answers here.
pub fn library_files(kind: CompilerKind, lib: &str) -> Vec<String> {
    match kind {
        CompilerKind::Msvc | CompilerKind::ClangCl | CompilerKind::Clang => {
            vec![format!("{lib}.lib")]
        }
        CompilerKind::Gcc => vec![
            format!("lib{lib}.dll.a"),
            format!("{lib}.dll.a"),
            format!("lib{lib}.a"),
            format!("{lib}.lib"),
            format!("lib{lib}.lib"),
            format!("lib{lib}.dll"),
            format!("{lib}.dll"),
        ],
    }
}

/// What makes GCC link its own runtime and `winpthread` statically, for
/// either `runtime` (`docs/en/specs/build.md`, "Settings and flags"): their
/// DLLs are not part of Windows. `-lstdc++` comes before `-lwinpthread`,
/// which resolves what libstdc++ uses; `-Bdynamic` after them leaves the
/// libraries the driver adds (UCRT's) as they are. From GCC 14, C++ also
/// links `libstdc++exp`, where MinGW's libstdc++ keeps what `std::print`
/// writes to a terminal with; before 14 there is no `<print>` to need it.
fn gcc_runtime_args(cxx: bool, version: Version) -> Vec<&'static str> {
    let mut args = vec!["-static-libgcc"];
    if cxx {
        args.push("-static-libstdc++");
    }
    args.push("-Wl,-Bstatic");
    if cxx {
        if version.major >= 14 {
            args.push("-lstdc++exp");
        }
        args.push("-lstdc++");
    }
    args.extend(["-lwinpthread", "-Wl,-Bdynamic"]);
    args
}

/// The arguments for the driver to link `objects` into `exe`: the driver's
/// own switches, the objects and the output, then the linker's arguments
/// (after `/link` for MSVC and clang-cl), with `link-flags` last.
pub fn link_args(input: &LinkInput<'_>, objects: &[PathBuf], exe: &Path) -> Vec<OsString> {
    let build = input.build;
    let profile = input.profile;
    // lld optimises at level 2 when it runs LTO unless told the level.
    let lto_level = match profile.opt_level {
        OptLevel::O0 => "0",
        OptLevel::O1 => "1",
        OptLevel::O2 | OptLevel::Size => "2",
        OptLevel::O3 => "3",
    };
    let mut args: Vec<OsString> = Vec::new();
    let push = |args: &mut Vec<OsString>, flags: &[&str]| {
        args.extend(flags.iter().map(OsString::from));
    };

    match input.kind {
        CompilerKind::Msvc => push(&mut args, &["/nologo"]),
        CompilerKind::ClangCl => {
            push(&mut args, &["/nologo", "-fuse-ld=lld"]);
            if profile.lto {
                push(&mut args, &["-flto"]);
            }
        }
        CompilerKind::Clang => {
            // `-fms-runtime-lib` is left out: the objects already say which C
            // runtime they use, and the linker follows them.
            push(&mut args, &[TARGET_SWITCH, "-fuse-ld=lld"]);
            if profile.debug_info {
                push(&mut args, &["-g"]);
            }
            if profile.lto {
                push(&mut args, &["-flto"]);
            }
        }
        CompilerKind::Gcc => {
            if profile.debug_info {
                push(&mut args, &["-g"]);
            }
            // At link time GCC optimises at the level the objects were
            // compiled at, so no level is given; `auto` runs it in
            // parallel, where plain `-flto` warns that it runs serially.
            if profile.lto {
                push(&mut args, &["-flto=auto"]);
            }
        }
    }

    args.extend(objects.iter().map(OsString::from));

    match input.kind {
        // The GNU-style drivers: clang linking with lld for the MSVC ABI,
        // and MinGW's GCC with its own ld.
        CompilerKind::Clang | CompilerKind::Gcc => {
            args.extend(["-o".into(), exe.into()]);
            for lib in &build.links {
                args.push(format!("-l{lib}").into());
            }
            for dir in &build.lib_dirs {
                let mut arg = OsString::from("-L");
                arg.push(dir);
                args.push(arg);
            }
            let gcc = input.kind == CompilerKind::Gcc;
            push(
                &mut args,
                match (input.subsystem, gcc) {
                    (Subsystem::Console, false) => &["-Wl,/subsystem:console"][..],
                    (Subsystem::Windows, false) => {
                        &["-Wl,/subsystem:windows", "-Wl,/entry:mainCRTStartup"]
                    }
                    // MinGW's start-up code calls `main` for either.
                    (Subsystem::Console, true) => &["-mconsole"],
                    (Subsystem::Windows, true) => &["-mwindows"],
                },
            );
            if gcc {
                push(&mut args, &gcc_runtime_args(input.cxx, input.version));
            } else if profile.lto {
                args.push(format!("-Wl,/opt:lldlto={lto_level}").into());
            }
            // `-Xlinker`, not `-Wl,`, which would split a flag at its commas.
            for flag in &build.link_flags {
                args.extend(["-Xlinker".into(), flag.into()]);
            }
            return args;
        }
        CompilerKind::Msvc | CompilerKind::ClangCl => {}
    }

    let mut out = OsString::from("/Fe:");
    out.push(exe);
    args.push(out);
    // Everything after `/link` goes to the linker itself.
    args.push("/link".into());
    for lib in &build.links {
        // The first name is the one cl and clang-cl are given.
        args.push(library_files(input.kind, lib).remove(0).into());
    }
    for dir in &build.lib_dirs {
        let mut arg = OsString::from("/LIBPATH:");
        arg.push(dir);
        args.push(arg);
    }
    // Stated for console too: otherwise a program with `WinMain` and no
    // `main` is silently linked as a GUI program.
    push(
        &mut args,
        match input.subsystem {
            Subsystem::Console => &["/SUBSYSTEM:CONSOLE"][..],
            Subsystem::Windows => &["/SUBSYSTEM:WINDOWS", "/ENTRY:mainCRTStartup"],
        },
    );
    if profile.debug_info {
        push(&mut args, &["/DEBUG"]);
        match input.kind.family() {
            // link.exe links incrementally under /DEBUG; lld does not.
            Family::Msvc => push(&mut args, &["/INCREMENTAL:NO"]),
            Family::Llvm | Family::Gnu => {}
        }
    }
    if profile.lto {
        match input.kind.family() {
            Family::Msvc => push(&mut args, &["/LTCG"]),
            Family::Llvm | Family::Gnu => args.push(format!("/opt:lldlto={lto_level}").into()),
        }
    }

    args.extend(build.link_flags.iter().map(OsString::from));
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The compilers for the MSVC ABI name a library `<lib>.lib`; GCC's
    /// `ld` tries seven names in each directory, in the order
    /// `-Wl,--verbose` lists its attempts.
    ///
    /// Mutation: name it otherwise for one compiler, or reorder GCC's
    /// names; this test fails.
    #[test]
    fn a_library_is_the_files_its_linker_tries() {
        for kind in CompilerKind::ALL {
            let want: Vec<&str> = match kind {
                CompilerKind::Msvc | CompilerKind::ClangCl | CompilerKind::Clang => {
                    vec!["foo.lib"]
                }
                CompilerKind::Gcc => vec![
                    "libfoo.dll.a",
                    "foo.dll.a",
                    "libfoo.a",
                    "foo.lib",
                    "libfoo.lib",
                    "libfoo.dll",
                    "foo.dll",
                ],
            };
            assert_eq!(library_files(kind, "foo"), want, "{kind:?}");
        }
    }
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
        args_for(kind, build, profile, subsystem, true)
    }

    fn args_for(
        kind: CompilerKind,
        build: &BuildSettings,
        profile: &ProfileSettings,
        subsystem: Subsystem,
        cxx: bool,
    ) -> Vec<String> {
        // A GCC from before libstdc++exp, so each case's expected arguments
        // need not say which GCC; the version's own test is below.
        let input = LinkInput {
            kind,
            version: Version {
                major: 13,
                minor: 2,
                patch: 0,
            },
            build,
            profile,
            subsystem,
            cxx,
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
        // GCC links its runtime and winpthread statically under either
        // runtime; only C++ takes libstdc++.
        for b in [&b, &static_runtime] {
            assert_eq!(
                args_for(CompilerKind::Gcc, b, &p, Subsystem::Console, true),
                [
                    "a.obj",
                    "b.obj",
                    "-o",
                    "app.exe",
                    "-mconsole",
                    "-static-libgcc",
                    "-static-libstdc++",
                    "-Wl,-Bstatic",
                    "-lstdc++",
                    "-lwinpthread",
                    "-Wl,-Bdynamic"
                ]
            );
            assert_eq!(
                args_for(CompilerKind::Gcc, b, &p, Subsystem::Console, false),
                [
                    "a.obj",
                    "b.obj",
                    "-o",
                    "app.exe",
                    "-mconsole",
                    "-static-libgcc",
                    "-Wl,-Bstatic",
                    "-lwinpthread",
                    "-Wl,-Bdynamic"
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
        assert_eq!(
            args(CompilerKind::Gcc, &full_build, &full_profile, w),
            [
                "-g",
                "-flto=auto",
                "a.obj",
                "b.obj",
                "-o",
                "app.exe",
                "-luser32",
                "-lgdi32",
                "-Llibs",
                "-Lmore",
                "-mwindows",
                "-static-libgcc",
                "-static-libstdc++",
                "-Wl,-Bstatic",
                "-lstdc++",
                "-lwinpthread",
                "-Wl,-Bdynamic",
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
            let gcc = args(CompilerKind::Gcc, &build(), &p, Subsystem::Console);
            assert!(!gcc.iter().any(|a| a.contains("lldlto")), "{gcc:?}");
        }
    }

    /// GCC 14 and later link `libstdc++exp` for C++, right before
    /// `libstdc++` and statically; GCC 13, which has none, and C do not.
    ///
    /// Mutation: give it whatever the version, after `-lstdc++`, or to C;
    /// this test fails.
    #[test]
    fn gcc_14_links_libstdcxx_exp_for_cxx() {
        let v = |major| Version {
            major,
            minor: 1,
            patch: 0,
        };
        assert_eq!(
            gcc_runtime_args(true, v(14)),
            [
                "-static-libgcc",
                "-static-libstdc++",
                "-Wl,-Bstatic",
                "-lstdc++exp",
                "-lstdc++",
                "-lwinpthread",
                "-Wl,-Bdynamic",
            ]
        );
        assert!(!gcc_runtime_args(true, v(13)).contains(&"-lstdc++exp"));
        assert!(!gcc_runtime_args(false, v(16)).contains(&"-lstdc++exp"));
    }
}
