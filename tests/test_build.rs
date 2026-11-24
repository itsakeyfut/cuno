//! The test build (`cuno_build::pipeline`, `Goal::Test`) with each real
//! compiler: a package with in-source tests and tests under `tests/` builds
//! into test executables that list and run their own tests, and leaves the
//! program alone. `cuno test` will run the same build from the command
//! line; until it exists the pipeline is driven here as the binary drives it.
//! See docs/en/specs/testing.md and docs/en/specs/build.md.

mod support;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use cuno_build::BuildError;
use cuno_build::exec::SystemSpawner;
use cuno_build::pipeline::{self, BuiltTest, Goal, Request, Toolchain};
use cuno_core::{Env, NameCase, Shell};
use cuno_manifest::{CliSettings, Inputs, Profile};
use cuno_toolchain::msvc::SystemRegistry;
use cuno_toolchain::select::{self, Found};
use cuno_toolchain::{HostOs, SystemRunner, prepare};

use support::Project;
use support::compilers::{Compiler, Language, require};

const COMPILERS: [(&str, Compiler); 3] = [
    ("msvc", Compiler::Msvc),
    ("clang", Compiler::Clang),
    ("clang-cl", Compiler::ClangCl),
];

/// The environment cuno would see: this process's, without any `CUNO_*`
/// variable, and with the project's cuno home.
fn environment(p: &Project) -> Env {
    let vars = std::env::vars_os()
        .filter(|(name, _)| {
            !name
                .to_string_lossy()
                .to_ascii_uppercase()
                .starts_with("CUNO_")
        })
        .chain([(OsString::from("CUNO_HOME"), p.home().as_os_str().to_owned())]);
    Env::from_pairs(vars, NameCase::host())
}

/// Builds the project's tests with `compiler` as `cuno test` will: the
/// settings, the toolchain, then the pipeline with `Goal::Test` and the
/// repository's headers as the bundled ones. Panics, with what the build
/// printed, unless it succeeds.
fn build_tests(p: &Project, compiler: &str) -> Vec<BuiltTest> {
    match try_build_tests(p, compiler) {
        Ok(tests) => tests,
        Err((e, output)) => panic!("{compiler}: {e}\n{output}"),
    }
}

/// [`build_tests`], returning a failure with what the build printed.
fn try_build_tests(p: &Project, compiler: &str) -> Result<Vec<BuiltTest>, (BuildError, String)> {
    let env = environment(p);
    let host = HostOs::Windows;
    let cli = CliSettings {
        compiler: Some(compiler.to_owned()),
        ..CliSettings::default()
    };
    let settings = cuno_manifest::resolve(Inputs {
        cwd: p.path(),
        env: &env,
        cli: &cli,
    })
    .expect("the settings")
    .value;
    let (project, manifest) = settings.project.as_ref().expect("a project");
    let runner = SystemRunner::new();
    let detected = prepare::detect(&env, host, &settings.llvm_paths(), &runner);
    let installs = detected.installs.expect("Visual Studio");
    let llvm = detected.llvm.expect("LLVM");
    let found = Found {
        installs: &installs,
        llvm: &llvm,
        explicit_msvc: settings.msvc_path(),
    };
    let selected =
        select::select(&settings.compiler, found, &settings.constraints, host).expect("a compiler");
    let prepared = prepare::prepare(&selected, &env, &SystemRegistry).expect("its environment");
    let version = selected.version.to_string();
    let bundled = Path::new(env!("CARGO_MANIFEST_DIR")).join("headers");
    let request = Request {
        root: &project.root,
        manifest,
        target_dir: &settings.target_dir,
        profile: Profile::Dev,
        goal: Goal::Test,
        host,
        bundled: &bundled,
        jobs: 4,
        verbose: false,
        quiet: true,
        started: Instant::now(),
        toolchain: Toolchain {
            kind: selected.kind,
            executable: &selected.executable,
            version: &selected.version,
            msvc: selected.msvc.as_ref().map(|toolset| &toolset.cl_version),
            env: &prepared.env,
            identity: prepared
                .identity(&version)
                .into_iter()
                .map(|s| s.to_owned())
                .collect(),
        },
        spawner: &SystemSpawner,
    };
    let output = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut shell = Shell::new(Box::new(Captured(output.clone())), None, &env, false);
    pipeline::build(&request, &mut shell)
        .map(|built| built.test_executables)
        .map_err(|e| {
            (
                e,
                String::from_utf8_lossy(&output.lock().unwrap()).into_owned(),
            )
        })
}

