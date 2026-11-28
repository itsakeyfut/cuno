//! Shared support for cuno's integration tests.
//!
//! Include it from a test file with `mod support;`. A typical test creates a
//! [`Project`], writes the files it needs, runs `cuno` in it and asserts the
//! exact result:
//!
//! ```ignore
//! let project = Project::new().file("Cuno.toml", "[package]\nname = \"demo\"\n");
//! project.cuno(&["--version"]).assert(0, "cuno 0.1.0\n", "");
//! ```
//!
//! Every program runs with a hermetic environment (see [`hermetic`]) and a
//! cuno home of its own (see [`Project::home`]), so neither the developer's or
//! the CI runner's environment nor their real cuno home can change a result.

// Each test file compiles its own copy of this module and uses only part of
// it; without this, `clippy -D warnings` fails on the helpers a given file
// does not call.
#![allow(dead_code)]

pub mod compilers;

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

/// A fresh directory for one test, under the system temp directory, removed
/// when the `Project` is dropped (also when the test panics). It holds the
/// project itself and, beside it, the cuno home the test's programs use.
pub struct Project {
    root: tempfile::TempDir,
    project: PathBuf,
    home: PathBuf,
}

impl Project {
    /// An empty project, with an empty cuno home, in a new, uniquely named
    /// directory.
    pub fn new() -> Project {
        let root = tempfile::Builder::new()
            .prefix("cuno-test-")
            .tempdir()
            .expect("a temporary directory should be created");
        let project = root.path().join("project");
        let home = root.path().join("home");
        for dir in [&project, &home] {
            std::fs::create_dir(dir).expect("the test's directories should be created");
        }
        Project {
            root,
            project,
            home,
        }
    }

    /// Writes `contents` to `path` inside the project, creating any parent
    /// directories. `path` is relative and `/`-separated. Returns `self` so
    /// that files can be chained.
    ///
    /// The file is dated an hour back, as a project's files are when a
    /// build starts: a build does not record a step whose input changed
    /// within two seconds of its start (`docs/en/specs/build.md`, "Deciding
    /// what changed"), so a file written just now would leave a test's
    /// first build unrecorded and its second running everything again.
    pub fn file(self, path: &str, contents: &str) -> Project {
        let full = self.project.join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).expect("parent directories should be created");
        }
        std::fs::write(&full, contents).expect("the file should be written");
        let an_hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        std::fs::File::options()
            .write(true)
            .open(&full)
            .and_then(|file| file.set_modified(an_hour_ago))
            .expect("the file's time should be set");
        self
    }

    /// The project's directory.
    pub fn path(&self) -> &Path {
        &self.project
    }

    /// The cuno home every program run from this project sees as `CUNO_HOME`.
    /// It sits beside the project directory rather than inside it, so nothing
    /// that walks or scans the project can come across it, and it is empty
    /// unless a test writes into it. Without it, cuno would fall back to the
    /// developer's real `%USERPROFILE%\.cuno`.
    pub fn home(&self) -> &Path {
        &self.home
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
    /// environment of their own (see [`compilers`]). `CUNO_HOME` is always
    /// [`Project::home`].
    pub fn run_command(&self, cmd: Command) -> Run {
        self.run_command_with(cmd, &[])
    }

    /// [`Project::run_command`] with `vars` set after the environment is
    /// made hermetic, so that a `CUNO_*` variable a test sets survives.
    pub fn run_command_with(&self, mut cmd: Command, vars: &[(&str, &str)]) -> Run {
        // The project, unless the test chose a directory in it.
        if cmd.get_current_dir().is_none() {
            cmd.current_dir(self.path());
        }
        hermetic(&mut cmd, std::env::vars_os().map(|(name, _)| name));
        // After `hermetic`, which removes every inherited `CUNO_*` variable.
        cmd.env("CUNO_HOME", &self.home);
        cmd.envs(vars.iter().copied());
        let output = cmd.output().expect("the program should start");
        // Decoded leniently: cuno passes a compiler's output through as the
        // compiler wrote it, which on a Japanese Visual Studio is CP932, not
        // UTF-8. cuno's own lines are ASCII and compare exactly either way.
        Run {
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
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

/// One test per compiler, in a module named after the test the loop was,
/// so the compilers run in parallel rather than one after another:
///
/// ```ignore
/// support::per_compiler! {
///     /// (the test's doc comment)
///     the_test = check,
///     [msvc = Msvc, clang = Clang, clang_cl = ClangCl]
/// }
/// ```
///
/// writes `the_test::msvc`, `the_test::clang` and `the_test::clang_cl`, each
/// calling `check(name, compiler)` with the compiler's entry in the file's
/// `COMPILERS`. `check` is any expression callable that way, and sees the
/// file's items. Each test first asserts that its function name is that
/// entry with `-` read as `_`, so a test cannot run another compiler than
/// the one it is named for; that would pass, the other compiler building
/// just as well. The module also gets `every_compiler_is_split`, which fails
/// when the file's `COMPILERS` and the list given here differ.
///
/// The doc comment goes inside the invocation: above it, rustc warns that it
/// documents nothing.
// Not every test binary that includes `support` splits a test.
#[allow(unused_macros)]
macro_rules! per_compiler {
    ($(#[$doc:meta])* $test:ident = $check:expr,
     [$($fn_name:ident = $compiler:ident),+ $(,)?]) => {
        $(#[$doc])*
        mod $test {
            #[allow(unused_imports)]
            use super::*;

            /// The compilers this test is split over, in order.
            const SPLIT: &[$crate::support::compilers::Compiler] =
                &[$($crate::support::compilers::Compiler::$compiler),+];

            /// The compiler's name in the file's `COMPILERS`.
            fn name(compiler: $crate::support::compilers::Compiler) -> &'static str {
                COMPILERS
                    .iter()
                    .find(|(_, c)| *c == compiler)
                    .map(|(name, _)| *name)
                    .unwrap_or_else(|| panic!("{compiler:?} is not in COMPILERS"))
            }

            $(
                #[test]
                fn $fn_name() {
                    let compiler = $crate::support::compilers::Compiler::$compiler;
                    let name = name(compiler);
                    assert_eq!(
                        stringify!($fn_name),
                        name.replace('-', "_"),
                        "the test is named for another compiler than it runs"
                    );
                    ($check)(name, compiler);
                }
            )+

            /// Every compiler in `COMPILERS` has a test here, and no other.
            #[test]
            fn every_compiler_is_split() {
                let listed: Vec<_> = COMPILERS.iter().map(|(_, c)| *c).collect();
                assert_eq!(SPLIT, listed.as_slice(), "COMPILERS and the split differ");
            }
        }
    };
}
#[allow(unused_imports)]
pub(crate) use per_compiler;
