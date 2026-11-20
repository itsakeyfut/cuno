//! The C/C++ compilers integration tests rely on, invoked directly rather
//! than through cuno: if these fail, a failure in a test of `cuno build` or
//! `cuno test` is about the toolchain, not about cuno.
//!
//! Where each compiler is looked for, and what happens when one is missing,
//! is documented in `tests/support/compilers.rs`.

mod support;

use support::Project;
use support::compilers::{Compiler, Language, Missing, Outcome, on_missing, require};

/// Compiles a program printing `greeting` with `compiler` in a fresh
/// project, runs it, and asserts its exact output.
fn hello(compiler: Compiler, language: Language) {
    let (source, body) = match language {
        Language::C => (
            "hello.c",
            "#include <stdio.h>\nint main(void) { fputs(GREETING, stdout); return 0; }\n",
        ),
        Language::Cxx => (
            "hello.cpp",
            "#include <iostream>\nint main() { std::cout << GREETING; }\n",
        ),
    };
    let text = format!("hello from {} {:?}", compiler.name(), language);
    let program = format!("#define GREETING \"{text}\\n\"\n{body}");
    // The C runtime opens stdout in text mode on Windows, so the program's
    // "\n" reaches the pipe as "\r\n".
    let newline = if cfg!(windows) { "\r\n" } else { "\n" };
    let greeting = format!("{text}{newline}");
    // Named unlike the source, so that a compiler falling back to its default
    // output name (`hello.exe` for cl) is caught.
    let exe = format!("greeting{}", std::env::consts::EXE_SUFFIX);
    let project = Project::new().file(source, &program);

    let Some(cmd) = require(compiler, language, source, &exe) else {
        return;
    };
    let compiled = project.run_command(cmd);
    assert_eq!(compiled.code, Some(0), "compiling failed:\n{compiled:?}");

    project
        .run(project.path().join(&exe), &[])
        .assert(0, &greeting, "");
}

/// Mutation: drop `/EHsc` or `/Fe:` from the cl.exe arguments in
/// `Compiler::compile`, or misspell the source in `hello`; this test fails.
#[cfg(windows)]
#[test]
fn msvc_builds_and_runs_c() {
    hello(Compiler::Msvc, Language::C);
}

#[cfg(windows)]
#[test]
fn msvc_builds_and_runs_cxx() {
    hello(Compiler::Msvc, Language::Cxx);
}

/// Mutation: drop `-o` from the clang arguments; a test here fails.
#[test]
fn clang_builds_and_runs_c() {
    hello(Compiler::Clang, Language::C);
}

#[test]
fn clang_builds_and_runs_cxx() {
    hello(Compiler::Clang, Language::Cxx);
}

#[cfg(windows)]
#[test]
fn clang_cl_builds_and_runs_c() {
    hello(Compiler::ClangCl, Language::C);
}

#[cfg(windows)]
#[test]
fn clang_cl_builds_and_runs_cxx() {
    hello(Compiler::ClangCl, Language::Cxx);
}

fn missing() -> Missing {
    Missing {
        compiler: "clang",
        detail: "clang.exe is not on PATH".into(),
    }
}

/// Mutation: let `CUNO_TEST_ALLOW_MISSING` apply on CI in `on_missing`; this
/// test fails.
#[test]
fn missing_compiler_fails_on_ci_even_when_allowed() {
    assert!(matches!(
        on_missing(&missing(), true, true),
        Outcome::Fail(_)
    ));
    assert!(matches!(
        on_missing(&missing(), true, false),
        Outcome::Fail(_)
    ));
}

/// Mutation: make `on_missing` skip by default; this test fails.
#[test]
fn missing_compiler_fails_locally_by_default() {
    assert!(matches!(
        on_missing(&missing(), false, false),
        Outcome::Fail(_)
    ));
}

/// Mutation: make `on_missing` ignore `allow_missing`; this test fails.
#[test]
fn missing_compiler_skips_locally_only_when_allowed() {
    assert_eq!(
        on_missing(&missing(), false, true),
        Outcome::Skip("skipped: clang not found: clang.exe is not on PATH".into())
    );
}

/// Mutation: drop the compiler, the detail or an override variable from the
/// failure message in `on_missing`; this test fails.
#[test]
fn failure_message_names_the_compiler_and_the_overrides() {
    let Outcome::Fail(message) = on_missing(&missing(), false, false) else {
        panic!("a missing compiler should fail");
    };
    for part in [
        "clang not found: clang.exe is not on PATH",
        "CUNO_TEST_VS_INSTALL",
        "CUNO_TEST_LLVM_BIN",
        "CUNO_TEST_ALLOW_MISSING=1",
    ] {
        assert!(message.contains(part), "{part:?} missing from:\n{message}");
    }
}
