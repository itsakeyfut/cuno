//! The compile arguments cuno builds, run through the real cl, clang-cl and
//! clang: each must compile what `docs/en/specs/build.md`, "Settings and
//! flags", says, and the program must see the settings (the standard, the C
//! runtime, RTTI, `NDEBUG`). A flag a driver ignores shows up here.
#![cfg(windows)]

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;

use cuno_core::Env;
use cuno_toolchain::flags::{
    BuildSettings, CompileInput, Edition, Lang, Mode, OptLevel, ProfileSettings, Runtime, Warnings,
    check_edition, compile_args,
};
use cuno_toolchain::msvc::{self, SystemRegistry};
use cuno_toolchain::select::CompilerKind;
use cuno_toolchain::{SystemRunner, Version};
use support::compilers::{Compiler, Language, require};

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
#ifdef _CPPRTTI
    std::printf(" rtti");
#endif
#ifdef NDEBUG
    std::printf(" ndebug");
#endif
    std::printf("\n");
}
"#;

const C: &str = "#include <stdio.h>\nint main(void) { printf(\"std=%ld\\n\", (long)__STDC_VERSION__); return 0; }\n";

/// Compiles `text` with `input`'s arguments, links it plainly, runs it and
/// returns what it printed.
fn build_and_run(
    kind: CompilerKind,
    program: &Path,
    env: &Env,
    input: &CompileInput<'_>,
    text: &str,
) -> String {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join(if input.lang == Lang::C {
        "main.c"
    } else {
        "main.cpp"
    });
    std::fs::write(&source, text).unwrap();
    let object = dir.path().join("main.obj");
    let exe = dir.path().join("main.exe");
    let run = |args: Vec<std::ffi::OsString>| {
        let output = Command::new(program)
            .args(&args)
            .current_dir(dir.path())
            .env_clear()
            .envs(env.vars())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{kind:?} {args:?}\n{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    };
    run(compile_args(input, &source, Some(&object)));
    // A plain link, until the link arguments exist (#62).
    let mut link: Vec<std::ffi::OsString> = vec![object.into()];
    match kind {
        CompilerKind::Clang => {
            link.push("--target=x86_64-pc-windows-msvc".into());
            link.push(
                match input.build.runtime {
                    Runtime::Dynamic => "-fms-runtime-lib=dll",
                    Runtime::Static => "-fms-runtime-lib=static",
                }
                .into(),
            );
            link.extend(["-o".into(), exe.clone().into()]);
        }
        _ => {
            link.push("/nologo".into());
            let mut out = std::ffi::OsString::from("/Fe:");
            out.push(&exe);
            link.push(out);
        }
    }
    run(link);
    let output = Command::new(&exe).output().unwrap();
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
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
/// program with each driver.
///
/// Mutation: drop `-fms-runtime-lib` for clang, `/MT`, `/GR-`, or
/// `NDEBUG`; this test fails.
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
