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
use crate::graph::{Graph, Step, StepKind};
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
            // First, so a color flag of the user's own, later, wins.
            .args(&step.color)
            .args(step.run_args())
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
            .chain(headers.into_iter().map(|h| deps::resolve(cwd, h)))
            .collect();
        let built: Vec<PathBuf> = waited.iter().map(|p| p.to_path_buf()).collect();
        let _ = records.record(step, &inputs, &built, started);
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
    let output = spawner
        .run(step, env, cwd)
        .unwrap_or_else(|e| failed(format!("could not run {}: {e}", step.program.display())));
    // A match rather than an `if`, so a new kind of step has to say whether
    // it needs a stamp.
    match step.kind {
        // A check writes no object, only its header file; the stamp is the
        // output its record is judged by, as an object is for a compile.
        StepKind::Check { .. } if output.success => {
            if let Err(e) = std::fs::write(&step.output, b"") {
                return failed(format!("could not write {}: {e}", step.output.display()));
            }
        }
        StepKind::Check { .. } | StepKind::Compile { .. } | StepKind::Link { .. } => {}
    }
    output
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
mod tests {
    use super::*;
    use crate::deps::Headers;
    use crate::graph::StepKind;
    use std::ffi::OsStr;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    fn step(name: &str, after: &[usize]) -> Step {
        Step {
            kind: StepKind::Compile {
                source: PathBuf::from(name),
            },
            program: PathBuf::from(name),
            args: Vec::new(),
            output: PathBuf::new(),
            after: after.to_vec(),
            echo: None,
            headers: None,
            reads: Vec::new(),
            searched: Vec::new(),
            looked_for: Vec::new(),
            color: Vec::new(),
            dependency: Vec::new(),
        }
    }

    /// A spawner whose steps take `ms` milliseconds each, fail when their
    /// program's name starts with `bad`, print their name, and record how many
    /// ran at once and in what order they started.
    #[derive(Default)]
    struct Fake {
        ms: u64,
        now: AtomicUsize,
        most: AtomicUsize,
        started: Mutex<Vec<String>>,
    }

    impl Spawner for Fake {
        fn run(&self, step: &Step, _env: &Env, _cwd: &Path) -> io::Result<Output> {
            let name = step.program.display().to_string();
            self.started.lock().unwrap().push(name.clone());
            let now = self.now.fetch_add(1, Ordering::SeqCst) + 1;
            self.most.fetch_max(now, Ordering::SeqCst);
            let ms = if name.starts_with("fast") { 1 } else { self.ms };
            std::thread::sleep(Duration::from_millis(ms));
            self.now.fetch_sub(1, Ordering::SeqCst);
            Ok(Output {
                success: !name.contains("bad"),
                stdout: format!("{name}\n").into_bytes(),
                stderr: Vec::new(),
            })
        }
    }

    fn run(graph: &Graph, jobs: usize, fake: &Fake) -> (Result<(), Failed>, Vec<String>) {
        let mut seen = Vec::new();
        let result = execute(
            graph,
            &Env::default(),
            Path::new("."),
            jobs,
            fake,
            None,
            &mut |event| match event {
                Event::Started(s) => seen.push(format!("start {}", s.program.display())),
                Event::Finished(s, out) => seen.push(format!(
                    "end {} {}",
                    s.program.display(),
                    String::from_utf8_lossy(&out.stdout).trim_end()
                )),
            },
        );
        (result, seen)
    }

    /// At most `jobs` steps run at once, and as many as that when they can.
    ///
    /// Mutation: ignore `jobs`, or run one step at a time; this test fails.
    #[test]
    fn jobs_caps_the_steps_running_at_once() {
        let graph = Graph {
            steps: (0..6).map(|i| step(&format!("s{i}"), &[])).collect(),
        };
        let fake = Fake {
            ms: 40,
            ..Fake::default()
        };
        let (result, _) = run(&graph, 2, &fake);
        assert_eq!(result, Ok(()));
        assert_eq!(fake.most.load(Ordering::SeqCst), 2);
    }

    /// A step starts only after every step it waits for has finished.
    ///
    /// Mutation: start a step before those it waits for finish; this test
    /// fails.
    #[test]
    fn a_step_waits_for_those_it_comes_after() {
        let graph = Graph {
            steps: vec![step("a", &[]), step("b", &[]), step("link", &[0, 1])],
        };
        let fake = Fake {
            ms: 20,
            ..Fake::default()
        };
        let (result, seen) = run(&graph, 4, &fake);
        assert_eq!(result, Ok(()));
        let link = seen.iter().position(|e| e == "start link").unwrap();
        for done in ["end a a", "end b b"] {
            assert!(
                seen.iter().position(|e| e == done).unwrap() < link,
                "{seen:?}"
            );
        }
    }

