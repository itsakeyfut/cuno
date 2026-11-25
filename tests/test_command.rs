//! `cuno test` on every compiler here: it builds a package's tests, runs them,
//! shows the report, and exits 0 only when every test passed. See
//! docs/en/specs/commands.md, "`cuno test [filter]`", and
//! docs/en/specs/testing.md.
//!
//! The expected paths (`target\debug\...\calc.exe`) and the crash code
//! (`0xC0000005`, an access violation) are Windows', where these compilers
//! run; another host will need its own.

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

/// `src/calc.cpp` holding in-source tests: `extra` is added to them, from
/// line 8, which the failure places asserted below count on.
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
/// failed `cuno build` does (`commands.md`, "`cuno test [filter]`").
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
        // No executable listed: nothing ran.
        assert!(!run.stderr.contains("running "), "{name}:\n{}", run.stderr);
    }
}

/// An executable whose tests cannot be listed (here, one name at two
/// places) makes `cuno test` exit 1 although no test failed
/// (`commands.md`, "`cuno test [filter]`").
///
/// Mutation: exit by failed tests alone, ignoring executables that could
/// not be listed; this test fails.
#[test]
fn an_unlisted_executable_exits_1() {
    for (name, compiler) in COMPILERS {
        if !here(compiler) {
            continue;
        }
        let p = project("").file(
            "src/more.cpp",
            "#ifdef CUNO_TEST\n#include <cuno/test.hpp>\n\
             static cuno::test::suite s = [] { cuno::test::test(\"add works\", [] {}); };\n#endif\n",
        );
        let run = p.cuno(&["test", "--compiler", name]);
        assert_eq!(run.code, Some(1), "{name}:\n{}", run.stderr);
        assert!(
            run.stderr
                .contains("error: two tests are named `add works`: "),
            "{name}:\n{}",
            run.stderr
        );
    }
}

