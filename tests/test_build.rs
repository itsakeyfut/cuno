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

use cuno_build::exec::SystemSpawner;
use cuno_build::pipeline::{self, Goal, Request, TestExecutable, Toolchain};
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
/// repository's headers as the bundled ones.
fn build_tests(p: &Project, compiler: &str) -> Vec<TestExecutable> {
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
    match pipeline::build(&request, &mut shell) {
        Ok(built) => built.tests,
        Err(e) => panic!(
            "{compiler}: {e}\n{}",
            String::from_utf8_lossy(&output.lock().unwrap())
        ),
    }
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

/// The names `exe --list` reports, in order.
fn listed(p: &Project, exe: &Path) -> Vec<String> {
    let result = p.path().join("list.txt");
    let mut cmd = Command::new(exe);
    cmd.arg("--list");
    let run = p.run_command_with(cmd, &[("CUNO_TEST_RESULT", result.to_str().unwrap())]);
    assert_eq!(run.code, Some(0), "{}: {}", exe.display(), run.stderr);
    std::fs::read_to_string(&result)
        .unwrap()
        .lines()
        .map(|record| record.split('\t').nth(1).unwrap().to_owned())
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

/// On every compiler, the in-source tests build into one executable that
/// holds only them, and each source under `tests/` into one of its own
/// with the helpers and the body, C and C++ alike; each lists and passes
/// its tests; the entry file and the program are left alone
/// (`project-layout.md`, "Link rules").
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
        let held: Vec<Vec<String>> = tests.iter().map(|t| listed(&p, &t.path)).collect();
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
