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
        headers: None,
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
