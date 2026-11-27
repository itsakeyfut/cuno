//! Selection and the standard library check on a real machine: the Clang the
//! test support uses, with the real Visual Studio and its standard library.
#![cfg(windows)]

mod support;

use std::path::PathBuf;

use cuno_core::Env;
use cuno_toolchain::flags::{
    BuildSettings, CompileInput, Edition, Lang, Mode, OptLevel, ProfileSettings, Runtime, Warnings,
};
use cuno_toolchain::llvm::{ExplicitPaths, find_llvm_tools};
use cuno_toolchain::msvc::{self, SystemRegistry};
use cuno_toolchain::probe::{self, ProbeInputs};
use cuno_toolchain::select::{self, Choices, CompilerKind, Constraints, Found, Selected};
use cuno_toolchain::{HostOs, SystemRunner, ToolchainError};
use support::compilers::{Compiler, Language, require};

/// `kind` selected on this machine, with the reference's LLVM first on
/// `PATH`, and the environment it builds in; `None` when skipped.
fn selected(kind: CompilerKind) -> Option<(Selected, Env)> {
    let compiler = match kind {
        CompilerKind::ClangCl => Compiler::ClangCl,
        _ => Compiler::Clang,
    };
    let reference = PathBuf::from(require(compiler, Language::C, "x.c", "x.exe")?.get_program());
    let process = Env::from_process();
    let mut path = vec![reference.parent().unwrap().to_owned()];
    path.extend(std::env::split_paths(
        process.get("PATH").unwrap_or_default(),
    ));
    let env = process.with("PATH", std::env::join_paths(path).unwrap());

    let installs = msvc::find_installations(&env, &SystemRunner::new()).unwrap();
    let llvm = find_llvm_tools(
        &env,
        HostOs::Windows,
        &ExplicitPaths::default(),
        &installs,
        &SystemRunner::new(),
    )
    .unwrap();
    let choices = Choices {
        cli: Some(kind.name().to_owned()),
        ..Default::default()
    };
    let found = Found {
        installs: &installs,
        llvm: &llvm,
        gcc: &[],
        explicit_msvc: None,
    };
    let selected =
        select::select(&choices, found, &Constraints::default(), HostOs::Windows).unwrap();
    let sdk = msvc::find_windows_sdk(&env, &SystemRegistry).unwrap();
    let resolved = msvc::resolve(selected.msvc.as_ref().unwrap(), &sdk);
    Some((selected, resolved.apply(&env)))
}

/// Probes `selected` for a C++20 project with `defines` and, unless
/// `rtti`, RTTI off, as the build would compile it.
fn probe(
    selected: &Selected,
    env: &Env,
    defines: &[&str],
    rtti: bool,
) -> Result<(), ToolchainError> {
    let scratch = tempfile::tempdir().unwrap();
    let build = BuildSettings {
        warnings: Warnings::Default,
        warnings_as_errors: false,
        defines: defines.iter().map(|d| (*d).to_owned()).collect(),
        include_dirs: vec![],
        links: vec![],
        lib_dirs: vec![],
        link_flags: vec![],
        flags: vec![],
        runtime: Runtime::Dynamic,
        exceptions: true,
        rtti,
    };
    let profile = ProfileSettings {
        opt_level: OptLevel::O0,
        debug_info: false,
        debug_assertions: true,
        lto: false,
    };
    let input = CompileInput {
        kind: selected.kind,
        lang: Lang::Cxx,
        edition: Edition::Cxx20,
        build: &build,
        profile: &profile,
        include_dirs: &[],
        test: false,
        mode: Mode::Check,
    };
    probe::check(
        selected,
        env,
        ProbeInputs {
            scratch: scratch.path(),
            input: &input,
            cwd: scratch.path(),
        },
        &SystemRunner::new(),
    )
}

/// The Clang the test support uses (on CI the pinned LLVM) is accepted by
/// the installed MSVC standard library, through clang and clang-cl alike,
/// including for a project built without RTTI; one pretending to be Clang 1
/// is rejected with the version the library asks for, unless the project
/// allows the mismatch as the library permits.
///
/// Mutation: treat the probe's failure as passing, or drop the build's flags
/// from the probe; this test fails.
#[test]
fn the_standard_library_decides_which_clang_it_accepts() {
    for kind in [CompilerKind::Clang, CompilerKind::ClangCl] {
        let Some((selected, env)) = selected(kind) else {
            continue;
        };
        probe(&selected, &env, &[], true).unwrap_or_else(|e| panic!("{kind:?}: {e}"));
        probe(&selected, &env, &["_HAS_STATIC_RTTI=0"], false)
            .unwrap_or_else(|e| panic!("{kind:?} without RTTI: {e}"));

        let err = probe(&selected, &env, &["__clang_major__=1"], true).unwrap_err();
        assert!(
            matches!(
                &err,
                ToolchainError::RejectedByStl {
                    needed: Some(_),
                    ..
                }
            ),
            "{kind:?}: {err:?}"
        );
        probe(
            &selected,
            &env,
            &[
                "__clang_major__=1",
                "_ALLOW_COMPILER_AND_STL_VERSION_MISMATCH",
            ],
            true,
        )
        .unwrap_or_else(|e| panic!("{kind:?}: {e}"));
    }
}