/// A shell's output, kept to show when the build fails.
struct Captured(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The names `exe --list` reports whose place is under `tests/`, the ones
/// cuno runs from an executable made from `tests/`, or (with `in_tests`
/// false) the others, the ones it runs from the in-source tests'
/// (`testing.md`, "Execution").
fn listed_where(p: &Project, exe: &Path, in_tests: bool) -> Vec<String> {
    let tests = p.path().join("tests");
    listed_with_files(p, exe)
        .into_iter()
        .filter(|(_, file)| Path::new(file).starts_with(&tests) == in_tests)
        .map(|(name, _)| name)
        .collect()
}

/// Each registration `exe --list` reports: its name and file.
fn listed_with_files(p: &Project, exe: &Path) -> Vec<(String, String)> {
    let result = p.path().join("list.txt");
    let mut cmd = Command::new(exe);
    cmd.arg("--list");
    let run = p.run_command_with(cmd, &[("CUNO_TEST_RESULT", result.to_str().unwrap())]);
    assert_eq!(run.code, Some(0), "{}: {}", exe.display(), run.stderr);
    std::fs::read_to_string(&result)
        .unwrap()
        .lines()
        .map(|record| {
            let fields: Vec<&str> = record.split('\t').collect();
            (fields[1].to_owned(), fields[2].replace("\\\\", "\\"))
        })
        .collect()
}

const MANIFEST: &str = "[package]\nname = \"calc\"\nversion = \"0.1.0\"\n";

const CALC: &str = r#"int add(int a, int b) { return a + b; }

#ifdef CUNO_TEST
#include <cuno/test.hpp>
static cuno::test::suite tests = [] {
    using namespace cuno::test;
    test("add in source", [] { expect(eq(add(1, 2), 3)); });
};
#endif
"#;

const MAIN: &str = r#"int add(int a, int b);
int main() { return add(1, 2) == 3 ? 0 : 1; }
"#;

const SUB: &str = "int sub(int a, int b) { return a - b; }
";

const HELPER: &str = "int twice(int x) { return 2 * x; }\n";

const ADDS: &str = r#"#include <cuno/test.hpp>
int add(int a, int b);
int twice(int x);
static cuno::test::suite tests = [] {
    using namespace cuno::test;
    test("add from tests", [] { expect(eq(twice(add(1, 1)), 4)); });
};
"#;

const C_TEST: &str = r#"#include <cuno/test.h>
int sub(int a, int b);
cuno_test(sub_from_c) { cuno_check_eq(sub(2, 2), 0); }
"#;

/// On every compiler, the in-source tests build into one executable, and
/// each source under `tests/` into one of its own with the helpers and the
/// body, C and C++ alike, every source compiled once with `CUNO_TEST`; the
/// tests cuno runs from each (the in-source ones from the first, those
/// under `tests/` from the others) list and pass; the entry file and the
/// program are left alone (`project-layout.md`, "Link rules").
///
/// Mutations, each failing this test: link the entry file into a test
/// executable (`main` twice); leave the helpers or the body out of a
/// `tests/` executable (unresolved symbols); compile without `CUNO_TEST`
/// (no in-source test); build the program instead.
#[test]
fn each_test_executable_holds_its_own_tests() {
    for (name, compiler) in COMPILERS {
        if require(compiler, Language::C, "x.c", "x.exe").is_none() {
            continue;
        }
        let p = Project::new()
            .file("cuno.toml", MANIFEST)
            .file("src/main.cpp", MAIN)
            .file("src/calc.cpp", CALC)
            .file("src/sub.c", SUB)
            .file("tests/adds.cpp", ADDS)
            .file("tests/c_test.c", C_TEST)
            .file("tests/common/helper.cpp", HELPER);
        let tests = build_tests(&p, name);
        let names: Vec<&str> = tests.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["calc", "tests-adds", "tests-c_test"], "{name}");
        let held: Vec<Vec<String>> = tests
            .iter()
            .map(|t| listed_where(&p, &t.path, t.file.is_some()))
            .collect();
        assert_eq!(
            held,
            [
                vec!["add in source".to_owned()],
                vec!["add from tests".to_owned()],
                vec!["sub_from_c".to_owned()],
            ],
            "{name}"
        );
        for (test, names) in tests.iter().zip(&held) {
            for test_name in names {
                let mut cmd = Command::new(&test.path);
                cmd.args(["--exact", test_name]);
                let result = p.path().join("result.txt");
                let run =
                    p.run_command_with(cmd, &[("CUNO_TEST_RESULT", result.to_str().unwrap())]);
                assert_eq!(
                    (run.code, std::fs::read_to_string(&result).unwrap()),
                    (Some(0), "done\n".to_owned()),
                    "{name}: {test_name}"
                );
            }
        }
        let program: PathBuf = p.path().join("target").join("debug").join("calc.exe");
        assert!(!program.exists(), "{name}: the program was built");
    }
}

