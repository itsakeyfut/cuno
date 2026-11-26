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

/// `--fix` never rewrites a file outside the package: a source whose fix
/// would also change a declaration in an `include-dirs` header is left
/// unfixed and said to be; a compile error in such a header is shown and
/// fails the run.
///
/// Mutation: apply fixes without looking where they go, or drop a foreign
/// compile error; this test fails.
#[test]
fn fix_leaves_other_packages_alone_and_their_errors_show() {
    let Some(name) = compilers().first().copied() else {
        return;
    };
    let vend = "#include <string>\nvoid f(std::string s);\n";
    let project = Project::new()
        .file(
            "cuno.toml",
            &format!(
                "{MANIFEST}[build]\ninclude-dirs = [\"vend\"]\n[lint]\npreset = \"none\"\nwarn = [\"performance-unnecessary-value-param\"]\n"
            ),
        )
        .file("vend/v.h", vend)
        .file(
            "src/main.cpp",
            "#include \"v.h\"\nvoid f(std::string s) { (void)s.size(); }\nint main() { return 0; }\n",
        );
    let run = project.cuno(&["lint", "--fix", "--compiler", name]);
    assert!(
        run.stderr
            .contains("warning: `src/main.cpp` is not fixed: a fix would change `"),
        "{name}: {}",
        run.stderr
    );
    assert_eq!(
        std::fs::read_to_string(project.path().join("vend/v.h")).unwrap(),
        vend
    );

    std::fs::write(project.path().join("vend/v.h"), "#include \"nope.h\"\n").unwrap();
    let run = project.cuno(&["lint", "--compiler", name]);
    assert_eq!(run.code, Some(1), "{name}: {}", run.stderr);
    assert!(
        run.stderr.contains("'nope.h' file not found"),
        "{name}: {}",
        run.stderr
    );
}

/// A package fresh from `cuno new`, C or C++, lints clean: nothing in the
/// templates or in cuno's bundled test headers is a finding under the
/// default preset.
///
/// Mutation: put `bugprone-throwing-static-initialization` back in the
/// preset, or drop the NOLINT on `CUNO_TEST_VALUE`'s conditional; this
/// test fails.
#[test]
fn a_fresh_package_lints_clean() {
    for name in compilers() {
        for lang in ["c++", "c"] {
            let parent = Project::new();
            let made = parent.cuno(&["new", "app", "--lang", lang, "--vcs", "none"]);
            assert_eq!(made.code, Some(0), "{}", made.stderr);
            let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_cuno"));
            cmd.args(["lint", "--compiler", name])
                .current_dir(parent.path().join("app"));
            let run = parent.run_command(cmd);
            assert_eq!(
                (run.code, run.stderr.as_str()),
                (Some(0), "      Linted 2 files: 0 warnings, 0 errors\n"),
                "{name} {lang}"
            );
        }
    }
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

/// A temporary directory that does not exist stops lint, at its first
/// check (the compiler's or clang-tidy's), with the shared help, in a
/// directory named for lint, and never says it was to compile: lint runs
/// no compiler (`architecture.md`, "What goes in `cuno-core`"). Lint's own
/// directory for analysing comes after those checks, so a missing
/// temporary directory never reaches it.
///
/// Mutation: name lint's directories `cuno-build-`, run the build's
/// check before compiling for lint, or drop the help; this test fails.
#[test]
fn a_missing_temporary_directory_stops_lint_with_the_shared_help() {
    for compiler in compilers() {
        let project = package("");
        let temp = project.home().join("missing").join("temp");
        let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_cuno"));
        cmd.args(["lint", "--compiler", compiler])
            .env("TEMP", &temp)
            .env("TMP", &temp);
        let run = project.run_command(cmd);
        assert_eq!(run.code, Some(3), "{compiler}: {}", run.stderr);
        assert!(
            run.stderr.contains(&format!(
                "error: cannot create {}\\cuno-lint-",
                temp.display()
            )),
            "{compiler}: {}",
            run.stderr
        );
        assert!(
            !run.stderr.contains(" to compile: "),
            "{compiler}: {}",
            run.stderr
        );
        assert!(
            run.stderr.ends_with(
                "help: set TMP and TEMP to a directory that exists and can be written
"
            ),
            "{compiler}: {}",
            run.stderr
        );
    }
}
