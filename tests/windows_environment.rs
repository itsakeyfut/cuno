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

/// Every directory cuno puts in `INCLUDE`, `LIB`, `LIBPATH` and in front of
/// `PATH` exists and is one `vcvars64.bat` sets, and the SDK tools are found
/// on that `PATH`: a change of layout in a new Visual Studio or SDK shows up
/// here.
///
/// Mutation: name any directory wrongly (`um` as `UM2`), or leave the SDK's
/// `bin` out of `PATH` (`rc.exe` is then not found); this test fails.
#[test]
fn every_directory_exists_and_is_one_vcvars64_sets() {
    let Some((install, _, resolved)) = resolved() else {
        return;
    };
    let vcvars = vcvars_environment(&install).unwrap_or_else(|m| panic!("{}", m.detail));
    for (name, dirs) in [
        ("INCLUDE", &resolved.include),
        ("LIB", &resolved.lib),
        ("LIBPATH", &resolved.libpath),
        ("PATH", &resolved.path),
    ] {
        let expected: Vec<String> = vcvars
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| std::env::split_paths(v).map(|d| normalized(&d)).collect())
            .unwrap_or_default();
        for dir in dirs {
            assert!(dir.is_dir(), "{name}: {} does not exist", dir.display());
            assert!(
                expected.contains(&normalized(dir)),
                "{name}: {} is not among {expected:#?}",
                dir.display()
            );
        }
    }
    for tool in ["cl.exe", "link.exe", "rc.exe", "mt.exe"] {
        assert!(
            resolved.path.iter().any(|dir| dir.join(tool).is_file()),
            "{tool} is not in {:#?}",
            resolved.path
        );
    }
}

/// A program using `<windows.h>` compiles, links and runs with MSVC, clang
/// and clang-cl given only cuno's environment, from a directory named in
/// Japanese, and every compiler sees the toolset cuno chose even when cuno
/// runs from the Developer Command Prompt of another toolset.
///
/// Mutation: drop `um` from `INCLUDE` or `ucrt` from `LIB`, or keep the
/// Developer Command Prompt's `VCToolsInstallDir` (clang then takes the stale
/// toolset and reports another `_MSC_VER`); this test fails.
#[test]
fn hello_world_builds_with_every_compiler_and_the_chosen_toolset() {
    let Some((_, toolset, resolved)) = resolved() else {
        return;
    };
    let dir = tempfile::Builder::new()
        .prefix("ソース ")
        .tempdir()
        .unwrap();
    // A Developer Command Prompt for a toolset that is not the one chosen.
    let stale = dir.path().join("stale").join("VC");
    let stale_toolset = stale.join("Tools").join("MSVC").join("14.29.30133");
    std::fs::create_dir_all(stale_toolset.join("include")).unwrap();
    let shell = Env::from_process()
        .without(&["INCLUDE", "LIB", "LIBPATH", "CL", "_CL_"])
        .with("VCINSTALLDIR", stale.clone().into())
        .with("VCToolsInstallDir", stale_toolset.clone().into())
        .with("EXTERNAL_INCLUDE", stale_toolset.join("include").into());
    let env = resolved.apply(&shell);
    std::fs::write(
        dir.path().join("main.c"),
        "#include <windows.h>\n#include <stdio.h>\nint main(void) { printf(\"hello %lu %d\\n\", (unsigned long)GetCurrentProcessId() * 0, _MSC_VER); return 0; }\n",
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
    let expected = format!(
        "hello 0 {}{:02}",
        toolset.cl_version.major, toolset.cl_version.minor
    );
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
            expected,
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