    /// After a failure nothing new starts, the steps already running are
    /// waited for and heard, and the failure is returned.
    ///
    /// Mutation: keep starting steps after a failure, stop listening to the
    /// running ones, or report success; this test fails.
    #[test]
    fn a_failure_stops_new_steps_and_waits_for_the_running() {
        let graph = Graph {
            steps: vec![
                step("fast-bad", &[]),
                step("slow", &[]),
                step("later", &[]),
                step("link", &[0, 1, 2]),
            ],
        };
        let fake = Fake {
            ms: 60,
            ..Fake::default()
        };
        let (result, seen) = run(&graph, 2, &fake);
        assert_eq!(result, Err(Failed(vec![0])));
        // The two start on their own threads, in either order.
        let mut started = fake.started.lock().unwrap().clone();
        started.sort();
        assert_eq!(started, ["fast-bad", "slow"], "{seen:?}");
        assert!(seen.contains(&"end slow slow".to_owned()), "{seen:?}");
    }

    /// cl.exe's first line is dropped only when it is exactly the source's
    /// file name; a line that merely contains it, and a step with no echo,
    /// keep everything.
    ///
    /// Mutation: drop the line on a partial match, drop more than the line,
    /// or drop it for a step with no echo; this test fails.
    #[test]
    fn only_an_exact_echo_line_is_dropped() {
        let echo = |name: Option<&str>| Step {
            echo: name.map(str::to_owned),
            ..step("cl", &[])
        };
        let out = |text: &str| Output {
            success: true,
            stdout: text.as_bytes().to_vec(),
            stderr: Vec::new(),
        };
        let stdout = |s: Option<&str>, text: &str| without_echo(&echo(s), out(text)).stdout;
        assert_eq!(
            stdout(Some("main.cpp"), "main.cpp\r\nwarning\r\n"),
            b"warning\r\n"
        );
        assert_eq!(stdout(Some("main.cpp"), "main.cpp"), b"");
        assert_eq!(
            stdout(Some("main.cpp"), "main.cpp(3): error C2065\r\n"),
            b"main.cpp(3): error C2065\r\n"
        );
        assert_eq!(stdout(None, "main.cpp\r\n"), b"main.cpp\r\n");
    }

    /// A step that cannot start fails with why, rather than stopping the run.
    ///
    /// Mutation: report it as success, or lose the reason; this test fails.
    #[test]
    fn a_step_that_cannot_start_fails_with_why() {
        struct Missing;
        impl Spawner for Missing {
            fn run(&self, _s: &Step, _e: &Env, _c: &Path) -> io::Result<Output> {
                Err(io::Error::new(io::ErrorKind::NotFound, "program not found"))
            }
        }
        let mut stderr = String::new();
        let result = execute(
            &Graph {
                steps: vec![step("cl", &[])],
            },
            &Env::default(),
            Path::new("."),
            1,
            &Missing,
            None,
            &mut |event| {
                if let Event::Finished(_, out) = event {
                    stderr = String::from_utf8_lossy(&out.stderr).into_owned();
                }
            },
        );
        assert_eq!(result, Err(Failed(vec![0])));
        assert_eq!(stderr, "could not run cl: program not found\n");
    }

