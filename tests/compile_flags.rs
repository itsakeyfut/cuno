//! The compile arguments cuno builds, run through the real cl, clang-cl,
//! clang and MinGW GCC: each must compile what `docs/en/specs/build.md`,
//! "Settings and flags", says, and the program must see the settings (the
//! standard, the C runtime, RTTI, `NDEBUG`). A flag a driver ignores shows
//! up here.
#![cfg(windows)]

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;

use cuno_core::Env;
use cuno_toolchain::flags::{
    BuildSettings, CompileInput, Edition, Lang, LinkInput, Mode, OptLevel, ProfileSettings,
    Runtime, Subsystem, Warnings, check_edition, compile_args, link_args,
};
use cuno_toolchain::msvc::{self, SystemRegistry};
use cuno_toolchain::select::CompilerKind;
use cuno_toolchain::{SystemRunner, Version};
use support::compilers::{Compiler, Language, gcc_dir, require, require_driver};

/// Each available driver, its program, and the environment it builds in.
fn drivers() -> Vec<(CompilerKind, PathBuf, Env, Version)> {
    let process = Env::from_process();
    let Some(reference) = require(Compiler::Msvc, Language::C, "x.c", "x.exe") else {
        return vec![];
    };
    let cl = PathBuf::from(reference.get_program());
    let installs = msvc::find_installations(&process, &SystemRunner::new()).unwrap();
    let toolset = installs
        .iter()
        .flat_map(|i| &i.toolsets)
        .find(|t| std::fs::canonicalize(t.cl()).ok() == std::fs::canonicalize(&cl).ok())
        .unwrap();
    let sdk = msvc::find_windows_sdk(&process, &SystemRegistry).unwrap();
    let env = msvc::resolve(toolset, &sdk).apply(&process);
    let any = Version {
        major: 99,
        minor: 0,
        patch: 0,
    };
    let mut out = vec![(CompilerKind::Msvc, cl, env.clone(), toolset.cl_version)];
    for (kind, compiler) in [
        (CompilerKind::ClangCl, Compiler::ClangCl),
        (CompilerKind::Clang, Compiler::Clang),
    ] {
        if let Some(reference) = require(compiler, Language::C, "x.c", "x.exe") {
            out.push((
                kind,
                PathBuf::from(reference.get_program()),
                env.clone(),
                any,
            ));
        }
    }
    // GCC uses no MSVC environment, and runs with its own directory on
    // `PATH`, as cuno runs it.
    if require_driver(Compiler::Gcc, Language::C).is_some() {
        let gcc = gcc_dir().unwrap().join("gcc.exe");
        let env = cuno_toolchain::gcc::env_for(&gcc, &process);
        out.push((CompilerKind::Gcc, gcc, env, any));
    }
    out
}

fn settings() -> (BuildSettings, ProfileSettings) {
    (
        BuildSettings {
            warnings: Warnings::All,
            warnings_as_errors: true,
            defines: vec![],
            include_dirs: vec![],
            links: vec![],
            lib_dirs: vec![],
            link_flags: vec![],
            flags: vec![],
            runtime: Runtime::Dynamic,
            exceptions: true,
            rtti: true,
        },
        ProfileSettings {
            opt_level: OptLevel::O0,
            debug_info: false,
            debug_assertions: true,
            lto: false,
        },
    )
}

const CXX: &str = r#"#include <cstdio>
#include <functional>
#include <vector>
int main() {
    std::vector<int> v{1};
    std::function<int(int)> f = [](int x) { return x + 1; };
    std::printf("std=%ld at=%d f=%d", (long)__cplusplus, v.at(0), f(1));
#ifdef _DLL
    std::printf(" dll");
#endif
#if defined(_CPPRTTI) || defined(__GXX_RTTI)
    std::printf(" rtti");
#endif
#ifdef NDEBUG
    std::printf(" ndebug");
#endif
    std::printf("\n");
}
"#;

const C: &str = "#include <stdio.h>\nint main(void) { printf(\"std=%ld\\n\", (long)__STDC_VERSION__); return 0; }\n";

