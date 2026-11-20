//! Running the programs detection asks (vswhere, compilers), behind a trait so
//! that tests can answer for them.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

/// What a finished program returned.
#[derive(Clone, Debug, Default)]
pub struct Output {
    /// Whether it exited with code 0.
    pub success: bool,
    /// Everything it wrote to stdout, as bytes: console programs on Windows
    /// write in the console code page, not UTF-8.
    pub stdout: Vec<u8>,
    /// Everything it wrote to stderr, as bytes.
    pub stderr: Vec<u8>,
}

/// Runs a program and waits for it.
pub trait Runner {
    /// Runs `program` with `args`; `Err` when it could not be started.
    fn run(&self, program: &Path, args: &[&OsStr]) -> std::io::Result<Output>;
}

/// The real [`Runner`], which starts processes. The binary passes it in.
pub struct SystemRunner;

impl Runner for SystemRunner {
    fn run(&self, program: &Path, args: &[&OsStr]) -> std::io::Result<Output> {
        let output = Command::new(program).args(args).output()?;
        Ok(Output {
            success: output.status.success(),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }
}
