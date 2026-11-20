//! The environment cuno builds for MSVC, clang and clang-cl on Windows,
//! checked against `vcvars64.bat` (through the test support's independent
//! capture) and by compiling and running a program with it, from a directory
//! whose name is not ASCII.
#![cfg(windows)]

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;

use cuno_core::Env;
use cuno_toolchain::msvc::{self, MsvcEnvironment, SystemRegistry};
use support::compilers::{Compiler, Language, require, vcvars_environment};

/// The installation and toolset whose cl.exe the reference uses, and the
/// environment cuno builds for it, or `None` when the test is skipped.
fn resolved() -> Option<(PathBuf, msvc::MsvcToolset, MsvcEnvironment)> {
    let reference = require(Compiler::Msvc, Language::C, "x.c", "x.exe")?;
    let reference_cl = PathBuf::from(reference.get_program());
    let env = Env::from_process();
    let installs = msvc::find_installations(&env, &cuno_toolchain::SystemRunner).unwrap();
    let (install, toolset) = installs
        .iter()
        .flat_map(|i| i.toolsets.iter().map(move |t| (i, t)))
        .find(|(_, t)| same_file(&t.cl(), &reference_cl))
        .unwrap_or_else(|| panic!("{} is not among {installs:#?}", reference_cl.display()));
    let sdk = msvc::find_windows_sdk(&env, &SystemRegistry).unwrap();
    Some((
        install.path.clone(),
        toolset.clone(),
        msvc::resolve(toolset, &sdk),
    ))
}

/// Every `INCLUDE` and `LIB` directory cuno builds is one `vcvars64.bat`
/// sets: a change of layout in a new Visual Studio or SDK shows up here.
///
/// Mutation: name an `INCLUDE` or `LIB` directory wrongly (`um` as `UM2`);
/// this test fails.
#[test]
fn every_directory_is_one_vcvars64_sets() {
    let Some((install, _, resolved)) = resolved() else {
        return;
    };
    let vcvars = vcvars_environment(&install).unwrap_or_else(|m| panic!("{}", m.detail));
    for (name, dirs) in [("INCLUDE", &resolved.include), ("LIB", &resolved.lib)] {
        let expected: Vec<String> = vcvars
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| std::env::split_paths(v).map(|d| normalized(&d)).collect())
            .unwrap_or_default();
        for dir in dirs {
            assert!(
                expected.contains(&normalized(dir)),
                "{name}: {} is not among {expected:#?}",
                dir.display()
            );
        }
    }
}

/// A program using `<windows.h>` compiles, links and runs with MSVC, clang
/// and clang-cl given only cuno's environment on top of a shell without any
/// Developer Command Prompt variable, from a directory named in Japanese.
///
/// Mutation: drop `um` from `INCLUDE` or `ucrt` from `LIB`; this test fails.
#[test]
fn hello_world_builds_with_every_compiler_from_a_plain_shell() {
    let Some((_, toolset, resolved)) = resolved() else {
        return;
    };
    let plain = Env::from_process().without(&[
        "INCLUDE",
        "LIB",
        "LIBPATH",
        "CL",
        "_CL_",
        "VSINSTALLDIR",
        "VCINSTALLDIR",
        "VCToolsInstallDir",
        "WindowsSdkDir",
    ]);
    let env = resolved.apply(&plain);
    let dir = tempfile::Builder::new()
        .prefix("ソース ")
        .tempdir()
        .unwrap();
    std::fs::write(
        dir.path().join("main.c"),
        "#include <windows.h>\n#include <stdio.h>\nint main(void) { printf(\"hello %lu\\n\", (unsigned long)GetCurrentProcessId() * 0); return 0; }\n",
    )
    .unwrap();

    let mut compilers = vec![(
        "msvc",
        toolset.cl(),
        vec!["/nologo", "main.c", "/Fe:msvc.exe"],
    )];
    for (name, compiler, args) in [
        ("clang", Compiler::Clang, vec!["main.c", "-o", "clang.exe"]),
        (
            "clang-cl",
            Compiler::ClangCl,
            vec!["/nologo", "main.c", "/Fe:clang-cl.exe"],
        ),
    ] {
        if let Some(reference) = require(compiler, Language::C, "x.c", "x.exe") {
            compilers.push((name, PathBuf::from(reference.get_program()), args));
        }
    }
    for (name, program, args) in compilers {
        let output = Command::new(&program)
            .args(&args)
            .current_dir(dir.path())
            .env_clear()
            .envs(env.vars())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{name} failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let run = Command::new(dir.path().join(format!("{name}.exe")))
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&run.stdout).trim(),
            "hello 0",
            "{name}"
        );
    }
}

/// A directory compared as Windows compares it: without case, and without
/// the doubled or trailing separators `vcvars64.bat` writes
/// (`Windows Kits\10\\Include`).
fn normalized(dir: &Path) -> String {
    dir.components()
        .collect::<PathBuf>()
        .to_string_lossy()
        .to_lowercase()
}

fn same_file(a: &Path, b: &Path) -> bool {
    std::fs::canonicalize(a).ok() == std::fs::canonicalize(b).ok()
}