/// Compiles `text` with `input`'s arguments and links it with the link
/// arguments for `subsystem`, in a new directory, which is returned with
/// the executable.
fn build(
    kind: CompilerKind,
    program: &Path,
    env: &Env,
    input: &CompileInput<'_>,
    subsystem: Subsystem,
    text: &str,
) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join(if input.lang == Lang::C {
        "main.c"
    } else {
        "main.cpp"
    });
    std::fs::write(&source, text).unwrap();
    let object = dir.path().join("main.obj");
    let exe = dir.path().join("main.exe");
    let run = |program: &Path, args: Vec<std::ffi::OsString>| {
        let output = Command::new(program)
            .args(&args)
            .current_dir(dir.path())
            .env_clear()
            .envs(env.vars())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{kind:?} {args:?}
{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    };
    run(program, compile_args(input, &source, Some(&object)));
    let cxx = input.lang == Lang::Cxx;
    let link = LinkInput {
        kind,
        // libstdc++exp is not what these check, and a GCC before 14 links none.
        version: cuno_toolchain::Version {
            major: 13,
            minor: 0,
            patch: 0,
        },
        build: input.build,
        profile: input.profile,
        subsystem,
        cxx,
    };
    run(
        &kind.link_driver(program, cxx),
        link_args(&link, &[object], &exe),
    );
    (dir, exe)
}

/// [`build`] as a console program, run, and what it printed.
fn build_and_run(
    kind: CompilerKind,
    program: &Path,
    env: &Env,
    input: &CompileInput<'_>,
    text: &str,
) -> String {
    let (_dir, exe) = build(kind, program, env, input, Subsystem::Console, text);
    let output = Command::new(&exe).output().unwrap();
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// The PE subsystem an executable was linked for: 2 for Windows GUI, 3 for
/// console.
fn pe_subsystem(exe: &Path) -> u16 {
    let bytes = std::fs::read(exe).unwrap();
    let pe = u32::from_le_bytes(bytes[0x3c..0x40].try_into().unwrap()) as usize;
    // The signature (4 bytes), the file header (20), then the optional
    // header, whose Subsystem field is at 68 in PE32+.
    let at = pe + 4 + 20 + 68;
    u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap())
}

/// Every edition compiles with each driver and the program reports that
/// standard, with warnings as errors on.
///
/// Mutation: emit a standard switch a driver ignores (`/std:c++23` on
/// clang-cl), use `/std:c++latest` (C++26 on current cl), or drop
/// `/Zc:__cplusplus`; this test fails.
#[test]
fn every_edition_reaches_the_program() {
    let (build, profile) = settings();
    for (kind, program, env, version) in drivers() {
        for (lang, edition, text, check) in [
            (
                Lang::Cxx,
                Edition::Cxx17,
                CXX,
                (|n: i64| n == 201703) as fn(i64) -> bool,
            ),
            (Lang::Cxx, Edition::Cxx20, CXX, |n| n == 202002),
            (Lang::Cxx, Edition::Cxx23, CXX, |n| n == 202302),
            (Lang::C, Edition::C11, C, |n| n == 201112),
            (Lang::C, Edition::C17, C, |n| n == 201710),
            (Lang::C, Edition::C23, C, |n| n > 201710),
        ] {
            // A cl too old for the edition is refused before compiling
            // (CI's may be); what it would do is not this test's.
            if check_edition(kind, version, edition).is_err() {
                assert_eq!(kind, CompilerKind::Msvc, "{edition:?}");
                continue;
            }
            let input = CompileInput {
                kind,
                lang,
                edition,
                build: &build,
                profile: &profile,
                include_dirs: &[],
                test: false,
                mode: Mode::Compile,
            };
            let printed = build_and_run(kind, &program, &env, &input, text);
            let std: i64 = printed
                .split_whitespace()
                .find_map(|w| w.strip_prefix("std="))
                .and_then(|n| n.parse().ok())
                .unwrap_or_else(|| panic!("{kind:?} {edition:?}: {printed}"));
            assert!(check(std), "{kind:?} {edition:?}: {printed}");
        }
    }
}

