//! Running the programs detection asks (vswhere, compilers), behind a trait so
//! that tests can answer for them.

use std::ffi::OsStr;
#[cfg(test)]
use std::ffi::OsString;
use std::io::{self, Read};
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SystemRunner {
    /// How long a tool may run; [`TOOL_TIMEOUT`] outside this module's tests.
    limit: Duration,
}

impl SystemRunner {
    /// A runner with the limit the spec gives.
    pub const fn new() -> SystemRunner {
        SystemRunner {
            limit: TOOL_TIMEOUT,
        }
    }
}

impl Default for SystemRunner {
    fn default() -> SystemRunner {
        SystemRunner::new()
    }
}

impl Runner for SystemRunner {
    /// Runs `program` with stdin closed, stopping it once the limit has
    /// passed.
    fn run(&self, program: &Path, args: &[&OsStr], env: &Env) -> io::Result<Output> {
        let limit = self.limit;
        let mut child = Command::new(program)
            .args(args)
            .env_clear()
            .envs(env.vars())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let timed_out = || {
            io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "did not finish within {} seconds and was stopped",
                    limit.as_secs_f64()
                ),
            )
        };
        // Each pipe is drained on its own thread, so a child that fills one
        // cannot stall waiting for cuno to read it. The bytes come back over a
        // channel rather than by joining, so that waiting for them is bounded
        // by the same deadline: a process the tool started can keep a pipe open
        // after the tool itself has exited.
        let drain = |pipe: Option<Box<dyn Read + Send>>| {
            let (send, receive) = mpsc::channel();
            std::thread::spawn(move || {
                let mut bytes = Vec::new();
                if let Some(mut pipe) = pipe {
                    let _ = pipe.read_to_end(&mut bytes);
                }
                let _ = send.send(bytes);
            });
            receive
        };
        let stdout = drain(child.stdout.take().map(|p| Box::new(p) as _));
        let stderr = drain(child.stderr.take().map(|p| Box::new(p) as _));
        let deadline = Instant::now() + limit;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {}
                // Not left running when its state cannot be read.
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(e);
                }
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(timed_out());
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let collect = |pipe: mpsc::Receiver<Vec<u8>>| {
            pipe.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .map_err(|_| timed_out())
        };
        Ok(Output {
            success: status.success(),
            stdout: collect(stdout)?,
            stderr: collect(stderr)?,
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
        let out = SystemRunner::new()
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

    /// The helpers below run as child processes of the tests: the test binary
    /// runs itself with `--ignored --exact <helper>`, and each helper does its
    /// work only when its variable is set, so `cargo test -- --ignored` stays
    /// harmless.
    fn helper(name: &str) -> (PathBuf, Vec<OsString>) {
        let args = ["--ignored", "--exact", name, "--nocapture"]
            .map(OsString::from)
            .to_vec();
        (std::env::current_exe().unwrap(), args)
    }

    fn run_helper(runner: SystemRunner, name: &str, env: &Env) -> io::Result<Output> {
        let (program, args) = helper(name);
        let args: Vec<&OsStr> = args.iter().map(|a| a.as_os_str()).collect();
        runner.run(&program, &args, env)
    }

    fn limited(limit: Duration) -> SystemRunner {
        SystemRunner { limit }
    }

    /// Sleeps, then writes the file `CUNO_SLEEP_MARKER` names.
    #[test]
    #[ignore]
    fn sleeps_then_marks() {
        if let Some(marker) = std::env::var_os("CUNO_SLEEP_MARKER") {
            std::thread::sleep(Duration::from_secs(3));
            std::fs::write(marker, "finished").unwrap();
        }
    }

    /// Starts [`sleeps_then_marks`] with this process's stdout, then exits at
    /// once, leaving the pipe open behind it.
    #[test]
    #[ignore]
    fn exits_leaving_a_sleeper() {
        if std::env::var_os("CUNO_LEAVE_SLEEPER").is_some() {
            let (program, args) = helper("run::tests::sleeps_then_marks");
            // Deliberately not waited for: leaving it running is the point.
            #[allow(clippy::zombie_processes)]
            Command::new(program)
                .args(args)
                .stdout(Stdio::inherit())
                .spawn()
                .unwrap();
        }
    }

    /// Writes more than a pipe holds to both streams, then fails.
    #[test]
    #[ignore]
    fn floods_both_streams_then_fails() {
        if std::env::var_os("CUNO_FLOOD").is_some() {
            use std::io::Write;
            let line = "0123456789abcdef".repeat(64);
            for _ in 0..200 {
                println!("out {line}");
                let _ = writeln!(io::stderr(), "err {line}");
            }
            std::process::exit(3);
        }
    }

    /// Reads stdin to its end and says so.
    #[test]
    #[ignore]
    fn reads_stdin_to_the_end() {
        if std::env::var_os("CUNO_READ_STDIN").is_some() {
            let mut input = Vec::new();
            io::stdin().read_to_end(&mut input).unwrap();
            println!("stdin ended");
        }
    }

    /// Runs [`reads_stdin_to_the_end`] through a runner, as a child whose
    /// own stdin the test keeps open, and reports what came back.
    #[test]
    #[ignore]
    fn runs_a_reader_of_stdin() {
        if std::env::var_os("CUNO_RUN_READER").is_some() {
            let env = Env::from_process()
                .without(&["CUNO_RUN_READER"])
                .with("CUNO_READ_STDIN", "1".into());
            match run_helper(
                limited(Duration::from_secs(5)),
                "run::tests::reads_stdin_to_the_end",
                &env,
            ) {
                Ok(out) => println!(
                    "reader: {}",
                    String::from_utf8_lossy(&out.stdout).contains("stdin ended")
                ),
                Err(e) => println!("reader: {e}"),
            }
        }
    }

    /// Mutation: change the limit the spec gives, or have `SystemRunner`
    /// default to another; this test fails.
    #[test]
    fn the_system_runner_stops_tools_after_thirty_seconds() {
        assert_eq!(TOOL_TIMEOUT, Duration::from_secs(30));
        assert_eq!(SystemRunner::new().limit, TOOL_TIMEOUT);
        assert_eq!(SystemRunner::default(), SystemRunner::new());
    }

    /// Mutation: wait without a deadline, run with another limit than the
    /// runner's, or return at the deadline without stopping the tool; this
    /// test fails (it returns late, or the tool finishes and writes its
    /// marker).
    #[test]
    fn a_tool_past_the_limit_is_stopped() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("marker");
        let env = Env::from_process().with("CUNO_SLEEP_MARKER", marker.clone().into());
        let started = Instant::now();
        let err = run_helper(
            limited(Duration::from_millis(300)),
            "run::tests::sleeps_then_marks",
            &env,
        )
        .unwrap_err();
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

    /// Mutation: wait for the output without the deadline (join the readers);
    /// this test fails (it returns only when the left-behind process exits).
    #[test]
    fn output_held_open_after_exit_is_waited_for_only_until_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let env = Env::from_process()
            .with("CUNO_SLEEP_MARKER", dir.path().join("marker").into())
            .with("CUNO_LEAVE_SLEEPER", "1".into());
        let started = Instant::now();
        let err = run_helper(
            limited(Duration::from_millis(1500)),
            "run::tests::exits_leaving_a_sleeper",
            &env,
        )
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut, "{err}");
        assert!(
            started.elapsed() < Duration::from_millis(2500),
            "{:?}",
            started.elapsed()
        );
    }

    /// Mutation: report every run as a success, drop or swap stderr, or read
    /// the pipes only after the tool exits (it then blocks on a full pipe
    /// until the limit); this test fails.
    #[test]
    fn a_failing_tool_returns_both_streams_in_full() {
        let env = Env::from_process().with("CUNO_FLOOD", "1".into());
        let out = run_helper(
            limited(Duration::from_secs(20)),
            "run::tests::floods_both_streams_then_fails",
            &env,
        )
        .unwrap();
        assert!(!out.success);
        let count = |bytes: &[u8], prefix: &str| {
            String::from_utf8_lossy(bytes)
                .lines()
                .filter(|l| l.starts_with(prefix))
                .count()
        };
        assert_eq!(count(&out.stdout, "out "), 200);
        assert_eq!(count(&out.stderr, "err "), 200);
        assert_eq!(count(&out.stdout, "err "), 0);
    }

    /// Mutation: let the tool inherit cuno's stdin instead of closing it;
    /// this test fails (a tool reading its input then waits until the
    /// limit, here on a pipe the test keeps open).
    #[test]
    fn a_tool_sees_its_stdin_closed() {
        let (program, args) = helper("run::tests::runs_a_reader_of_stdin");
        let mut middle = Command::new(program)
            .args(args)
            .env("CUNO_RUN_READER", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let _open = middle.stdin.take();
        let mut text = String::new();
        middle
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        middle.wait().unwrap();
        assert!(text.contains("reader: true"), "{text}");
    }
}