/// A type whose shape changes under `CUNO_TEST` is the same type in a test
/// under `tests/` and in the body it calls, so a correct program passes
/// (`project-layout.md`, "Link rules").
///
/// Mutation: link the `tests/` executables to a body compiled without
/// `CUNO_TEST`; `set` then writes where the test does not read, and this
/// test fails.
#[test]
fn a_test_and_the_body_see_one_definition() {
    for (name, compiler) in COMPILERS {
        if require(compiler, Language::C, "x.c", "x.exe").is_none() {
            continue;
        }
        let p = Project::new()
            .file("cuno.toml", MANIFEST)
            .file("src/main.cpp", "int main() { return 0; }\n")
            .file(
                "src/calc.h",
                "struct Calc {\n#ifdef CUNO_TEST\n    int calls = 0;\n#endif\n    int value = 0;\n};\nvoid set(Calc &c, int v);\n",
            )
            .file(
                "src/calc.cpp",
                "#include \"calc.h\"\nvoid set(Calc &c, int v) { c.value = v; }\n",
            )
            .file(
                "tests/calc.cpp",
                "#include <cuno/test.hpp>\n#include \"calc.h\"\n\
                 static cuno::test::suite s = [] { using namespace cuno::test;\n\
                 test(\"set\", [] { Calc c; set(c, 7); expect(eq(c.value, 7)); }); };\n",
            );
        let tests = build_tests(&p, name);
        let mut cmd = Command::new(&tests[1].path);
        cmd.args(["--exact", "set"]);
        let result = p.path().join("result.txt");
        let run = p.run_command_with(cmd, &[("CUNO_TEST_RESULT", result.to_str().unwrap())]);
        assert_eq!(
            (run.code, std::fs::read_to_string(&result).unwrap()),
            (Some(0), "done\n".to_owned()),
            "{name}"
        );
    }
}

/// A `main` in a file under `tests/` is refused by the linker rather than
/// taking the runner's place, which would run no test at all
/// (`headers/cuno/test_main.c`).
///
/// Mutation: give the runtime `wmain` again; the link succeeds and this
/// test fails.
#[test]
fn a_main_under_tests_does_not_replace_the_runner() {
    for (name, compiler) in COMPILERS {
        if require(compiler, Language::C, "x.c", "x.exe").is_none() {
            continue;
        }
        let p = Project::new()
            .file("cuno.toml", MANIFEST)
            .file("src/main.cpp", "int main() { return 0; }\n")
            .file("tests/mine.cpp", "int main() { return 0; }\n");
        let result = try_build_tests(&p, name);
        assert!(
            matches!(result, Err((BuildError::Failed { link: true, .. }, _))),
            "{name}: {result:?}"
        );
    }
}

/// The runner over what the build made, on every compiler: each test runs
/// alone and is judged by what it reported, a crash fails that test only,
/// an in-source test runs once though every executable holds it, and the
/// counts add up (`testing.md`, "Execution").
///
/// Mutations, each failing this test: run every listed test from a
/// `tests/` executable (the in-source test counts three times); trust the
/// exit code over the result file (the crash passes).
#[test]
fn the_runner_judges_what_the_build_made() {
    for (name, compiler) in COMPILERS {
        if require(compiler, Language::C, "x.c", "x.exe").is_none() {
            continue;
        }
        let p = Project::new()
            .file("cuno.toml", MANIFEST)
            .file("src/main.cpp", MAIN)
            .file("src/calc.cpp", CALC)
            .file("src/sub.c", SUB)
            .file("tests/adds.cpp", ADDS)
            .file(
                "tests/fails.c",
                "#include <cuno/test.h>\n#include <stdio.h>\n#include <stdlib.h>\n\
                 int sub(int a, int b);\n\
                 cuno_test(sub_is_wrong) { puts(\"said by the test\"); cuno_check_eq(sub(3, 1), 1); }\n\
                 cuno_test(crashes) { abort(); }\n\
                 cuno_test(skipped_one) { cuno_skip(\"later\"); }\n",
            )
            .file("tests/common/helper.cpp", HELPER);
        let tests = build_tests(&p, name);
        let executables: Vec<cuno_test::TestExecutable> = tests
            .iter()
            .map(|t| cuno_test::TestExecutable {
                path: t.path.clone(),
                integration: t.file.is_some(),
            })
            .collect();
        let scratch = tempfile::tempdir().unwrap();
        let env = environment(&p);
        let output = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut shell = Shell::new(Box::new(Captured(output.clone())), None, &env, false);
        let summary = cuno_test::run(
            &cuno_test::TestRun {
                executables: &executables,
                filter: None,
                nocapture: false,
                jobs: 4,
                root: p.path(),
                cwd: p.path(),
                quiet: false,
                env: &env,
                scratch: scratch.path(),
                case: NameCase::host(),
                launcher: &cuno_test::SystemLauncher,
            },
            &mut shell,
        );
        let text = String::from_utf8_lossy(&output.lock().unwrap()).replace("\r\n", "\n");
        assert_eq!(
            (
                summary.passed,
                summary.failed,
                summary.skipped,
                summary.unlisted
            ),
            (2, 2, 1, 0),
            "{name}:\n{text}"
        );
        assert!(
            text.contains("---- sub_is_wrong ----\ntests/fails.c:5: eq(sub(3, 1), 1) failed\n  left:  2\n  right: 1\nsaid by the test\n"),
            "{name}:\n{text}"
        );
        assert!(
            text.contains(
                "---- crashes ----\nthe test ended without reporting a result (exit code "
            ),
            "{name}:\n{text}"
        );
        assert_eq!(
            text.matches("test add in source ... ok").count(),
            1,
            "{name}:\n{text}"
        );
    }
}
