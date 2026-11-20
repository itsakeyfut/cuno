//! Running the programs detection asks (vswhere, compilers), behind a trait so
//! that tests can answer for them.

use std::ffi::OsStr;
use std::io::{self, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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
    /// started, or did not finish in time (`ErrorKind::TimedOut`).
    fn run(&self, program: &Path, args: &[&OsStr], env: &Env) -> std::io::Result<Output>;
}

/// How long a tool may run before it is stopped (`docs/en/specs/toolchain.md`,
/// "Detection").
pub const TOOL_TIMEOUT: Duration = Duration::from_secs(30);

/// The real [`Runner`], which starts processes and stops any that runs
/// longer than [`TOOL_TIMEOUT`]. The binary passes it in.
pub struct SystemRunner;

impl Runner for SystemRunner {
    fn run(&self, program: &Path, args: &[&OsStr], env: &Env) -> io::Result<Output> {
        run_with_timeout(program, args, env, TOOL_TIMEOUT)
    }
}

/// Runs `program`, stopping it once `limit` has passed.
fn run_with_timeout(
    program: &Path,
    args: &[&OsStr],
    env: &Env,
    limit: Duration,
) -> io::Result<Output> {
    let mut child = Command::new(program)
        .args(args)
        .env_clear()
        .envs(env.vars())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    // Each pipe is drained on its own thread, so a child that fills one
    // cannot stall waiting for cuno to read it.
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut bytes);
            }
            bytes
        })
    };
    let stdout = drain(child.stdout.take().map(|p| Box::new(p) as _));
    let stderr = drain(child.stderr.take().map(|p| Box::new(p) as _));
    let deadline = Instant::now() + limit;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            // The readers are left behind rather than joined: a grandchild
            // still holding a pipe would keep them, and cuno, waiting.
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "did not finish within {} seconds and was stopped",
                    limit.as_secs_f64()
                ),
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    Ok(Output {
        success: status.success(),
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
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

    /// Run by [`a_tool_past_the_limit_is_stopped`] as a child process: sleeps,
    /// then writes the file `CUNO_SLEEP_MARKER` names. Does nothing when run
    /// any other way.
    #[test]
    #[ignore]
    fn sleeps_then_marks() {
        if let Some(marker) = std::env::var_os("CUNO_SLEEP_MARKER") {
            std::thread::sleep(Duration::from_secs(3));
            std::fs::write(marker, "finished").unwrap();
        }
    }

    fn sleeper(marker: &Path) -> (std::path::PathBuf, Vec<std::ffi::OsString>, Env) {
        let args = ["--ignored", "--exact", "run::tests::sleeps_then_marks"]
            .map(Into::into)
            .to_vec();
        let env = Env::from_process().with("CUNO_SLEEP_MARKER", marker.into());
        (std::env::current_exe().unwrap(), args, env)
    }

    /// Mutation: wait without a deadline, or return at the deadline without
    /// stopping the tool; this test fails (it returns late, or the tool
    /// finishes and writes its marker).
    #[test]
    fn a_tool_past_the_limit_is_stopped() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("marker");
        let (program, args, env) = sleeper(&marker);
        let args: Vec<&OsStr> = args.iter().map(|a| a.as_os_str()).collect();
        let started = Instant::now();
        let err = run_with_timeout(&program, &args, &env, Duration::from_millis(300)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut, "{err}");
        assert!(err.to_string().contains("0.3 seconds"), "{err}");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
        std::thread::sleep(Duration::from_secs(4));
        assert!(!marker.exists(), "the tool kept running after the limit");
    }

    /// Mutation: treat every run as timed out, or lose its output; this test
    /// fails.
    #[test]
    fn a_tool_within_the_limit_returns_its_output() {
        let dir = tempfile::tempdir().unwrap();
        let (program, _, env) = sleeper(&dir.path().join("unused"));
        let out = run_with_timeout(
            &program,
            &["--list".as_ref()],
            &env,
            Duration::from_secs(20),
        )
        .unwrap();
        assert!(out.success);
        assert!(
            String::from_utf8_lossy(&out.stdout).contains("run::tests::sleeps_then_marks"),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );
    }
}