/// The C runtime, RTTI, `NDEBUG`, exceptions and optimisation reach the
/// program with each driver (GCC's C runtime is UCRT's DLL either way,
/// which its headers say with `_DLL`).
///
/// Mutation: drop `-fms-runtime-lib` for clang, `/MT`, `/GR-`, `-fno-rtti`,
/// or `NDEBUG`; this test fails.
#[test]
fn runtime_rtti_and_assertions_reach_the_program() {
    let (mut build, mut profile) = settings();
    for (kind, program, env, _) in drivers() {
        for (runtime, exceptions, rtti, assertions, opt, expected) in [
            (
                Runtime::Dynamic,
                true,
                true,
                true,
                OptLevel::O0,
                "at=1 f=2 dll rtti",
            ),
            (
                Runtime::Static,
                false,
                false,
                false,
                OptLevel::O2,
                "at=1 f=2 ndebug",
            ),
        ] {
            build.runtime = runtime;
            build.exceptions = exceptions;
            build.rtti = rtti;
            profile.debug_assertions = assertions;
            profile.opt_level = opt;
            profile.debug_info = !assertions;
            let input = CompileInput {
                kind,
                lang: Lang::Cxx,
                edition: Edition::Cxx20,
                build: &build,
                profile: &profile,
                include_dirs: &[],
                test: false,
                mode: Mode::Compile,
            };
            let printed = build_and_run(kind, &program, &env, &input, CXX);
            let rest = printed.split_once(' ').map(|(_, r)| r).unwrap_or_default();
            let expected = match kind {
                // MinGW links UCRT's DLL under either runtime.
                CompilerKind::Gcc if !expected.contains(" dll") => {
                    expected.replacen("f=2", "f=2 dll", 1)
                }
                CompilerKind::Gcc => expected.to_owned(),
                CompilerKind::Msvc | CompilerKind::Clang | CompilerKind::ClangCl => {
                    expected.to_owned()
                }
            };
            assert_eq!(rest, expected, "{kind:?} {runtime:?}");
        }
    }
}

/// With exceptions or RTTI off, every driver refuses `try` and `dynamic_cast`
/// rather than building a program that misbehaves.
///
/// Mutation: drop `/we4530`, `/we4541` or `-Werror=rtti`; this test fails.
#[test]
fn disabled_exceptions_and_rtti_are_refused_by_every_driver() {
    let (mut build, profile) = settings();
    build.warnings = Warnings::Default;
    build.warnings_as_errors = false;
    for (kind, program, env, _) in drivers() {
        for (exceptions, rtti, text) in [
            (
                false,
                true,
                "int main() { try { throw 1; } catch (int) {} }
",
            ),
            (
                true,
                false,
                "struct B { virtual ~B() {} }; struct D : B {}; int main() { D d; B* b = &d; return dynamic_cast<D*>(b) ? 0 : 1; }
",
            ),
        ] {
            build.exceptions = exceptions;
            build.rtti = rtti;
            let input = CompileInput {
                kind,
                lang: Lang::Cxx,
                edition: Edition::Cxx20,
                build: &build,
                profile: &profile,
                include_dirs: &[],
                test: false,
                mode: Mode::Compile,
            };
            let dir = tempfile::tempdir().unwrap();
            let source = dir.path().join("main.cpp");
            std::fs::write(&source, text).unwrap();
            let output = Command::new(&program)
                .args(compile_args(
                    &input,
                    &source,
                    Some(&dir.path().join("main.obj")),
                ))
                .current_dir(dir.path())
                .env_clear()
                .envs(env.vars())
                .output()
                .unwrap();
            assert!(
                !output.status.success(),
                "{kind:?} exceptions={exceptions} rtti={rtti} compiled"
            );
        }
    }
}

