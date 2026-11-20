//! Tests of the shared test support in `tests/support`, so that a defect in
//! the helpers shows up here rather than as a confusing failure elsewhere.

mod support;

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;

use support::compilers::{Missing, Outcome, on_missing};
use support::{Project, Run, hermetic};

/// Mutation: make `Project::file` write to `path` relative to the current
/// directory, or skip creating parent directories; this test fails.
#[test]
fn files_are_written_under_the_project() {
    let project = Project::new().file("a/b.txt", "contents\n");
    let read = std::fs::read_to_string(project.path().join("a").join("b.txt"))
        .expect("the file should exist under the project");
    assert_eq!(read, "contents\n");
}

/// Run only as the program a test starts through `Project::run` (see
/// `observe`): prints its working directory and environment as UTF-8.
///
/// The tests ask this test binary rather than `cmd /C cd`, `set` or `env`,
/// because those print paths in the console's code page (CP932 on Japanese
/// Windows), and a temp directory under a non-ASCII user name would then not
/// decode at all.
#[test]
#[ignore = "started as a child program by observe()"]
fn report_directory_and_environment() {
    println!("cwd={}", std::env::current_dir().unwrap().display());
    for (name, value) in std::env::vars_os() {
        println!("env:{}={}", name.to_string_lossy(), value.to_string_lossy());
    }
}

/// What a program run through `project` sees: its working directory and its
/// environment, from `report_directory_and_environment`.
fn observe(project: &Project) -> (PathBuf, Vec<(String, String)>) {
    let run = project.run(
        std::env::current_exe().unwrap(),
        &[
            "--exact",
            "report_directory_and_environment",
            "--ignored",
            "--nocapture",
        ],
    );
    assert_eq!(run.code, Some(0), "{run:?}");
    let cwd = run
        .stdout
        .lines()
        .find_map(|line| line.strip_prefix("cwd="))
        .unwrap_or_else(|| panic!("no cwd= line:\n{}", run.stdout));
    let env = run
        .stdout
        .lines()
        .filter_map(|line| line.strip_prefix("env:")?.split_once('='))
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect();
    (PathBuf::from(cwd), env)
}

/// The value of `name` in an environment from `observe`.
fn var<'a>(env: &'a [(String, String)], name: &str) -> Option<&'a str> {
    env.iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// Mutation: remove `.current_dir(self.path())` from `Project::run_command`;
/// the child then runs in the test's own directory and this test fails.
#[test]
fn programs_run_in_the_project_directory() {
    let project = Project::new();
    let (cwd, _) = observe(&project);
    assert_eq!(
        std::fs::canonicalize(cwd).unwrap(),
        std::fs::canonicalize(project.path()).unwrap()
    );
}

/// Mutation: give `Project::new` a fixed directory name (`.rand_bytes(0)`);
/// the second project then cannot get a directory of its own and this test
/// fails.
#[test]
fn projects_do_not_share_a_directory() {
    let first = Project::new().file("same.txt", "first");
    let second = Project::new().file("same.txt", "second");
    assert_ne!(first.path(), second.path());
    assert_eq!(
        std::fs::read_to_string(first.path().join("same.txt")).unwrap(),
        "first"
    );
    assert_eq!(
        std::fs::read_to_string(second.path().join("same.txt")).unwrap(),
        "second"
    );
}

/// Mutation: remove either loop from `hermetic`, or drop the upper-casing of
/// the name; a colour or `CUNO_*` variable is then left set and this test
/// fails.
#[test]
fn hermetic_removes_colour_and_cuno_variables() {
    let mut cmd = Command::new("unused");
    cmd.env("CUNO_PROBE", "1")
        .env("CLICOLOR_FORCE", "1")
        .env("cuno_lower", "1")
        .env("KEEP_ME", "1");
    let inherited = ["CUNO_PROBE", "cuno_lower", "KEEP_ME", "PATH"].map(OsString::from);
    hermetic(&mut cmd, inherited.into_iter());

    let envs: Vec<_> = cmd
        .get_envs()
        .map(|(name, value)| (name.to_string_lossy().into_owned(), value.is_some()))
        .collect();
    // `None` means "removed from the child's environment".
    assert!(envs.contains(&("CUNO_PROBE".into(), false)), "{envs:?}");
    assert!(envs.contains(&("CLICOLOR_FORCE".into(), false)), "{envs:?}");
    assert!(envs.contains(&("cuno_lower".into(), false)), "{envs:?}");
    assert!(envs.contains(&("KEEP_ME".into(), true)), "{envs:?}");
    assert!(!envs.iter().any(|(name, _)| name == "PATH"), "{envs:?}");
}

