//! Selection and the standard library check on a real machine: the Clang the
//! test support uses, with the real Visual Studio and its standard library.
#![cfg(windows)]

mod support;

use std::path::PathBuf;

use cuno_core::Env;
use cuno_toolchain::llvm::{ExplicitPaths, find_llvm_tools};
use cuno_toolchain::msvc::{self, SystemRegistry};
use cuno_toolchain::select::{
    self, Choices, CompilerKind, Constraints, Found, ProbeInputs, Selected,
};
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
        explicit_msvc: None,
    };
    let selected =
        select::select(&choices, found, &Constraints::default(), HostOs::Windows).unwrap();
    let sdk = msvc::find_windows_sdk(&env, &SystemRegistry).unwrap();
    let resolved = msvc::resolve(selected.msvc.as_ref().unwrap(), &sdk);
    Some((selected, resolved.apply(&env)))
}

fn probe(selected: &Selected, env: &Env, flags: &[&str]) -> Result<(), ToolchainError> {
    let scratch = tempfile::tempdir().unwrap();
    let flags: Vec<std::ffi::OsString> = flags.iter().map(Into::into).collect();
    select::check(
        selected,
        env,
        ProbeInputs {
            scratch: scratch.path(),
            flags: &flags,
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
        let (define, no_rtti) = match kind {
            CompilerKind::ClangCl => ("/D", "/GR-"),
            _ => ("-D", "-fno-rtti"),
        };
        let d = |macro_: &str| format!("{define}{macro_}");
        probe(&selected, &env, &[]).unwrap_or_else(|e| panic!("{kind:?}: {e}"));
        probe(&selected, &env, &[no_rtti, &d("_HAS_STATIC_RTTI=0")])
            .unwrap_or_else(|e| panic!("{kind:?} without RTTI: {e}"));

        let err = probe(&selected, &env, &[&d("__clang_major__=1")]).unwrap_err();
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
                &d("__clang_major__=1"),
                &d("_ALLOW_COMPILER_AND_STL_VERSION_MISMATCH"),
            ],
        )
        .unwrap_or_else(|e| panic!("{kind:?}: {e}"));
    }
}
