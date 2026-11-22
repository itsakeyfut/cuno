//! Running a build graph: its steps in parallel, each after those it waits
//! for, stopping at the first failure (`docs/en/specs/commands.md`,
//! "`cuno build` / `cuno check`").

use std::io;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::SystemTime;

use cuno_core::Env;

use crate::deps;
use crate::graph::{Graph, Step};
use crate::state::Records;

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
    /// A step is about to start. A step that is up to date never starts.
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

/// What a step's thread reports back.
enum Report {
    /// It is not up to date and is starting.
    Starting,
    /// It ran.
    Ran(Output),
    /// It was up to date and did not run.
    UpToDate,
}

/// Runs `graph` with `env` in `cwd`, at most `jobs` steps at a time, each
/// once every step it waits for has succeeded. With `records`, a step is
/// skipped when it is up to date (a link's inputs are the objects, so it
/// runs when one of them changed) (`docs/en/specs/build.md`, "Deciding what changed"), and recorded when
/// it succeeds. After a failure no step starts; the steps running are
/// waited for. `on` hears every start and finish, on this thread, so a
/// step's output is shown whole when it ends.
pub fn execute(
    graph: &Graph,
    env: &Env,
    cwd: &Path,
    jobs: usize,
    spawner: &dyn Spawner,
    records: Option<&Records>,
    on: &mut dyn FnMut(Event<'_>),
) -> Result<(), Failed> {
    let jobs = jobs.max(1);
    let mut state = vec![State::Waiting; graph.steps.len()];
    let mut failed = Vec::new();
    let mut running = 0;
    let (done, finished) = mpsc::channel::<(usize, Report)>();
    std::thread::scope(|scope| {
        loop {
            if failed.is_empty() {
                for (i, step) in graph.steps.iter().enumerate() {
                    if running >= jobs {
                        break;
                    }
                    let ready = state[i] == State::Waiting
                        && step.after.iter().all(|&a| state[a] == State::Done);
                    if !ready {
                        continue;
                    }
                    state[i] = State::Running;
                    running += 1;
                    let waited: Vec<&Path> = step
                        .after
                        .iter()
                        .map(|&a| graph.steps[a].output.as_path())
                        .collect();
                    let done = done.clone();
                    scope.spawn(move || {
                        // A panic is a failure that still reports back, so
                        // the wait below always ends.
                        let report = std::panic::catch_unwind(AssertUnwindSafe(|| {
                            if records.is_some_and(|r| r.up_to_date(step)) {
                                return Report::UpToDate;
                            }
                            let _ = done.send((i, Report::Starting));
                            Report::Ran(perform(step, &waited, env, cwd, spawner, records))
                        }))
                        .unwrap_or_else(|_| {
                            Report::Ran(Output {
                                success: false,
                                stdout: Vec::new(),
                                stderr: format!(
                                    "cuno failed while running {}\n",
                                    step.program.display()
                                )
                                .into_bytes(),
                            })
                        });
                        // The receiver outlives every sender in this scope.
                        let _ = done.send((i, report));
                    });
                }
            }
            if running == 0 {
                break;
            }
            let (i, report) = finished.recv().expect("a running step always reports back");
            let step = &graph.steps[i];
            match report {
                Report::Starting => on(Event::Started(step)),
                Report::UpToDate => {
                    running -= 1;
                    state[i] = State::Done;
                }
                Report::Ran(output) => {
                    running -= 1;
                    let output = without_echo(step, output);
                    on(Event::Finished(step, &output));
                    if output.success {
                        state[i] = State::Done;
                    } else {
                        state[i] = State::Failed;
                        failed.push(i);
                    }
                }
            }
        }
    });
    if failed.is_empty() {
        Ok(())
    } else {
        Err(Failed(failed))
    }
}

/// Runs `step` and, with `records`, records it when it succeeds, as
/// having read what it [`Step::reads`], the outputs of the steps it
/// `waited` for, and every header its compiler named.
fn perform(
    step: &Step,
    waited: &[&Path],
    env: &Env,
    cwd: &Path,
    spawner: &dyn Spawner,
    records: Option<&Records>,
) -> Output {
    if let Some(records) = records {
        records.forget(step);
    }
    let started = SystemTime::now();
    let output = run(step, env, cwd, spawner);
    if !output.success {
        return output;
    }
    let Some(records) = records else {
        return output;
    };
    let headers = match &step.headers {
        Some(headers) => std::fs::read(headers.file())
            .ok()
            .and_then(|bytes| headers.parse(&bytes)),
        None => Some(Vec::new()),
    };
    // Headers that cannot be read leave the step unrecorded: it runs again
    // next time rather than being judged on part of what it read.
    if let Some(headers) = headers {
        let inputs: Vec<PathBuf> = step
            .reads
            .iter()
            .cloned()
            .chain(waited.iter().map(|p| p.to_path_buf()))
            .chain(headers.into_iter().map(|h| deps::resolve(cwd, h)))
            .collect();
        let _ = records.record(step, &inputs, started);
    }
    output
}

/// Runs one step, its output directory made first. A step that cannot
/// start fails with why on its standard error.
fn run(step: &Step, env: &Env, cwd: &Path, spawner: &dyn Spawner) -> Output {
    let failed = |message: String| Output {
        success: false,
        stdout: Vec::new(),
        stderr: format!("{message}\n").into_bytes(),
    };
    if let Some(dir) = step.output.parent()
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        return failed(format!("could not create {}: {e}", dir.display()));
    }
    spawner
        .run(step, env, cwd)
        .unwrap_or_else(|e| failed(format!("could not run {}: {e}", step.program.display())))
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
    let first = &output.stdout[..end];
    let first = first.strip_suffix(b"\n").unwrap_or(first);
    let first = first.strip_suffix(b"\r").unwrap_or(first);
    if is_echo(first, name) {
        output.stdout.drain(..end);
    }
    output
}

/// Whether `line` is cl's echo of the file `name`. An ASCII name is
/// compared exactly. A name with other characters reaches cl's output in
/// the console's code page (CP932 on Japanese Windows), not UTF-8, so it is
/// recognised by its ASCII start and end around non-ASCII bytes, and by
/// holding no `:`, which no Windows file name has and every cl diagnostic
/// does (`main.cpp(3): error ...`).
fn is_echo(line: &[u8], name: &str) -> bool {
    let name = name.as_bytes();
    if name.is_ascii() {
        return line == name;
    }
    let head = name.iter().take_while(|b| b.is_ascii()).count();
    let tail = name.iter().rev().take_while(|b| b.is_ascii()).count();
    line.len() > head + tail
        && line.starts_with(&name[..head])
        && line.ends_with(&name[name.len() - tail..])
        && !line[head].is_ascii()
        && !line.contains(&b':')
}

#[cfg(test)]
mod tests;