/// Mutation: drop stdout or stderr from the message in `Run::assert`; the
/// expected text is then missing from the panic and this test fails.
#[test]
#[should_panic(
    expected = "--- stdout\nexpected:\nout-expected\nactual:\nout-actual\n\
                           --- stderr\nexpected:\nerr-expected\nactual:\nerr-actual"
)]
fn assert_reports_all_three_values() {
    let run = Run {
        code: Some(1),
        stdout: "out-actual".into(),
        stderr: "err-actual".into(),
    };
    run.assert(0, "out-expected", "err-expected");
}

/// Mutation: make `Run::assert` ignore the exit code; this test fails.
#[test]
#[should_panic(expected = "--- exit code\nexpected: 0\nactual:   Some(1)")]
fn assert_checks_the_exit_code() {
    let run = Run {
        code: Some(1),
        stdout: String::new(),
        stderr: String::new(),
    };
    run.assert(0, "", "");
}

/// The variables that `Project::run` must keep away from the child, set to
/// `1` by `run_does_not_pass_on_parent_variables` on the process running this
/// test. `CUNO_HOME` is among them: the child must see the project's own home
/// instead, whatever the parent had.
const PROBES: [&str; 3] = ["CUNO_PROBE", "CLICOLOR_FORCE", "CUNO_HOME"];

/// Run only by `run_does_not_pass_on_parent_variables`, which starts this
/// test binary again with `PROBES` set, so that the parent environment is
/// controlled without changing the test process's own (which needs `unsafe`).
#[test]
#[ignore = "run with PROBES set by run_does_not_pass_on_parent_variables"]
fn child_of_run_sees_no_probe_variables() {
    for name in PROBES {
        assert!(
            std::env::var_os(name).is_some(),
            "{name} should be set by the caller"
        );
    }
    let project = Project::new();
    let (_, env) = observe(&project);
    for name in ["CUNO_PROBE", "CLICOLOR_FORCE"] {
        assert_eq!(var(&env, name), None, "{name} reached the child");
    }
    // CUNO_HOME is present, but as the project's own home, not the parent's.
    assert_eq!(
        var(&env, "CUNO_HOME").map(PathBuf::from),
        Some(project.home().to_path_buf())
    );
}

/// Mutation: remove the `hermetic(...)` call from `Project::run_command`, or
/// pass it an empty iterator; the probe variables then reach the child and
/// this test fails. Mutation: set `CUNO_HOME` before the `hermetic` call in
/// `Project::run_command`; `hermetic` then removes it and this test fails.
#[test]
fn run_does_not_pass_on_parent_variables() {
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args([
        "--exact",
        "child_of_run_sees_no_probe_variables",
        "--ignored",
    ]);
    for name in PROBES {
        cmd.env(name, "1");
    }
    let output = cmd.output().expect("the test binary should start again");
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
}

/// Mutation: remove `cmd.env("CUNO_HOME", &self.home)` from
/// `Project::run_command`; the child then sees no `CUNO_HOME` and this test
/// fails. (Setting it before `hermetic` only shows when the parent has a
/// `CUNO_HOME`, which `run_does_not_pass_on_parent_variables` covers.)
#[test]
fn programs_see_the_project_s_own_cuno_home() {
    let project = Project::new();
    let (_, env) = observe(&project);
    assert_eq!(
        var(&env, "CUNO_HOME").map(PathBuf::from),
        Some(project.home().to_path_buf())
    );
    assert!(project.home().is_dir());
    assert!(!project.home().starts_with(project.path()));
}

/// Mutation: keep the temporary directory (`.disable_cleanup(true)` on the
/// `tempfile::Builder` in `Project::new`); the directory holding the project
/// and its home then outlives the `Project` and this test fails.
#[test]
fn dropping_a_project_removes_its_directories() {
    let project = Project::new().file("a.txt", "a");
    let root = project.path().parent().unwrap().to_path_buf();
    assert!(project.home().starts_with(&root));
    drop(project);
    assert!(!root.exists(), "{} was left behind", root.display());
}

// Tests of the missing-compiler policy and the MSVC environment capture in
// `tests/support/compilers.rs`. The compilers themselves are tested in
// `tests/toolchains.rs`.
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

/// Mutation: discard vcvars64.bat's output again (`>nul` before `&&` in
/// `vcvars_environment`); the reason disappears and this test fails.
#[cfg(windows)]
#[test]
fn failing_vcvars_is_reported_with_its_output() {
    let install = Project::new().file(
        "VC/Auxiliary/Build/vcvars64.bat",
        "@echo [ERROR:vcvars.bat] fake failure\r\n@exit /b 1\r\n",
    );
    let missing = support::compilers::vcvars_environment(install.path())
        .expect_err("a failing vcvars64.bat should be reported");
    assert!(
        missing.detail.contains("[ERROR:vcvars.bat] fake failure"),
        "{}",
        missing.detail
    );
}
