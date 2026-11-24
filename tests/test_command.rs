//! `cuno test` on every compiler here: it builds a package's tests, runs
//! them, shows the report, and exits 0 only when every test passed. See
//! docs/en/specs/commands.md, "`cuno test`", and docs/en/specs/testing.md.

mod support;

use support::Project;
use support::compilers::{Compiler, Language, require};

const COMPILERS: [(&str, Compiler); 3] = [
    ("msvc", Compiler::Msvc),
    ("clang", Compiler::Clang),
    ("clang-cl", Compiler::ClangCl),
];

fn here(compiler: Compiler) -> bool {
    require(compiler, Language::C, "x.c", "x.exe").is_some()
}

const MANIFEST: &str = "[package]\nname = \"calc\"\nversion = \"0.1.0\"\n";
const MAIN: &str = "int add(int a, int b);\nint main() { return add(1, 2) == 3 ? 0 : 1; }\n";

/// `src/calc.cpp` holding in-source tests: `extra` is added to them.
fn calc(extra: &str) -> String {
    format!(
        "#include <cstdio>\n\
         int add(int a, int b) {{ return a + b; }}\n\
         #ifdef CUNO_TEST\n\
         #include <cuno/test.hpp>\n\
         static cuno::test::suite tests = [] {{\n\
         using namespace cuno::test;\n\
         test(\"add works\", [] {{ std::puts(\"from add works\"); expect(eq(add(1, 2), 3)); }});\n\
         {extra}\
         }};\n\
         #endif\n"
    )
}

const INTEGRATION: &str = "#include <cuno/test.hpp>\nint add(int, int);\n\
     static cuno::test::suite s = [] { cuno::test::test(\"from tests\", [] {\n\
     cuno::test::expect(cuno::test::eq(add(1, 1), 2)); }); };\n";

fn project(extra: &str) -> Project {
    Project::new()
        .file("cuno.toml", MANIFEST)
        .file("src/main.cpp", MAIN)
        .file("src/calc.cpp", &calc(extra))
        .file("tests/adds.cpp", INTEGRATION)
}

/// A package whose tests all pass: both executables run, the report shows
/// each test once, a passing test's output is not shown, and the exit code
/// is 0.
///
/// Mutation: exit with anything but 0 on success, skip the `tests/`
/// executable, or show a passing test's output; this test fails.
#[test]
fn a_passing_package_exits_0() {
    for (name, compiler) in COMPILERS {
        if !here(compiler) {
            continue;
        }
        let p = project("");
        let run = p.cuno(&["test", "--compiler", name]);
        assert_eq!(run.code, Some(0), "{name}:\n{}", run.stderr);
        let err = run.stderr.replace("\r\n", "\n");
        for line in [
            "test add works ... ok\n",
            "test from tests ... ok\n",
            "test result: ok. 1 passed; 0 failed; 0 skipped; finished in ",
        ] {
            assert!(err.contains(line), "{name}: no {line:?} in\n{err}");
        }
        assert_eq!(err.matches("test add works").count(), 1, "{name}:\n{err}");
        assert!(!err.contains("from add works"), "{name}:\n{err}");
        assert!(
            err.contains("Running `target\\debug\\test\\calc\\bin\\calc.exe`"),
            "{name}:\n{err}"
        );
    }
}

/// A failing test and a crashing one: exit code 1, each reported FAILED
/// with its place, its values and its output, the crash with its exit
/// code, and the other tests still run.
///
/// Mutation: exit 0 on a failure, trust a crashed test's exit code, or
/// hide a failure's output; this test fails.
#[test]
fn a_failing_test_exits_1_with_its_output() {
    let extra = "test(\"add is wrong\", [] { std::puts(\"said before failing\"); expect(eq(add(2, 2), 5)); });\n\
                 test(\"crashes\", [] { int *volatile p = nullptr; *p = 1; });\n";
    for (name, compiler) in COMPILERS {
        if !here(compiler) {
            continue;
        }
        let p = project(extra);
        let run = p.cuno(&["test", "--compiler", name]);
        assert_eq!(run.code, Some(1), "{name}:\n{}", run.stderr);
        let err = run.stderr.replace("\r\n", "\n");
        for line in [
            "test add works ... ok\n",
            "test add is wrong ... FAILED\n",
            "test crashes ... FAILED\n",
            "---- add is wrong ----\nsrc/calc.cpp:8: eq failed\n  left:  4\n  right: 5\nsaid before failing\n",
            "---- crashes ----\nthe test ended without reporting a result (exit code 0xC0000005)\n",
            "test result: FAILED. 1 passed; 2 failed; 0 skipped; finished in ",
            "test from tests ... ok\n",
        ] {
            assert!(err.contains(line), "{name}: no {line:?} in\n{err}");
        }
    }
}

/// The filter applies to every executable, one with no match still shows
/// `running 0 tests`, and `--nocapture` shows a passing test's output.
///
/// Mutation: drop the filter, skip an executable it leaves empty, or ignore
/// `--nocapture`; this test fails.
#[test]
fn the_filter_and_nocapture() {
    for (name, compiler) in COMPILERS {
        if !here(compiler) {
            continue;
        }
        let p = project("");
        let run = p.cuno(&["test", "--compiler", name, "--nocapture", "works"]);
        assert_eq!(run.code, Some(0), "{name}:\n{}", run.stderr);
        let err = run.stderr.replace("\r\n", "\n");
        assert!(
            err.contains("running 1 tests\ntest add works ... ok\nfrom add works\n"),
            "{name}:\n{err}"
        );
        assert!(err.contains("running 0 tests\n"), "{name}:\n{err}");
        assert!(!err.contains("from tests"), "{name}:\n{err}");
    }
}

/// A test build that does not compile exits 1 and runs nothing, as a
/// failed `cuno build` does (`commands.md`, "`cuno test`").
///
/// Mutation: run the tests after a failed build, or exit 0; this test
/// fails.
#[test]
fn a_failed_build_exits_1() {
    for (name, compiler) in COMPILERS {
        if !here(compiler) {
            continue;
        }
        let p = project("test(\"broken\", [] { this does not compile; });\n");
        let run = p.cuno(&["test", "--compiler", name]);
        assert_eq!(run.code, Some(1), "{name}:\n{}", run.stderr);
        assert!(
            run.stderr.contains("error: could not compile `calc`"),
            "{name}:\n{}",
            run.stderr
        );
        assert!(!run.stderr.contains("running"), "{name}:\n{}", run.stderr);
    }
}
