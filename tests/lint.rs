//! `cuno lint` run as a process, on each compiler found with clang-tidy:
//! what it analyses, shows, fixes and exits with
//! (`docs/en/specs/commands.md`, "`cuno lint`";
//! `docs/en/specs/dev-tools.md`, "`[lint]`").

mod support;

use support::Project;
use support::compilers::{Compiler, Language, require, require_llvm_tool};

const MANIFEST: &str = "[package]\nname = \"app\"\nversion = \"0.1.0\"\n";

/// Code with a `modernize-use-nullptr` finding.
const ZERO: &str = "int main() {\n    int* p = 0;\n    return p == 0 ? 0 : 1;\n}\n";

const COMPILERS: [(&str, Compiler); 3] = [
    ("msvc", Compiler::Msvc),
    ("clang", Compiler::Clang),
    ("clang-cl", Compiler::ClangCl),
];

/// The compilers here, when clang-tidy is too.
fn compilers() -> Vec<&'static str> {
    if require_llvm_tool("clang-tidy").is_none() {
        return Vec::new();
    }
    COMPILERS
        .iter()
        .filter(|(_, c)| require(*c, Language::Cxx, "x.cpp", "x.exe").is_some())
        .map(|(name, _)| *name)
        .collect()
}

fn package(lint: &str) -> Project {
    Project::new()
        .file("cuno.toml", &format!("{MANIFEST}[lint]\n{lint}"))
        .file("src/main.cpp", ZERO)
        .file(
            "src/third_party/v.cpp",
            "void v() { int* q = 0; (void)q; }\n",
        )
}

/// A preset finding is a warning, exit 0; the same check in `deny` is an
/// error, exit 1; an excluded file is not analysed; the last line counts
/// files and findings.
///
/// Mutation: ignore `deny`, analyse an excluded file, or count a warning
/// as an error; this test fails.
#[test]
fn warnings_pass_and_deny_fails() {
    for name in compilers() {
        let project = package(
            "preset = \"none\"\nwarn = [\"modernize-use-nullptr\"]\nexclude = [\"src/third_party/**\"]\n",
        );
        let run = project.cuno(&["lint", "--compiler", name]);
        assert_eq!(run.code, Some(0), "{name}: {}", run.stderr);
        assert!(
            run.stderr
                .contains("warning: use nullptr [modernize-use-nullptr]"),
            "{name}: {}",
            run.stderr
        );
        assert!(!run.stderr.contains("v.cpp"), "{name}: {}", run.stderr);
        assert!(
            run.stderr
                .ends_with("      Linted 1 file: 2 warnings, 0 errors\n"),
            "{name}: {}",
            run.stderr
        );

        let project = package(
            "preset = \"none\"\ndeny = [\"modernize-use-nullptr\"]\nexclude = [\"src/third_party/**\"]\n",
        );
        let run = project.cuno(&["lint", "--compiler", name]);
        assert_eq!(run.code, Some(1), "{name}: {}", run.stderr);
        assert!(
            run.stderr.contains("error: use nullptr"),
            "{name}: {}",
            run.stderr
        );
        assert!(
            run.stderr.ends_with("0 warnings, 2 errors\n"),
            "{name}: {}",
            run.stderr
        );
    }
}

/// A source that does not compile is an error, exit 1.
///
/// Mutation: drop clang-tidy's compile errors as noise; this test fails.
#[test]
fn a_source_that_does_not_compile_fails() {
    for name in compilers() {
        let project = package("")
            .file("src/main.cpp", "int main() { return undeclared; }\n")
            .file("src/third_party/v.cpp", "void v() {}\n");
        let run = project.cuno(&["lint", "--compiler", name]);
        assert_eq!(run.code, Some(1), "{name}: {} {}", run.stderr, run.stderr);
        assert!(run.stderr.contains("undeclared"), "{name}: {}", run.stderr);
    }
}

/// `--fix` fixes the source and does not count what it fixed.
///
/// Mutation: skip `--fix`, or count a fixed finding as an error; this test
/// fails.
#[test]
fn fix_fixes_the_source() {
    let Some(name) = compilers().first().copied() else {
        return;
    };
    let project = package("preset = \"none\"\ndeny = [\"modernize-use-nullptr\"]\n");
    let run = project.cuno(&["lint", "--fix", "--compiler", name]);
    assert_eq!(run.code, Some(0), "{name}: {} {}", run.stderr, run.stderr);
    let fixed = std::fs::read_to_string(project.path().join("src/main.cpp")).unwrap();
    assert!(fixed.contains("int* p = nullptr;"), "{fixed}");
}

/// With no check enabled, nothing runs: a warning, exit 0. lint takes no
/// profile.
///
/// Mutation: run clang-tidy with no checks, or accept `--release`; this
/// test fails.
#[test]
fn no_checks_and_no_profile() {
    let project = package("preset = \"none\"\n");
    project.cuno(&["lint"]).assert(
        0,
        "",
        "warning: no checks are enabled ([lint] preset = \"none\" and no warn / deny); nothing was analysed\n",
    );
    let run = project.cuno(&["lint", "--release"]);
    assert_eq!(run.code, Some(2), "{}", run.stderr);
}
