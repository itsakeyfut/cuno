//! Locating the C/C++ compilers that integration tests build with.
//!
//! This deliberately shares no code with cuno's own compiler detection: the
//! tests that use it are what that detection is checked against.
//!
//! # Where each compiler is looked for
//!
//! - **MSVC** (Windows only): the Visual Studio installation in
//!   `CUNO_TEST_VS_INSTALL` if set, otherwise the latest one with the C++ x64
//!   tools, as reported by `vswhere.exe`. Its environment is taken from
//!   `VC\Auxiliary\Build\vcvars64.bat`, once per test binary.
//! - **clang / clang-cl**: the directory in `CUNO_TEST_LLVM_BIN` if set,
//!   otherwise `PATH`. They run without the MSVC environment: on Windows clang
//!   finds the MSVC libraries and the Windows SDK itself. clang-cl is Windows
//!   only.
//!
//! # When a compiler is missing
//!
//! A test that needs a missing compiler fails, naming the compiler and how to
//! install it or point the tests at it. Outside CI, setting
//! `CUNO_TEST_ALLOW_MISSING=1` turns that into a skip, reported on stderr in a
//! normal `cargo test` run. On CI (`CI=true`) a missing compiler always fails,
//! whatever else is set. See [`require`] and [`on_missing`].
//!
//! The `CUNO_TEST_*` variables are read by the test process only; like every
//! `CUNO_*` variable, they are removed from the programs a test runs.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// A compiler the tests can build with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compiler {
    /// Microsoft's `cl.exe`, with the environment from `vcvars64.bat`.
    Msvc,
    /// The GCC-style `clang` / `clang++` drivers.
    Clang,
    /// The MSVC-style `clang-cl` driver.
    ClangCl,
}

/// The source language of a program to compile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    /// C.
    C,
    /// C++.
    Cxx,
}

/// Why a compiler could not be used.
#[derive(Debug, Clone)]
pub struct Missing {
    /// The compiler's name, as in [`Compiler::name`].
    pub compiler: &'static str,
    /// What was looked for and where.
    pub detail: String,
}

/// What a test does about a [`Missing`] compiler.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Fail the test with this message.
    Fail(String),
    /// Skip the test, reporting this message.
    Skip(String),
}

impl Compiler {
    /// The name used in messages.
    pub fn name(self) -> &'static str {
        match self {
            Compiler::Msvc => "MSVC (cl.exe)",
            Compiler::Clang => "clang",
            Compiler::ClangCl => "clang-cl",
        }
    }

    /// A command that compiles `source` (in the current directory) to the
    /// executable `exe`, with the environment the compiler needs, or why the
    /// compiler is unavailable.
    pub fn compile(self, language: Language, source: &str, exe: &str) -> Result<Command, Missing> {
        let mut cmd = match self {
            Compiler::Msvc => msvc_command()?,
            Compiler::Clang => Command::new(llvm_tool(
                self,
                match language {
                    Language::C => "clang",
                    Language::Cxx => "clang++",
                },
            )?),
            Compiler::ClangCl => Command::new(llvm_tool(self, "clang-cl")?),
        };
        match self {
            Compiler::Msvc | Compiler::ClangCl => {
                cmd.arg("/nologo");
                if language == Language::Cxx {
                    // Standard C++ exception handling; without it cl warns
                    // (C4530) on any use of <iostream>.
                    cmd.arg("/EHsc");
                }
                cmd.arg(source).arg(format!("/Fe:{exe}"));
            }
            Compiler::Clang => {
                cmd.arg(source).arg("-o").arg(exe);
            }
        }
        Ok(cmd)
    }
}

/// Returns a compile command for `compiler`, applying the missing-compiler
/// policy (see the module documentation) to the real environment: panics if
/// the compiler is missing and the test must fail, returns `None` (after
/// reporting it) if the test is to be skipped.
#[track_caller]
pub fn require(compiler: Compiler, language: Language, source: &str, exe: &str) -> Option<Command> {
    let missing = match compiler.compile(language, source, exe) {
        Ok(cmd) => return Some(cmd),
        Err(missing) => missing,
    };
    let ci = std::env::var_os("CI").is_some_and(|v| v == "true");
    let allow = std::env::var_os("CUNO_TEST_ALLOW_MISSING").is_some_and(|v| v == "1");
    match on_missing(&missing, ci, allow) {
        Outcome::Fail(message) => panic!("{message}"),
        Outcome::Skip(message) => {
            // Written to stderr directly rather than with `eprintln!`, which
            // the test harness captures and only shows for failing tests.
            let _ = writeln!(std::io::stderr(), "{message}");
            None
        }
    }
}

/// The missing-compiler policy: on CI always fail; elsewhere fail unless
/// skipping was allowed. The flags are parameters so that the policy can be
/// tested without changing the test process's environment.
pub fn on_missing(missing: &Missing, ci: bool, allow_missing: bool) -> Outcome {
    if allow_missing && !ci {
        return Outcome::Skip(format!(
            "skipped: {} not found: {}",
            missing.compiler, missing.detail
        ));
    }
    Outcome::Fail(format!(
        "{} not found: {}\n\
         Install it, or point the tests at an installation with \
         CUNO_TEST_VS_INSTALL (Visual Studio) or CUNO_TEST_LLVM_BIN (LLVM).\n\
         Outside CI, CUNO_TEST_ALLOW_MISSING=1 skips tests that need it.",
        missing.compiler, missing.detail
    ))
}