/// A program calling a system library links with each driver from the link
/// arguments, as a console and as a `windows` program, with debug
/// information (a `.pdb`, but for GCC, which keeps DWARF in the
/// executable) and with LTO, and runs.
///
/// Mutation: drop the library, `/ENTRY:mainCRTStartup`, the subsystem,
/// `-mwindows`, `/DEBUG` or `-g` at link time, or `-fuse-ld=lld` for
/// clang-cl with LTO; this test fails.
#[test]
fn programs_link_and_run_with_every_setting() {
    let text = "#include <windows.h>
int main() { return GetSystemMetrics(SM_CXSCREEN) >= 0 ? 0 : 1; }
";
    let (mut build_settings, mut profile) = settings();
    build_settings.links = vec!["user32".into()];
    for (kind, program, env, _) in drivers() {
        for (subsystem, expected, debug, lto) in [
            (Subsystem::Console, 3, true, false),
            (Subsystem::Windows, 2, false, true),
        ] {
            profile.debug_info = debug;
            profile.lto = lto;
            let input = CompileInput {
                kind,
                lang: Lang::Cxx,
                edition: Edition::Cxx20,
                build: &build_settings,
                profile: &profile,
                include_dirs: &[],
                test: false,
                mode: Mode::Compile,
            };
            let (dir, exe) = build(kind, &program, &env, &input, subsystem, text);
            assert_eq!(pe_subsystem(&exe), expected, "{kind:?} {subsystem:?}");
            let status = Command::new(&exe).status().unwrap();
            assert!(status.success(), "{kind:?} {subsystem:?} {status}");
            assert_eq!(
                dir.path().join("main.pdb").is_file(),
                debug && kind != CompilerKind::Gcc,
                "{kind:?} {subsystem:?}"
            );
        }
    }
}

/// Links a program against a library built here, found through `lib-dirs`,
/// with a `link-flags` entry that holds a comma, on each driver (GCC's an
/// `ar` archive and a GNU ld flag).
///
/// Mutation: drop `lib-dirs`, or pass `link-flags` to clang or GCC as
/// `-Wl,` (which splits `/OPT:REF,ICF`); this test fails.
#[test]
fn lib_dirs_and_link_flags_reach_the_linker() {
    let (settings_build, profile) = settings();
    for (kind, program, env, _) in drivers() {
        let libs = tempfile::tempdir().unwrap();
        let source = libs.path().join("answer.c");
        std::fs::write(&source, "int answer(void) { return 42; }\n").unwrap();
        let object = libs.path().join("answer.obj");
        let input = CompileInput {
            kind,
            lang: Lang::C,
            edition: Edition::C17,
            build: &settings_build,
            profile: &profile,
            include_dirs: &[],
            test: false,
            mode: Mode::Compile,
        };
        let status = Command::new(&program)
            .args(compile_args(&input, &source, Some(&object)))
            .current_dir(libs.path())
            .env_clear()
            .envs(env.vars())
            .status()
            .unwrap();
        assert!(status.success(), "{kind:?}");
        let (archiver, archive, flag) = match kind {
            CompilerKind::Gcc => (
                program.with_file_name("ar.exe"),
                vec![
                    "rcs".into(),
                    libs.path().join("libanswer.a").into_os_string(),
                    object.into(),
                ],
                "--exclude-libs=libx,liby",
            ),
            CompilerKind::Msvc | CompilerKind::Clang | CompilerKind::ClangCl => {
                let lib_exe = std::env::split_paths(env.get("PATH").unwrap())
                    .map(|d| d.join("lib.exe"))
                    .find(|p| p.is_file())
                    .unwrap();
                let mut out = std::ffi::OsString::from("/OUT:");
                out.push(libs.path().join("answer.lib"));
                (
                    lib_exe,
                    vec![std::ffi::OsString::from("/NOLOGO"), out, object.into()],
                    "/OPT:REF,ICF",
                )
            }
        };
        let status = Command::new(archiver)
            .args(archive)
            .env_clear()
            .envs(env.vars())
            .status()
            .unwrap();
        assert!(status.success());

        let linked = BuildSettings {
            links: vec!["answer".into()],
            lib_dirs: vec![libs.path().to_owned()],
            link_flags: vec![flag.into()],
            ..settings_build.clone()
        };
        let input = CompileInput {
            lang: Lang::Cxx,
            edition: Edition::Cxx20,
            build: &linked,
            ..input
        };
        let text = "extern \"C\" int answer(void);\nint main() { return answer(); }\n";
        let (_dir, exe) = build(kind, &program, &env, &input, Subsystem::Console, text);
        let code = Command::new(&exe).status().unwrap().code();
        assert_eq!(code, Some(42), "{kind:?}");
    }
}

