use super::*;
use crate::graph::StepKind;
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
    assert_eq!(
        *fake.started.lock().unwrap(),
        ["fast-bad", "slow"],
        "{seen:?}"
    );
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
        &mut |event| {
            if let Event::Finished(_, out) = event {
                stderr = String::from_utf8_lossy(&out.stderr).into_owned();
            }
        },
    );
    assert_eq!(result, Err(Failed(vec![0])));
    assert_eq!(stderr, "could not run cl: program not found\n");
}
