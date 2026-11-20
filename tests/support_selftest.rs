//! Tests of the shared test support in `tests/support`, so that a defect in
//! the helpers shows up here rather than as a confusing failure elsewhere.

mod support;

use std::ffi::OsString;
use std::process::Command;

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

/// Mutation: remove `.current_dir(self.path())` from `Project::run`; the child
/// then runs in the test's own directory and this test fails.
#[test]
fn programs_run_in_the_project_directory() {
    let project = Project::new();
    let run = if cfg!(windows) {
        project.run("cmd", &["/C", "cd"])
    } else {
        project.run("pwd", &[])
    };
    let expected = std::fs::canonicalize(project.path()).unwrap();
    let actual = std::fs::canonicalize(run.stdout.trim_end()).unwrap();
    assert_eq!(actual, expected, "{run:?}");
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

/// The variables that `Project::run` must keep away from the child, set by
/// `run_does_not_pass_on_parent_variables` on the process running this test.
const PROBES: [&str; 2] = ["CUNO_PROBE", "CLICOLOR_FORCE"];

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
    let run = if cfg!(windows) {
        project.run("cmd", &["/C", "set"])
    } else {
        project.run("env", &[])
    };
    for name in PROBES {
        let leaked = run
            .stdout
            .lines()
            .any(|line| line.starts_with(&format!("{name}=")));
        assert!(!leaked, "{name} reached the child:\n{}", run.stdout);
    }
}

/// Mutation: remove the `hermetic(...)` call from `Project::run`, or pass it
/// an empty iterator; the probe variables then reach the child and this test
/// fails.
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