/// A program with `WinMain` and no `main` under the default console
/// subsystem fails to link on every driver for the MSVC ABI, instead of
/// becoming a GUI program with no console (MinGW's start-up code calls
/// `WinMain` from a console program, which keeps its console); the static
/// C runtime leaves no VCRUNTIME140.dll import; and GCC's executables
/// import none of GCC's own DLLs under either runtime, so they start
/// outside cuno.
///
/// Mutation: leave the console subsystem unstated, link the dynamic C
/// runtime for `runtime = "static"`, or leave GCC's runtime or
/// `winpthread` dynamic; this test fails.
#[test]
fn console_is_stated_and_the_runtime_is_the_ones_asked_for() {
    let (settings_build, profile) = settings();
    let win_main = "#include <windows.h>\nint WINAPI WinMain(HINSTANCE a, HINSTANCE b, LPSTR c, int d) { return 0; }\n";
    for (kind, program, env, _) in drivers() {
        let input = CompileInput {
            kind,
            lang: Lang::Cxx,
            edition: Edition::Cxx20,
            build: &settings_build,
            profile: &profile,
            include_dirs: &[],
            test: false,
            mode: Mode::Compile,
        };
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("main.cpp");
        std::fs::write(&source, win_main).unwrap();
        let object = dir.path().join("main.obj");
        let run = |program: &Path, args: Vec<std::ffi::OsString>| {
            Command::new(program)
                .args(args)
                .current_dir(dir.path())
                .env_clear()
                .envs(env.vars())
                .output()
                .unwrap()
                .status
                .success()
        };
        let mut lenient = settings_build.clone();
        lenient.warnings_as_errors = false;
        let compile = CompileInput {
            build: &lenient,
            ..input
        };
        assert!(
            run(&program, compile_args(&compile, &source, Some(&object))),
            "{kind:?}"
        );
        let link = LinkInput {
            kind,
            // libstdc++exp is not what these check, and a GCC before 14 links none.
            version: cuno_toolchain::Version {
                major: 13,
                minor: 0,
                patch: 0,
            },
            build: &lenient,
            profile: &profile,
            subsystem: Subsystem::Console,
            cxx: true,
        };
        let exe = dir.path().join("main.exe");
        let linked = run(
            &kind.link_driver(&program, true),
            link_args(&link, &[object], &exe),
        );
        match kind {
            CompilerKind::Gcc => assert_eq!(pe_subsystem(&exe), 3, "a console program"),
            CompilerKind::Msvc | CompilerKind::Clang | CompilerKind::ClangCl => {
                assert!(!linked, "{kind:?} linked WinMain as a console program");
            }
        }

        for (runtime, imports) in [(Runtime::Dynamic, true), (Runtime::Static, false)] {
            let with_runtime = BuildSettings {
                runtime,
                ..settings_build.clone()
            };
            let input = CompileInput {
                build: &with_runtime,
                ..input
            };
            let (_dir, exe) = build(kind, &program, &env, &input, Subsystem::Console, CXX);
            let bytes = std::fs::read(&exe).unwrap().to_ascii_lowercase();
            let has = |name: &[u8]| bytes.windows(name.len()).any(|w| w == name);
            match kind {
                CompilerKind::Gcc => {
                    for dll in [&b"libstdc++-6.dll"[..], b"libwinpthread-1.dll", b"libgcc_s"] {
                        assert!(
                            !has(dll),
                            "{runtime:?} imports {}",
                            String::from_utf8_lossy(dll)
                        );
                    }
                }
                CompilerKind::Msvc | CompilerKind::Clang | CompilerKind::ClangCl => {
                    assert_eq!(has(b"vcruntime140.dll"), imports, "{kind:?} {runtime:?}");
                }
            }
        }
    }
}
