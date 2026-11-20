//! Shared support for cuno's integration tests.
//!
//! Include it from a test file with `mod support;`. A typical test creates a
//! [`Project`], writes the files it needs, runs `cuno` in it and asserts the
//! exact result:
//!
//! ```ignore
//! let project = Project::new().file("cuno.toml", "[package]\nname = \"demo\"\n");
//! project.cuno(&["--version"]).assert(0, "cuno 0.0.0\n", "");
//! ```
//!
//! Every program runs with a hermetic environment (see [`hermetic`]), so the
//! developer's or the CI runner's environment cannot change a result.

// Each test file compiles its own copy of this module and uses only part of
// it; without this, `clippy -D warnings` fails on the helpers a given file
// does not call.
#![allow(dead_code)]

use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::Command;

/// A fresh directory for one test, under the system temp directory, removed
/// when the `Project` is dropped (also when the test panics).
pub struct Project {
    dir: tempfile::TempDir,
}

impl Project {
    /// An empty project in a new, uniquely named directory.
    pub fn new() -> Project {
        let dir = tempfile::Builder::new()
            .prefix("cuno-test-")
            .tempdir()
            .expect("a temporary project directory should be created");
        Project { dir }
    }

    /// Writes `contents` to `path` inside the project, creating any parent
    /// directories. `path` is relative and `/`-separated. Returns `self` so
    /// that files can be chained.
    pub fn file(self, path: &str, contents: &str) -> Project {
        let full = self.dir.path().join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).expect("parent directories should be created");
        }
        std::fs::write(&full, contents).expect("the file should be written");
        self
    }

    /// The project's directory.
    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Runs the built `cuno` binary with `args`, in the project directory.
    pub fn cuno(&self, args: &[&str]) -> Run {
        self.run(env!("CARGO_BIN_EXE_cuno"), args)
    }

    /// Runs `program` with `args` in the project directory, with the same
    /// hermetic environment as [`Project::cuno`]. Used to run programs other
    /// than cuno, such as what cuno builds.
    pub fn run(&self, program: impl AsRef<OsStr>, args: &[&str]) -> Run {
        let mut cmd = Command::new(program);
        cmd.args(args);
        self.run_command(cmd)
    }

    /// Runs a prepared `cmd` in the project directory, with the same hermetic
    /// environment as [`Project::run`]. Variables `cmd` sets itself are kept
    /// unless [`hermetic`] removes them; used for compilers, which need an
    /// environment of their own (see [`compilers`]).
    pub fn run_command(&self, mut cmd: Command) -> Run {
        cmd.current_dir(self.path());
        hermetic(&mut cmd, std::env::vars_os().map(|(name, _)| name));
        let output = cmd.output().expect("the program should start");
        Run {
            code: output.status.code(),
            stdout: String::from_utf8(output.stdout).expect("stdout should be UTF-8"),
            stderr: String::from_utf8(output.stderr).expect("stderr should be UTF-8"),
        }
    }
}

/// Removes from `cmd` every variable that could change cuno's output: the
/// colour variables clap reads (`CLICOLOR_FORCE`, `CLICOLOR`, `NO_COLOR`), and
/// every variable among `inherited` whose name starts with `CUNO_` (in any
/// case).
///
/// `inherited` is the names of the variables the child would inherit. It is a
/// parameter so that tests can pass synthetic names instead of modifying the
/// test process's own environment, which needs `unsafe` in edition 2024.
pub fn hermetic(cmd: &mut Command, inherited: impl Iterator<Item = OsString>) {
    for name in ["CLICOLOR_FORCE", "CLICOLOR", "NO_COLOR"] {
        cmd.env_remove(name);
    }
    for name in inherited {
        // Upper-cased because Windows variable names are case-insensitive.
        if name
            .to_string_lossy()
            .to_ascii_uppercase()
            .starts_with("CUNO_")
        {
            cmd.env_remove(name);
        }
    }
}

/// What a finished process returned and printed.
#[derive(Debug)]
pub struct Run {
    /// The exit code, or `None` if the process was terminated by a signal.
    pub code: Option<i32>,
    /// Everything written to stdout.
    pub stdout: String,
    /// Everything written to stderr.
    pub stderr: String,
}

impl Run {
    /// Asserts the exit code, stdout and stderr exactly. On failure the
    /// message shows all three, expected and actual, so a failing test
    /// explains itself.
    #[track_caller]
    pub fn assert(&self, code: i32, stdout: &str, stderr: &str) {
        if self.code == Some(code) && self.stdout == stdout && self.stderr == stderr {
            return;
        }
        panic!(
            "process result differs\n\
             --- exit code\nexpected: {code}\nactual:   {:?}\n\
             --- stdout\nexpected:\n{stdout}\nactual:\n{}\n\
             --- stderr\nexpected:\n{stderr}\nactual:\n{}",
            self.code, self.stdout, self.stderr,
        );
    }
}