/// `name` (`clang`, `clang++`, `clang-cl`) from `CUNO_TEST_LLVM_BIN` or `PATH`.
fn llvm_tool(compiler: Compiler, name: &str) -> Result<PathBuf, Missing> {
    if compiler == Compiler::ClangCl && !cfg!(windows) {
        return Err(Missing {
            compiler: compiler.name(),
            detail: "clang-cl is only used on Windows".into(),
        });
    }
    let file = executable(name);
    if let Some(dir) = std::env::var_os("CUNO_TEST_LLVM_BIN") {
        let path = Path::new(&dir).join(&file);
        return if path.is_file() {
            Ok(path)
        } else {
            Err(Missing {
                compiler: compiler.name(),
                detail: format!(
                    "{} does not exist (from CUNO_TEST_LLVM_BIN)",
                    path.display()
                ),
            })
        };
    }
    find_in(std::env::var_os("PATH").unwrap_or_default(), &file).ok_or_else(|| Missing {
        compiler: compiler.name(),
        detail: format!("{file} is not on PATH"),
    })
}

/// `name` with the platform's executable suffix.
fn executable(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

/// The first directory in the `PATH`-style list `dirs` that holds `file`.
fn find_in(dirs: impl AsRef<std::ffi::OsStr>, file: &str) -> Option<PathBuf> {
    std::env::split_paths(&dirs)
        .map(|dir| dir.join(file))
        .find(|path| path.is_file())
}

/// `cl.exe` with the environment of a developer prompt.
fn msvc_command() -> Result<Command, Missing> {
    let env = msvc_environment().clone()?;
    let get = |name: &str| {
        env.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    };
    let path = get("PATH").unwrap_or_default();
    let cl = find_in(path, "cl.exe").ok_or_else(|| Missing {
        compiler: Compiler::Msvc.name(),
        detail: "cl.exe is not on the PATH set by vcvars64.bat".into(),
    })?;
    let mut cmd = Command::new(cl);
    for name in ["INCLUDE", "LIB", "LIBPATH", "PATH"] {
        if let Some(value) = get(name) {
            cmd.env(name, value);
        }
    }
    Ok(cmd)
}

/// The environment `vcvars64.bat` sets up, resolved once per test binary
/// because it takes seconds.
fn msvc_environment() -> &'static Result<HashMap<String, String>, Missing> {
    static ENV: OnceLock<Result<HashMap<String, String>, Missing>> = OnceLock::new();
    ENV.get_or_init(resolve_msvc_environment)
}

#[cfg(not(windows))]
fn resolve_msvc_environment() -> Result<HashMap<String, String>, Missing> {
    Err(Missing {
        compiler: Compiler::Msvc.name(),
        detail: "MSVC is only available on Windows".into(),
    })
}

#[cfg(windows)]
fn resolve_msvc_environment() -> Result<HashMap<String, String>, Missing> {
    let install = match std::env::var_os("CUNO_TEST_VS_INSTALL") {
        Some(dir) => PathBuf::from(dir),
        None => vswhere_installation().map_err(|detail| Missing {
            compiler: Compiler::Msvc.name(),
            detail,
        })?,
    };
    vcvars_environment(&install)
}

/// The environment `vcvars64.bat` of the Visual Studio installation at
/// `install` sets up, or why it could not be obtained (including what
/// `vcvars64.bat` printed when it failed).
#[cfg(windows)]
pub fn vcvars_environment(install: &Path) -> Result<HashMap<String, String>, Missing> {
    use std::os::windows::process::CommandExt;

    let missing = |detail: String| Missing {
        compiler: Compiler::Msvc.name(),
        detail,
    };
    let vcvars = install.join(r"VC\Auxiliary\Build\vcvars64.bat");
    if !vcvars.is_file() {
        return Err(missing(format!("{} does not exist", vcvars.display())));
    }
    // `cmd /s /c "..."` strips only the outer quotes, so the quoted path
    // inside survives; Rust's own argument quoting does not suit cmd, hence
    // `raw_arg`.
    let output = Command::new("cmd")
        .raw_arg(format!(
            "/d /s /c \"call \"{}\" && echo {ENV_MARKER} && set\"",
            vcvars.display()
        ))
        .output()
        .map_err(|e| missing(format!("cmd could not run vcvars64.bat: {e}")))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    // vcvars64.bat reports its errors on stdout, so its output is kept rather
    // than discarded, and the environment is only what `set` printed after
    // the marker.
    let Some((_, env)) = stdout.split_once(ENV_MARKER) else {
        return Err(missing(format!(
            "{} failed:\n{}\n{}",
            vcvars.display(),
            stdout.trim_end(),
            String::from_utf8_lossy(&output.stderr).trim_end()
        )));
    };
    Ok(env
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect())
}

/// Printed between vcvars64.bat's own output and `set`'s; only reached when
/// vcvars64.bat succeeded.
#[cfg(windows)]
const ENV_MARKER: &str = "--- cuno-test: environment follows ---";

/// The installation path of the latest Visual Studio with the C++ x64 tools.
#[cfg(windows)]
fn vswhere_installation() -> Result<PathBuf, String> {
    let base = std::env::var_os("ProgramFiles(x86)").ok_or("ProgramFiles(x86) is not set")?;
    let vswhere = Path::new(&base).join(r"Microsoft Visual Studio\Installer\vswhere.exe");
    let output = Command::new(&vswhere)
        .args(["-latest", "-products", "*", "-requires"])
        .arg("Microsoft.VisualStudio.Component.VC.Tools.x86.x64")
        .args(["-property", "installationPath"])
        .output()
        .map_err(|e| format!("{} could not run: {e}", vswhere.display()))?;
    let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if path.is_empty() {
        return Err("vswhere found no Visual Studio with the C++ x64 tools".into());
    }
    Ok(PathBuf::from(path))
}
