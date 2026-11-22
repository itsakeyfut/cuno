//! Running a build graph: its steps in parallel, each after those it waits
//! for, stopping at the first failure (`docs/en/specs/commands.md`,
//! "`cuno build` / `cuno check`").

use std::io;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;

use cuno_core::Env;

use crate::graph::{Graph, Step};

/// What a step's process did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    /// Whether it exited with success.
    pub success: bool,
    /// Its standard output, as written.
    pub stdout: Vec<u8>,
    /// Its standard error, as written.
    pub stderr: Vec<u8>,
}

/// Runs a step's process. A trait so the executor can be tested without
/// compilers.
pub trait Spawner: Sync {
    /// Runs `step` with exactly `env`, in `cwd`, and waits for it.
    fn run(&self, step: &Step, env: &Env, cwd: &Path) -> io::Result<Output>;
}

/// Runs steps as real processes, with no input and both outputs captured.
pub struct SystemSpawner;

impl Spawner for SystemSpawner {
    fn run(&self, step: &Step, env: &Env, cwd: &Path) -> io::Result<Output> {
        let output = Command::new(&step.program)
            .args(&step.args)
            .env_clear()
            .envs(env.vars())
            .current_dir(cwd)
            .stdin(Stdio::null())
            .output()?;
        Ok(Output {
            success: output.status.success(),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }
}

/// What happens while a graph runs, in the order it happens.
#[derive(Debug)]
pub enum Event<'a> {
    /// A step is about to start.
    Started(&'a Step),
    /// A step finished, with what it printed.
    Finished(&'a Step, &'a Output),
}

/// The steps that failed, by index, in the order they finished.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failed(pub Vec<usize>);

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Waiting,
    Running,
    Done,
    Failed,
}

/// Runs `graph` with `env` in `cwd`, at most `jobs` steps at a time, each
/// once every step it waits for has succeeded. After a failure no step
/// starts; the steps running are waited for. `on` hears every start and
/// finish, on this thread, so a step's output is shown whole when it ends.
pub fn execute(
    graph: &Graph,
    env: &Env,
    cwd: &Path,
    jobs: usize,
    spawner: &dyn Spawner,
    on: &mut dyn FnMut(Event<'_>),
) -> Result<(), Failed> {
    let jobs = jobs.max(1);
    let mut state = vec![State::Waiting; graph.steps.len()];
    let mut failed = Vec::new();
    let mut running = 0;
    let (done, finished) = mpsc::channel::<(usize, Output)>();
    std::thread::scope(|scope| {
        loop {
            if failed.is_empty() {
                for (i, step) in graph.steps.iter().enumerate() {
                    if running >= jobs {
                        break;
                    }
                    let ready = state[i] == State::Waiting
                        && step.after.iter().all(|&a| state[a] == State::Done);
                    if ready {
                        state[i] = State::Running;
                        running += 1;
                        on(Event::Started(step));
                        let done = done.clone();
                        scope.spawn(move || {
                            let output = run(step, env, cwd, spawner);
                            // The receiver outlives every sender in this scope.
                            let _ = done.send((i, output));
                        });
                    }
                }
            }
            if running == 0 {
                break;
            }
            let (i, output) = finished.recv().expect("a running step always reports back");
            running -= 1;
            let output = without_echo(&graph.steps[i], output);
            on(Event::Finished(&graph.steps[i], &output));
            if output.success {
                state[i] = State::Done;
            } else {
                state[i] = State::Failed;
                failed.push(i);
            }
        }
    });
    if failed.is_empty() {
        Ok(())
    } else {
        Err(Failed(failed))
    }
}

/// Runs one step, its output directory made first. A step that cannot
/// start fails with why on its standard error.
fn run(step: &Step, env: &Env, cwd: &Path, spawner: &dyn Spawner) -> Output {
    let result = match step.output.parent() {
        Some(dir) => std::fs::create_dir_all(dir),
        None => Ok(()),
    }
    .and_then(|()| spawner.run(step, env, cwd));
    result.unwrap_or_else(|e| Output {
        success: false,
        stdout: Vec::new(),
        stderr: format!("could not run {}: {e}\n", step.program.display()).into_bytes(),
    })
}

/// `output` without cl.exe's first line, the source's file name, when it is
/// exactly that line.
fn without_echo(step: &Step, mut output: Output) -> Output {
    let Some(name) = &step.echo else {
        return output;
    };
    let end = output
        .stdout
        .iter()
        .position(|&b| b == b'\n')
        .map_or(output.stdout.len(), |i| i + 1);
    let first = String::from_utf8_lossy(&output.stdout[..end]);
    if first.trim_end_matches(['\r', '\n']) == name {
        output.stdout.drain(..end);
    }
    output
}

#[cfg(test)]
mod tests;