/// Run from a subdirectory with `--release` and `-q`: the release build's tests
/// run in the project root with the environment cuno was given, no `Running`
/// line is shown, and no directory of results is left behind; without `-q`, the
/// `Running` path is absolute, the executable not being under the current
/// directory (`commands.md`, "`cuno test [filter]`"; `build.md`, "Output
/// directory").
///
/// Mutation: run the tests elsewhere than the root, hand them another
/// environment, build `dev` whatever the profile, show `Running` under
/// `-q` or relative to the root, or leave the run's directory; this test
/// fails.
#[test]
fn a_release_run_from_a_subdirectory() {
    let probe = "test(\"sees its world\", [] {\n\
                 const char *v = std::getenv(\"CALC_PROBE\");\n\
                 require(v != nullptr);\n\
                 expect(eq(std::string(v), std::string(\"given\")));\n\
                 std::FILE *f = std::fopen(\"cuno.toml\", \"r\");\n\
                 expect(f != nullptr);\n\
                 if (f) std::fclose(f);\n\
                 });\n";
    for (name, compiler) in COMPILERS {
        if !here(compiler) {
            continue;
        }
        let p = project(probe).file(
            "src/calc_headers.h",
            "#include <cstdlib>\n#include <string>\n",
        );
        let source = std::fs::read_to_string(p.path().join("src/calc.cpp")).unwrap();
        std::fs::write(
            p.path().join("src/calc.cpp"),
            format!("#include \"calc_headers.h\"\n{source}"),
        )
        .unwrap();
        let run_in_src = |args: &[&str]| {
            let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_cuno"));
            cmd.args(args).current_dir(p.path().join("src"));
            p.run_command_with(cmd, &[("CALC_PROBE", "given")])
        };
        let quiet = run_in_src(&["-q", "test", "--release", "--compiler", name]);
        assert_eq!(quiet.code, Some(0), "{name}:\n{}", quiet.stderr);
        assert!(
            !quiet.stderr.contains("Running"),
            "{name}:\n{}",
            quiet.stderr
        );
        assert!(
            quiet.stderr.contains("test sees its world ... ok"),
            "{name}:\n{}",
            quiet.stderr
        );
        let left: Vec<_> = std::fs::read_dir(p.path().join("target/release/test"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(left, [std::ffi::OsString::from("calc")], "{name}");

        let shown = run_in_src(&["test", "--release", "--compiler", name]);
        let exe = p
            .path()
            .join("target")
            .join("release")
            .join("test")
            .join("calc")
            .join("bin")
            .join("calc.exe");
        assert!(
            shown
                .stderr
                .contains(&format!("Running `{}`", exe.display())),
            "{name}:\n{}",
            shown.stderr
        );
    }
}

/// Two in-source tests in `calc.c`, written with the C header.
const C_CALC: &str = "#include <cuno/test.h>\n\
     int add(int a, int b) { return a + b; }\n\
     cuno_test(adds) { cuno_check_eq(add(1, 2), 3); }\n\
     cuno_test(adds_zero) { cuno_check_eq(add(0, 0), 0); }\n";

/// Two in-source tests in `calc.cpp`, written with the C++ header.
const CXX_CALC: &str = "int add(int a, int b) { return a + b; }\n\
     #ifdef CUNO_TEST\n\
     #include <cuno/test.hpp>\n\
     static cuno::test::suite tests = [] {\n\
     cuno::test::test(\"adds\", [] { cuno::test::expect(cuno::test::eq(add(1, 2), 3)); });\n\
     cuno::test::test(\"adds zero\", [] { cuno::test::expect(cuno::test::eq(add(0, 0), 0)); });\n\
     };\n\
     #endif\n";

/// A package's files, as `(path, contents)`.
type Files = &'static [(&'static str, &'static str)];

/// The shapes a package's tests come in: its `lang`, and its files, each
/// with two in-source tests and one under `tests/`.
const SHAPES: [(&str, &str, Files); 3] = [
    (
        "c",
        "lang = \"c\"\n",
        &[
            (
                "src/main.c",
                "int add(int a, int b);\nint main(void) { return add(1, 2) == 3 ? 0 : 1; }\n",
            ),
            ("src/calc.c", C_CALC),
            (
                "tests/adds.c",
                "#include <cuno/test.h>\nint add(int a, int b);\ncuno_test(adds_from_tests) { cuno_check_eq(add(1, 1), 2); }\n",
            ),
        ],
    ),
    (
        "c++",
        "",
        &[
            ("src/main.cpp", MAIN),
            ("src/calc.cpp", CXX_CALC),
            ("tests/adds.cpp", INTEGRATION),
        ],
    ),
    (
        "c in c++",
        "",
        &[
            (
                "src/main.cpp",
                "extern \"C\" int add(int a, int b);\nint main() { return add(1, 2) == 3 ? 0 : 1; }\n",
            ),
            ("src/calc.c", C_CALC),
            (
                "tests/adds.cpp",
                "#include <cuno/test.hpp>\nextern \"C\" int add(int, int);\n\
             static cuno::test::suite s = [] { cuno::test::test(\"from tests\", [] {\n\
             cuno::test::expect(cuno::test::eq(add(1, 1), 2)); }); };\n",
            ),
        ],
    ),
];

/// The builds tests are run in: the flags `cuno test` is given, and what
/// the manifest adds.
const BUILDS: [(&str, &[&str], &str); 3] = [
    ("dev", &[], ""),
    ("release", &["--release"], ""),
    (
        "release with lto",
        &["--release"],
        "[profile.release]\nlto = true\n",
    ),
];

/// Every shape of package, in every build, on every compiler: `cuno test`
/// runs both in-source tests in the package's executable and the one test
/// in the `tests/` executable, and says so in the `test result:` lines.
/// The ways tests have been lost before each have a guard of their own;
/// this one holds the count through cuno's own flags, `lto` included
/// (`testing.md`, "Execution").
///
/// Mutations, each failing this test, naming the shape, build and
/// compiler: drop the `/include` pragma that keeps a C registration in
/// `headers/cuno/test.h` (`c, release with lto, msvc`); compile the test
/// build without `CUNO_TEST`, with the assert in `graph::plan` removed
/// (every combination); run only the first test executable (the `tests/`
/// line missing).
#[test]
fn no_test_is_lost_in_any_build() {
    let want = "ok. 2 passed; 0 failed; 0 skipped\nok. 1 passed; 0 failed; 0 skipped\n";
    for (shape, lang, files) in SHAPES {
        for (build, flags, profile) in BUILDS {
            let manifest = format!("{MANIFEST}{lang}{profile}");
            for (name, compiler) in COMPILERS {
                if !here(compiler) {
                    continue;
                }
                let p = files.iter().fold(
                    Project::new().file("cuno.toml", &manifest),
                    |p, (path, text)| p.file(path, text),
                );
                let at = format!("{shape}, {build}, {name}");
                let run = p.cuno(&[&["-q", "test", "--compiler", name][..], flags].concat());
                let err = run.stderr.replace("\r\n", "\n");
                assert_eq!(run.code, Some(0), "{at}:\n{err}");
                let results: String = err
                    .lines()
                    .filter_map(|line| line.strip_prefix("test result: "))
                    .map(|line| {
                        let kept = line.split_once("; finished in ").map_or(line, |(k, _)| k);
                        format!("{kept}\n")
                    })
                    .collect();
                assert_eq!(results, want, "{at}:\n{err}");
            }
        }
    }
}