    /// A step whose run panics fails and reports back, so the run ends rather
    /// than waiting for it for ever.
    ///
    /// Mutation: let a panicking step report nothing; this test fails (by
    /// timing out instead of hanging the suite).
    #[test]
    fn a_panicking_step_fails_rather_than_hangs() {
        struct Panics;
        impl Spawner for Panics {
            fn run(&self, _s: &Step, _e: &Env, _c: &Path) -> io::Result<Output> {
                panic!("a bug in the spawner");
            }
        }
        let (done, finished) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let graph = Graph {
                steps: vec![step("cl", &[])],
            };
            let result = execute(
                &graph,
                &Env::default(),
                Path::new("."),
                1,
                &Panics,
                None,
                &mut |_| {},
            );
            let _ = done.send(result);
        });
        let result = finished
            .recv_timeout(Duration::from_secs(10))
            .expect("execute returned");
        assert_eq!(result, Err(Failed(vec![0])));
    }

    /// An output directory that cannot be made is named as the failure, not
    /// the program.
    ///
    /// Mutation: report it as the program failing to run; this test fails.
    #[test]
    fn an_output_directory_that_cannot_be_made_is_named() {
        let root = tempfile::tempdir().unwrap();
        let blocker = root.path().join("target");
        std::fs::write(&blocker, "a file").unwrap();
        let step = Step {
            output: blocker.join("debug").join("a.obj"),
            ..step("cl", &[])
        };
        let mut stderr = String::new();
        let fake = Fake::default();
        let result = execute(
            &Graph { steps: vec![step] },
            &Env::default(),
            root.path(),
            1,
            &fake,
            None,
            &mut |event| {
                if let Event::Finished(_, out) = event {
                    stderr = String::from_utf8_lossy(&out.stderr).into_owned();
                }
            },
        );
        assert_eq!(result, Err(Failed(vec![0])));
        assert!(
            stderr.starts_with(&format!(
                "could not create {}: ",
                blocker.join("debug").display()
            )),
            "{stderr}"
        );
        assert!(fake.started.lock().unwrap().is_empty());
    }

    /// cl prints a non-ASCII file name in the console's code page: `日本.cpp`
    /// arrives as CP932 bytes and is still the echo, while a diagnostic that
    /// starts with it is not.
    ///
    /// Mutation: compare the line as UTF-8 text; this test fails.
    #[test]
    fn a_non_ascii_echo_in_the_console_code_page_is_dropped() {
        let step = Step {
            echo: Some("日本.cpp".to_owned()),
            ..step("cl", &[])
        };
        let cp932_name = [0x93, 0xfa, 0x96, 0x7b, b'.', b'c', b'p', b'p'];
        let out = |line: &[u8]| Output {
            success: true,
            stdout: [line, b"\r\nnext\r\n"].concat(),
            stderr: Vec::new(),
        };
        assert_eq!(without_echo(&step, out(&cp932_name)).stdout, b"next\r\n");
        let diagnostic = [&cp932_name[..], b"(3): error C2065"].concat();
        assert_eq!(
            without_echo(&step, out(&diagnostic)).stdout,
            out(&diagnostic).stdout
        );
    }

    /// A compiler stand-in that writes its output and a dependency file naming
    /// `header`, fails while `fail` is set, and runs `during` while it
    /// "compiles".
    struct Compiles {
        header: PathBuf,
        fail: std::sync::atomic::AtomicBool,
        runs: AtomicUsize,
        during: Box<dyn Fn() + Sync>,
    }

    impl Spawner for Compiles {
        fn run(&self, step: &Step, _env: &Env, _cwd: &Path) -> io::Result<Output> {
            self.runs.fetch_add(1, Ordering::SeqCst);
            (self.during)();
            std::fs::write(&step.output, "obj")?;
            if let Some(headers) = &step.headers {
                // Named relative to the working directory, as a compiler names
                // a header found through a relative `-I`.
                let name = self.header.file_name().unwrap().to_string_lossy();
                std::fs::write(
                    headers.file(),
                    format!("out: {}\n", name.replace(' ', "\\ ")),
                )?;
            }
            Ok(Output {
                success: !self.fail.load(Ordering::SeqCst),
                stdout: Vec::new(),
                stderr: Vec::new(),
            })
        }
    }

    /// One compile of `a.cpp`, reading `a.h`, in a temporary directory.
    fn compile_case(during: Box<dyn Fn() + Sync>) -> (tempfile::TempDir, Graph, Compiles) {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("a.cpp");
        let header = root.path().join("a h.h");
        std::fs::write(&source, "int a;").unwrap();
        std::fs::write(&header, "#define A").unwrap();
        // An hour back, as a project's files are when a build starts: one
        // written just now is within the margin and leaves the step unrecorded
        // (`docs/en/specs/build.md`, "Deciding what changed").
        for file in [&source, &header] {
            std::fs::File::options()
                .write(true)
                .open(file)
                .unwrap()
                .set_modified(std::time::SystemTime::now() - Duration::from_secs(3600))
                .unwrap();
        }
        let output = root.path().join("obj").join("a.obj");
        let step = Step {
            output: output.clone(),
            headers: Some(Headers::DepFile(output.with_extension("d"))),
            reads: vec![source],
            searched: Vec::new(),
            looked_for: Vec::new(),
            color: Vec::new(),
            dependency: Vec::new(),
            ..step("clang", &[])
        };
        let spawner = Compiles {
            header,
            fail: Default::default(),
            runs: AtomicUsize::new(0),
            during,
        };
        (root, Graph { steps: vec![step] }, spawner)
    }

    fn build(root: &Path, graph: &Graph, spawner: &Compiles) -> Result<(), Failed> {
        let records = Records::new([OsStr::new("v1")], &[], cuno_core::NameCase::Insensitive);
        execute(
            graph,
            &Env::default(),
            root,
            1,
            spawner,
            Some(&records),
            &mut |_| {},
        )
    }

    /// With records, a step that succeeded is skipped until its source or a
    /// header it read changes; a step that fails is not recorded, so it runs
    /// again even with nothing changed. Each edit changes the file's size: an
    /// edit made within the file system's timestamp resolution that keeps the
    /// size is one the records do not see (`build.md`, "Deciding what
    /// changed"), and the test would then depend on the clock.
    ///
    /// Mutation: record a failed step, skip forgetting the record before a
    /// run, leave the headers or the source out of what is recorded, or take
    /// a relative header from the directory cuno runs in; this test fails.
    #[test]
    fn records_skip_what_succeeded_and_rerun_what_changed_or_failed() {
        let (root, graph, spawner) = compile_case(Box::new(|| {}));
        let root = root.path();
        assert_eq!(build(root, &graph, &spawner), Ok(()));
        assert_eq!(build(root, &graph, &spawner), Ok(()));
        assert_eq!(spawner.runs.load(Ordering::SeqCst), 1);

        std::fs::write(&spawner.header, "#define BB").unwrap();
        assert_eq!(build(root, &graph, &spawner), Ok(()));
        assert_eq!(spawner.runs.load(Ordering::SeqCst), 2);
        std::fs::write(root.join("a.cpp"), "int bb;").unwrap();
        assert_eq!(build(root, &graph, &spawner), Ok(()));
        assert_eq!(spawner.runs.load(Ordering::SeqCst), 3);

        std::fs::write(root.join("a.cpp"), "int ccc;").unwrap();
        spawner.fail.store(true, Ordering::SeqCst);
        assert_eq!(build(root, &graph, &spawner), Err(Failed(vec![0])));
        // Nothing changed since the failure, and it still runs.
        spawner.fail.store(false, Ordering::SeqCst);
        assert_eq!(build(root, &graph, &spawner), Ok(()));
        assert_eq!(spawner.runs.load(Ordering::SeqCst), 5);
    }

    /// A source saved while its compile runs leaves the step unrecorded, so
    /// the next build compiles what is now there. The compile goes on for
    /// longer than the margin after the save, so the test holds when the start
    /// is read, not only the margin.
    ///
    /// Mutation: take the inputs' stamps without the step's start time, or
    /// read the start after the compile; this test fails.
    #[test]
    fn a_source_saved_during_its_compile_is_compiled_again() {
        let saved = std::sync::Arc::new(Mutex::new(None::<PathBuf>));
        let during = {
            let saved = saved.clone();
            Box::new(move || {
                if let Some(source) = saved.lock().unwrap().take() {
                    std::thread::sleep(Duration::from_millis(20));
                    std::fs::write(source, "int saved;").unwrap();
                    std::thread::sleep(Duration::from_millis(2_200));
                }
            })
        };
        let (root, graph, spawner) = compile_case(during);
        *saved.lock().unwrap() = Some(root.path().join("a.cpp"));
        assert_eq!(build(root.path(), &graph, &spawner), Ok(()));
        assert_eq!(build(root.path(), &graph, &spawner), Ok(()));
        assert_eq!(spawner.runs.load(Ordering::SeqCst), 2);
    }

    /// A step with no jobs to run on still runs: `jobs` of 0 is taken as 1.
    ///
    /// Mutation: take 0 as given; this test fails.
    #[test]
    fn no_jobs_is_one_job() {
        let fake = Fake::default();
        let (result, _) = run(
            &Graph {
                steps: vec![step("cl", &[])],
            },
            0,
            &fake,
        );
        assert_eq!(result, Ok(()));
        assert_eq!(*fake.started.lock().unwrap(), ["cl"]);
    }

    /// A check that passes leaves its stamp, the file a later build judges it
    /// up to date by; one that fails leaves none, so it runs again. (`Fake`
    /// fails a program whose name contains `bad`.)
    ///
    /// Mutation: write no stamp, or write it whether the check passed or not;
    /// this test fails.
    #[test]
    fn a_passing_check_leaves_its_stamp() {
        let dir = tempfile::tempdir().unwrap();
        let check = |program: &str| Step {
            kind: StepKind::Check {
                source: PathBuf::from("a.cpp"),
            },
            output: dir.path().join(format!("obj/{program}.check")),
            ..step(program, &[])
        };
        let graph = Graph {
            steps: vec![check("fast"), check("fast-bad")],
        };
        let (result, _) = run(&graph, 1, &Fake::default());
        assert_eq!(result, Err(Failed(vec![1])));
        assert_eq!(std::fs::read(&graph.steps[0].output).unwrap(), b"");
        assert!(!graph.steps[1].output.exists());
    }
}
