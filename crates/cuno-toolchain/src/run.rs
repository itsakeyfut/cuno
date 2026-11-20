//! Running the programs detection asks (vswhere, compilers), behind a trait so
//! that tests can answer for them.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

use cuno_core::Env;

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
    /// Runs `program` with `args` and exactly the environment `env` (nothing
    /// inherited from the process running cuno); `Err` when it could not be
    /// started.
    fn run(&self, program: &Path, args: &[&OsStr], env: &Env) -> std::io::Result<Output>;
}

/// The real [`Runner`], which starts processes. The binary passes it in.
pub struct SystemRunner;

impl Runner for SystemRunner {
    fn run(&self, program: &Path, args: &[&OsStr], env: &Env) -> std::io::Result<Output> {
        let output = Command::new(program)
            .args(args)
            .env_clear()
            .envs(env.vars())
            .output()?;
        Ok(Output {
            success: output.status.success(),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cuno_core::NameCase;

    /// Mutation: drop `env_clear`, or the `envs` call; this test fails (the
    /// child would see the test process's environment, or not the given one).
    #[cfg(windows)]
    #[test]
    fn the_child_sees_exactly_the_given_environment() {
        let comspec = std::env::var_os("ComSpec").expect("ComSpec is set on Windows");
        let root = std::env::var_os("SystemRoot").expect("SystemRoot is set on Windows");
        let env = Env::from_pairs(
            [
                ("SystemRoot".into(), root),
                ("CUNO_RUN_PROBE".into(), "given".into()),
            ],
            NameCase::Insensitive,
        );
        let out = SystemRunner
            .run(
                comspec.as_ref(),
                &["/d".as_ref(), "/c".as_ref(), "set".as_ref()],
                &env,
            )
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.lines().any(|l| l == "CUNO_RUN_PROBE=given"), "{text}");
        assert!(
            !text
                .lines()
                .any(|l| l.to_ascii_uppercase().starts_with("PATH=")),
            "{text}"
        );
    }
}
