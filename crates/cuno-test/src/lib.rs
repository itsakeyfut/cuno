//! Running test executables and reporting their results: each test in a
//! process of its own, up to `jobs` at once, its output captured and shown
//! for failures (or always, with `--nocapture`), the report printed as the
//! tests finish. See `docs/en/specs/testing.md`, "Execution" and "Output".
//! Building them is `cuno-build`'s (`docs/en/specs/architecture.md`).

pub mod record;
pub mod select;

use std::collections::VecDeque;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc;
use std::time::Instant;

use cuno_core::scratch::Scratch;
use cuno_core::{Env, NameCase, Shell};

use crate::record::{Ending, Report};

/// What a process printed and how it ended.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProcessOutput {
    /// Its exit code, or `None` when it was ended by a signal.
    pub code: Option<i32>,
    /// What it wrote to stdout.
    pub stdout: Vec<u8>,
    /// What it wrote to stderr.
    pub stderr: Vec<u8>,
}

/// Starts a test executable and waits for it, its output captured.
pub trait Launcher: Sync {
    /// Runs `exe` with `args` in `cwd`, with exactly the variables of `env`.
    fn launch(
        &self,
        exe: &Path,
        args: &[OsString],
        env: &Env,
        cwd: &Path,
    ) -> io::Result<ProcessOutput>;
}

/// The [`Launcher`] that starts real processes, with no standard input.
pub struct SystemLauncher;

impl Launcher for SystemLauncher {
    fn launch(
        &self,
        exe: &Path,
        args: &[OsString],
        env: &Env,
        cwd: &Path,
    ) -> io::Result<ProcessOutput> {
        let output = cuno_core::program::command(exe, args, env, cwd)
            .stdin(std::process::Stdio::null())
            .output()?;
        Ok(ProcessOutput {
            code: output.status.code(),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }
}

/// A test executable to run.
#[derive(Clone, Debug)]
pub struct TestExecutable {
    /// Where it is.
    pub path: PathBuf,
    /// Whether it is made from an integration test (a file under `tests/`),
    /// and so runs only the tests placed there (`testing.md`, "Execution").
    pub integration: bool,
}

/// One run of a package's tests.
pub struct TestRun<'a> {
    /// The executables, in the order to run them.
    pub executables: &'a [TestExecutable],
    /// Run only the tests whose names contain this.
    pub filter: Option<&'a str>,
    /// Show every test's output, not only a failed one's.
    pub nocapture: bool,
    /// How many tests run at once; zero is taken as one.
    pub jobs: usize,
    /// The project's root: where tests run, what places in failures are
    /// shown relative to, and where `tests/` is.
    pub root: &'a Path,
    /// The current directory, which executables' paths are shown relative to,
    /// as `cuno run` shows its executable (`commands.md`, "`cuno test
    /// [filter]`").
    pub cwd: &'a Path,
    /// The environment the tests get, as cuno was given it.
    pub env: &'a Env,
    /// A directory of cuno's in which the run makes a directory of its own
    /// for the result files, removed when it ends, so two runs sharing it
    /// never read each other's results.
    pub scratch: &'a Path,
    /// How the host compares names.
    pub case: NameCase,
    /// What starts the executables.
    pub launcher: &'a dyn Launcher,
}

/// How a run went.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    /// Tests that passed.
    pub passed: usize,
    /// Tests that failed, crashes included.
    pub failed: usize,
    /// Tests skipped.
    pub skipped: usize,
    /// Executables whose tests could not be listed, and so did not run.
    pub unlisted: usize,
}

impl Summary {
    /// Whether everything that was to run ran and passed: what `cuno test`
    /// turns into exit code 0 (`commands.md`, "`cuno test [filter]`").
    pub fn succeeded(&self) -> bool {
        self.failed == 0 && self.unlisted == 0
    }
}

/// The verdict on one test.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Verdict {
    /// It ran to its end with no failed check, and its process succeeded.
    Passed,
    /// It skipped itself with no failed check before, for this reason.
    Skipped(String),
    /// Anything else: a failed check, a crash or an early exit, a result
    /// cuno cannot read, or a process that failed after reporting success.
    Failed,
}

/// What became of one test.
struct Outcome {
    /// The test's name.
    name: String,
    /// What its process printed and how it ended.
    output: ProcessOutput,
    /// What its result file said.
    report: Report,
    /// The verdict on it.
    verdict: Verdict,
    /// Why it failed, when its report does not say: it did not finish, or
    /// its file could not be read.
    note: Option<String>,
}

/// Why an executable's tests could not be listed.
struct ListFailure {
    /// The `error:` line.
    message: String,
    /// The `help:` line, when there is one.
    help: Option<&'static str>,
    /// What the executable printed, when it ran.
    output: Option<ProcessOutput>,
}

impl ListFailure {
    fn new(message: String) -> ListFailure {
        ListFailure {
            message,
            help: None,
            output: None,
        }
    }
}

/// Runs every executable's tests, printing what `testing.md`, "Output",
/// shows, and returns how it went.
pub fn run(r: &TestRun<'_>, shell: &mut Shell) -> Summary {
    let mut summary = Summary::default();
    let results = match own_directory(r.scratch) {
        Ok(scratch) => scratch,
        Err(e) => {
            shell.error(format!(
                "cannot make a directory for the test results in {}: {e}",
                r.scratch.display()
            ));
            summary.unlisted = r.executables.len();
            return summary;
        }
    };
    for (index, exe) in r.executables.iter().enumerate() {
        shell.status(
            "Running",
            format!(
                "`{}`",
                cuno_core::program::shown(&exe.path, r.cwd).display()
            ),
        );
        match list(r, results.path(), index, exe) {
            Ok(names) => run_executable(r, results.path(), index, &names, shell, &mut summary),
            Err(failure) => {
                summary.unlisted += 1;
                if let Some(output) = &failure.output {
                    raw_lines(shell, &output.stdout);
                    raw_lines(shell, &output.stderr);
                }
                shell.error(failure.message);
                if let Some(help) = failure.help {
                    shell.help(help);
                }
            }
        }
    }
    drop(results);
    summary
}

/// A new directory in `scratch`, the output directory's place for them,
/// that no other run uses, removed when dropped.
fn own_directory(scratch: &Path) -> io::Result<Scratch> {
    std::fs::create_dir_all(scratch)?;
    Scratch::new(scratch, "test").map_err(|(_, e)| e)
}

/// An exit code as a person reads it: a Windows crash code, which comes out
/// negative, in hexadecimal (`0xC0000005`) as it is documented and searched
/// for (`testing.md`, "Output").
fn exit_code(code: Option<i32>) -> String {
    match code {
        None => "no exit code".to_owned(),
        Some(c) if c < 0 => format!("exit code 0x{:08X}", c as u32),
        Some(c) => format!("exit code {c}"),
    }
}

/// A process's output, ended with a line break if it did not end with one,
/// so the report's next line starts on a line of its own.
fn raw_lines(shell: &mut Shell, bytes: &[u8]) {
    shell.raw(bytes);
    if bytes.last().is_some_and(|&b| b != b'\n') {
        shell.raw(b"\n");
    }
}

/// The names of `exe`'s tests to run.
fn list(
    r: &TestRun<'_>,
    results: &Path,
    index: usize,
    exe: &TestExecutable,
) -> Result<Vec<String>, ListFailure> {
    let file = results.join(format!("list-{index}.txt"));
    let env = r
        .env
        .with("CUNO_TEST_RESULT", file.clone().into_os_string());
    let path = cuno_core::program::shown(&exe.path, r.cwd).display();
    let output = r
        .launcher
        .launch(&exe.path, &[OsString::from("--list")], &env, r.root)
        .map_err(|e| ListFailure::new(format!("cannot run {path}: {e}")))?;
    if output.code != Some(0) {
        return Err(ListFailure {
            message: format!(
                "{path} ended with {} listing its tests",
                exit_code(output.code)
            ),
            help: None,
            output: Some(output),
        });
    }
    // A list that is not there is not an empty list: taking it as one
    // would report success for tests that never ran.
    let text = std::fs::read_to_string(&file).map_err(|e| ListFailure {
        message: format!("{path} wrote no list of its tests that cuno can read: {e}"),
        help: None,
        output: Some(output.clone()),
    })?;
    let registrations =
        record::registrations(&text).map_err(|e| ListFailure::new(format!("{path}: {e}")))?;
    select::tests_to_run(&registrations, exe.integration, r.filter, r.root, r.case).map_err(|e| {
        ListFailure {
            message: e.to_string(),
            help: Some(e.help()),
            output: None,
        }
    })
}

/// Runs `names` from executable `index`, up to `r.jobs` at once, printing
/// each test's line as it finishes, then the failures and the result line.
fn run_executable(
    r: &TestRun<'_>,
    results: &Path,
    index: usize,
    names: &[String],
    shell: &mut Shell,
    summary: &mut Summary,
) {
    let exe = &r.executables[index];
    let started = Instant::now();
    shell.raw(format!("\nrunning {} tests\n", names.len()).as_bytes());
    let queue: Mutex<VecDeque<(usize, &String)>> = Mutex::new(names.iter().enumerate().collect());
    let (sender, receiver) = mpsc::channel::<Outcome>();
    let mut failures: Vec<Outcome> = Vec::new();
    let (mut passed, mut failed, mut skipped) = (0, 0, 0);
    // At least one worker, and none idle.
    let workers = r.jobs.max(1).min(names.len().max(1));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let sender = sender.clone();
            let queue = &queue;
            scope.spawn(move || {
                loop {
                    let next = queue.lock().unwrap().pop_front();
                    let Some((position, name)) = next else { break };
                    let file = results.join(format!("result-{index}-{position}.txt"));
                    let _ = sender.send(run_test(r, exe, &file, name));
                }
            });
        }
        drop(sender);
        // Printed here alone, one test at a time, so outputs never mix.
        for outcome in receiver {
            let status = match &outcome.verdict {
                Verdict::Passed => "ok".to_owned(),
                Verdict::Skipped(reason) if reason.is_empty() => "skipped".to_owned(),
                Verdict::Skipped(reason) => format!("skipped ({reason})"),
                Verdict::Failed => "FAILED".to_owned(),
            };
            shell.raw(format!("test {} ... {status}\n", outcome.name).as_bytes());
            if r.nocapture {
                raw_lines(shell, &outcome.output.stdout);
                raw_lines(shell, &outcome.output.stderr);
            }
            match outcome.verdict {
                Verdict::Passed => passed += 1,
                Verdict::Skipped(_) => skipped += 1,
                Verdict::Failed => {
                    failed += 1;
                    failures.push(outcome);
                }
            }
        }
    });
    if !failures.is_empty() {
        shell.raw(b"\nfailures:\n");
        for outcome in &failures {
            shell.raw(format!("---- {} ----\n", outcome.name).as_bytes());
            for f in &outcome.report.failures {
                let place = select::shown(&f.file, r.root, r.case);
                shell.raw(format!("{place}:{}: {}\n", f.line, f.message).as_bytes());
            }
            for info in &outcome.report.infos {
                shell.raw(format!("info: {info}\n").as_bytes());
            }
            if let Some(note) = &outcome.note {
                shell.raw(format!("{note}\n").as_bytes());
            }
            // Already shown after the test's line under `--nocapture`.
            if !r.nocapture {
                raw_lines(shell, &outcome.output.stdout);
                raw_lines(shell, &outcome.output.stderr);
            }
        }
    }
    let verdict = if failed == 0 { "ok" } else { "FAILED" };
    shell.raw(
        format!(
            "\ntest result: {verdict}. {passed} passed; {failed} failed; {skipped} skipped; finished in {:.2}s\n",
            started.elapsed().as_secs_f64()
        )
        .as_bytes(),
    );
    summary.passed += passed;
    summary.failed += failed;
    summary.skipped += skipped;
}

/// Runs test `name` of `exe` alone, its result in `file`, and judges it by
/// that file (`testing.md`, "Contract between cuno and test executables").
fn run_test(r: &TestRun<'_>, exe: &TestExecutable, file: &Path, name: &str) -> Outcome {
    let env = r
        .env
        .with("CUNO_TEST_RESULT", file.to_owned().into_os_string());
    let args = [OsString::from("--exact"), OsString::from(name)];
    let output = match r.launcher.launch(&exe.path, &args, &env, r.root) {
        Ok(output) => output,
        Err(e) => {
            return Outcome {
                name: name.to_owned(),
                output: ProcessOutput::default(),
                report: no_report(),
                verdict: Verdict::Failed,
                note: Some(format!(
                    "cannot run {}: {e}",
                    cuno_core::program::shown(&exe.path, r.cwd).display()
                )),
            };
        }
    };
    let text = match std::fs::read_to_string(file) {
        Ok(text) => Ok(Some(text)),
        // No file is a test that did not finish, which the report says.
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("cannot read the test's result: {e}")),
    };
    let (report, note) = match text {
        Ok(text) => match record::report(text.as_deref()) {
            Ok(report) => (report, None),
            Err(e) => (no_report(), Some(e.to_string())),
        },
        Err(note) => (no_report(), Some(note)),
    };
    let code = exit_code(output.code);
    let (verdict, note) = match (&report.ending, report.failures.is_empty(), note) {
        (_, _, Some(note)) => (Verdict::Failed, Some(note)),
        (Ending::Unfinished, _, None) => (
            Verdict::Failed,
            Some(format!(
                "the test ended without reporting a result ({code})"
            )),
        ),
        // A skip after a failed check fails (testing.md, "Contract between
        // cuno and test executables").
        (_, false, None) => (Verdict::Failed, None),
        // Reported as finished, yet its process failed after: a crash on
        // the way out is a failure too.
        (_, true, None) if output.code != Some(0) => (
            Verdict::Failed,
            Some(format!(
                "the test reported no failure, yet ended with {code}"
            )),
        ),
        (Ending::Skipped(reason), true, None) => (Verdict::Skipped(reason.clone()), None),
        (Ending::Done, true, None) => (Verdict::Passed, None),
    };
    Outcome {
        name: name.to_owned(),
        output,
        report,
        verdict,
        note,
    }
}

/// The report of a test whose result file said nothing usable: no failed
/// check, and not finished.
fn no_report() -> Report {
    Report {
        failures: Vec::new(),
        infos: Vec::new(),
        ending: Ending::Unfinished,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::Write;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    /// How a fake test behaves: what it writes to its result file, its exit
    /// code and its output.
    #[derive(Clone, Default)]
    struct Script {
        records: Option<&'static str>,
        code: Option<i32>,
        stdout: &'static str,
        stderr: &'static str,
        /// Whether `--exact` cannot start the process.
        cannot_start: bool,
    }

    /// A launcher that plays scripts instead of starting processes.
    #[derive(Default)]
    struct Fake {
        /// Per executable path: its `--list` records, or `Err` for an exit
        /// code to end listing with.
        lists: HashMap<PathBuf, Result<String, i32>>,
        /// Executables that list with exit code 0 and write no file.
        no_list_file: Vec<PathBuf>,
        /// Executables that cannot be started.
        unstartable: Vec<PathBuf>,
        /// Per test name.
        tests: HashMap<String, Script>,
        running: AtomicUsize,
        most: AtomicUsize,
        hold: Duration,
    }

    impl Launcher for Fake {
        fn launch(
            &self,
            exe: &Path,
            args: &[OsString],
            env: &Env,
            _cwd: &Path,
        ) -> io::Result<ProcessOutput> {
            let result = PathBuf::from(env.get("CUNO_TEST_RESULT").unwrap());
            if self.unstartable.iter().any(|u| u == exe) {
                return Err(io::Error::new(io::ErrorKind::NotFound, "not there"));
            }
            if args[0] == "--list" && self.no_list_file.iter().any(|u| u == exe) {
                return Ok(ProcessOutput {
                    code: Some(0),
                    ..ProcessOutput::default()
                });
            }
            if args[0] == "--list" {
                return match &self.lists[exe] {
                    Ok(records) => {
                        std::fs::write(result, records)?;
                        Ok(ProcessOutput {
                            code: Some(0),
                            ..ProcessOutput::default()
                        })
                    }
                    Err(code) => Ok(ProcessOutput {
                        code: Some(*code),
                        stderr: b"static init failed\n".to_vec(),
                        ..ProcessOutput::default()
                    }),
                };
            }
            let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
            self.most.fetch_max(now, Ordering::SeqCst);
            std::thread::sleep(self.hold);
            self.running.fetch_sub(1, Ordering::SeqCst);
            let script = self.tests[args[1].to_str().unwrap()].clone();
            if script.cannot_start {
                return Err(io::Error::new(io::ErrorKind::NotFound, "gone"));
            }
            if let Some(records) = script.records {
                std::fs::write(result, records)?;
            }
            Ok(ProcessOutput {
                code: script.code,
                stdout: script.stdout.as_bytes().to_vec(),
                stderr: script.stderr.as_bytes().to_vec(),
            })
        }
    }

    /// What a shell wrote.
    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl Write for Captured {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    const ROOT: &str = "C:/p";

    fn listing(names: &[&str]) -> String {
        names
            .iter()
            .enumerate()
            .map(|(i, n)| format!("test\t{n}\tC:/p/src/a.cpp\t{}\tmodern\t1\n", i + 1))
            .collect()
    }

    fn passing() -> Script {
        Script {
            records: Some("done\n"),
            code: Some(0),
            ..Script::default()
        }
    }

    /// Runs `exes` with `fake`, returning the summary and what was printed,
    /// the timing replaced with `T`.
    fn run_with(
        fake: &Fake,
        exes: &[TestExecutable],
        jobs: usize,
        nocapture: bool,
        filter: Option<&str>,
    ) -> (Summary, String) {
        let scratch = tempfile::tempdir().unwrap();
        run_in(fake, exes, jobs, nocapture, filter, scratch.path())
    }

    /// [`run_with`] in the scratch directory `scratch`.
    fn run_in(
        fake: &Fake,
        exes: &[TestExecutable],
        jobs: usize,
        nocapture: bool,
        filter: Option<&str>,
        scratch: &Path,
    ) -> (Summary, String) {
        let env = Env::from_pairs([], NameCase::Insensitive);
        let out = Captured::default();
        let mut shell = Shell::new(Box::new(out.clone()), None, &env, false);
        let summary = run(
            &TestRun {
                executables: exes,
                filter,
                nocapture,
                jobs,
                root: Path::new(ROOT),
                cwd: Path::new(ROOT),
                env: &env,
                scratch,
                case: NameCase::Insensitive,
                launcher: fake,
            },
            &mut shell,
        );
        let text = String::from_utf8(out.0.lock().unwrap().clone()).unwrap();
        let timed = text
            .lines()
            .map(|line| match line.find("finished in ") {
                Some(at) => format!("{}finished in T", &line[..at]),
                None => line.to_owned(),
            })
            .collect::<Vec<_>>()
            .join("\n");
        (summary, timed)
    }

    fn exe(path: &str) -> TestExecutable {
        TestExecutable {
            path: PathBuf::from(path),
            integration: false,
        }
    }

    /// The report reads as `testing.md`, "Output", shows: the `Running`
    /// line, the count, a line per test, the failures with their places
    /// relative to the root, their `info` and output, and the result line.
    /// A passing test's output is not shown.
    ///
    /// Mutation: change any line's wording, show a passing test's output,
    /// leave a failure's output out, or show places as spelled; this test
    /// fails.
    #[test]
    fn the_report_reads_as_the_spec_shows() {
        let mut fake = Fake::default();
        fake.lists.insert(
            PathBuf::from("C:/p/target/app.exe"),
            Ok(listing(&["add works", "parse rejects empty", "slow path"])),
        );
        fake.tests.insert(
            "add works".into(),
            Script {
                stdout: "quiet\n",
                ..passing()
            },
        );
        fake.tests.insert(
            "parse rejects empty".into(),
            Script {
                records: Some(
                    "info\tinput = \"\"\nfail\tC:/p/src/parser.cpp\t42\teq(result, 0) failed\\n  left:  -1\\n  right: 0\ndone\n",
                ),
                code: Some(1),
                stdout: "parsing\n",
                stderr: "",
                cannot_start: false,
            },
        );
        fake.tests.insert(
            "slow path".into(),
            Script {
                records: Some("skip\ttoo slow\n"),
                code: Some(0),
                ..Script::default()
            },
        );
        let (summary, text) = run_with(&fake, &[exe("C:/p/target/app.exe")], 1, false, None);
        assert_eq!(
            text,
            "     Running `target/app.exe`\n\
             \n\
             running 3 tests\n\
             test add works ... ok\n\
             test parse rejects empty ... FAILED\n\
             test slow path ... skipped (too slow)\n\
             \n\
             failures:\n\
             ---- parse rejects empty ----\n\
             src/parser.cpp:42: eq(result, 0) failed\n  left:  -1\n  right: 0\n\
             info: input = \"\"\n\
             parsing\n\
             \n\
             test result: FAILED. 1 passed; 1 failed; 1 skipped; finished in T"
                .replace(
                    "target/app.exe",
                    &Path::new("target/app.exe").display().to_string()
                )
        );
        assert_eq!(
            summary,
            Summary {
                passed: 1,
                failed: 1,
                skipped: 1,
                unlisted: 0,
            }
        );
        assert!(!summary.succeeded());
    }

    /// A test that ends without a result (a crash) fails alone, with its
    /// exit code shown; one that reports no failure but then fails its
    /// process fails too; one that skips after a failure fails.
    ///
    /// Mutation: trust the exit code over a missing result, a passing
    /// result over a failing exit code, or a skip over a failure; this test
    /// fails.
    #[test]
    fn a_test_that_did_not_finish_fails_alone() {
        let mut fake = Fake::default();
        fake.lists.insert(
            PathBuf::from("C:/p/app.exe"),
            Ok(listing(&["crash", "after", "skip after", "fine"])),
        );
        fake.tests.insert(
            "crash".into(),
            Script {
                records: None,
                code: Some(0),
                stderr: "boom\n",
                ..Script::default()
            },
        );
        fake.tests.insert(
            "after".into(),
            Script {
                records: Some("done\n"),
                code: Some(3),
                ..Script::default()
            },
        );
        fake.tests.insert(
            "skip after".into(),
            Script {
                records: Some("fail\ta.c\t1\tx failed\nskip\tlater\n"),
                code: Some(1),
                ..Script::default()
            },
        );
        fake.tests.insert("fine".into(), passing());
        let (summary, text) = run_with(&fake, &[exe("C:/p/app.exe")], 1, false, None);
        assert_eq!((summary.passed, summary.failed), (1, 3), "{text}");
        assert!(
            text.contains(
                "---- crash ----\nthe test ended without reporting a result (exit code 0)\nboom\n"
            ),
            "{text}"
        );
        assert!(
            text.contains(
                "---- after ----\nthe test reported no failure, yet ended with exit code 3\n"
            ),
            "{text}"
        );
        assert!(text.contains("test skip after ... FAILED\n"), "{text}");
    }

    /// Tests run up to `jobs` at once, never more, and with one job one at
    /// a time.
    ///
    /// Mutation: ignore `jobs`, or start a worker per test; this test
    /// fails.
    #[test]
    fn as_many_tests_run_at_once_as_jobs_says() {
        for (jobs, most) in [(3, 3), (1, 1)] {
            let mut fake = Fake {
                hold: Duration::from_millis(60),
                ..Fake::default()
            };
            let names = ["a", "b", "c", "d", "e", "f"];
            fake.lists
                .insert(PathBuf::from("C:/p/app.exe"), Ok(listing(&names)));
            for name in names {
                fake.tests.insert(name.into(), passing());
            }
            let (summary, _) = run_with(&fake, &[exe("C:/p/app.exe")], jobs, false, None);
            assert_eq!(summary.passed, 6);
            assert_eq!(fake.most.load(Ordering::SeqCst), most, "jobs {jobs}");
        }
    }

    /// With `--nocapture` every test's output follows its own line, whole,
    /// even when tests run in parallel.
    ///
    /// Mutation: print output as it arrives, show only failures' under
    /// `--nocapture`, or show it again under the failures; this test
    /// fails.
    #[test]
    fn nocapture_shows_each_tests_output_after_its_line() {
        let mut fake = Fake {
            hold: Duration::from_millis(20),
            ..Fake::default()
        };
        let names = ["a", "b", "c", "d"];
        fake.lists
            .insert(PathBuf::from("C:/p/app.exe"), Ok(listing(&names)));
        for name in names {
            let stdout: &'static str = Box::leak(format!("out of {name}\n").into_boxed_str());
            fake.tests.insert(
                name.into(),
                Script {
                    stdout,
                    ..passing()
                },
            );
        }
        let (_, text) = run_with(&fake, &[exe("C:/p/app.exe")], 4, true, None);
        for name in names {
            assert!(
                text.contains(&format!("test {name} ... ok\nout of {name}\n")),
                "{text}"
            );
            assert_eq!(text.matches(&format!("out of {name}")).count(), 1, "{text}");
        }
    }

    /// An executable whose list cannot be taken is an error with its
    /// output, counted, and the next executable still runs; one made from
    /// `tests/` runs only the tests placed there; the filter applies.
    ///
    /// Mutation: stop at the first unlisted executable, drop its output,
    /// skip the selection, or ignore the filter; this test fails.
    #[test]
    fn listing_selects_what_runs() {
        let mut fake = Fake::default();
        fake.lists.insert(PathBuf::from("C:/p/broken.exe"), Err(3));
        fake.lists.insert(
            PathBuf::from("C:/p/tests-a.exe"),
            Ok("test\tin source\tC:/p/src/a.cpp\t1\tmodern\t1\n\
                test\tadds one\tC:/p/tests/a.cpp\t2\tmodern\t1\n\
                test\tadds two\tC:/p/tests/a.cpp\t3\tmodern\t1\n"
                .into()),
        );
        fake.tests.insert("adds one".into(), passing());
        fake.tests.insert("adds two".into(), passing());
        let exes = [
            exe("C:/p/broken.exe"),
            TestExecutable {
                path: PathBuf::from("C:/p/tests-a.exe"),
                integration: true,
            },
        ];
        let (summary, text) = run_with(&fake, &exes, 1, false, Some("one"));
        assert_eq!(
            summary,
            Summary {
                passed: 1,
                failed: 0,
                skipped: 0,
                unlisted: 1,
            }
        );
        assert!(
            text.contains(
                "static init failed\nerror: broken.exe ended with exit code 3 listing its tests\n"
            ),
            "{text}"
        );
        assert!(
            text.contains("running 1 tests\ntest adds one ... ok\n"),
            "{text}"
        );
        assert!(!summary.succeeded());
    }

    /// Under `--nocapture` a failed test's output follows its line and is
    /// not shown a second time among the failures.
    ///
    /// Mutation: show the captured output among the failures under
    /// `--nocapture` too; this test fails.
    #[test]
    fn nocapture_shows_a_failures_output_once() {
        let mut fake = Fake::default();
        fake.lists
            .insert(PathBuf::from("C:/p/app.exe"), Ok(listing(&["bad"])));
        fake.tests.insert(
            "bad".into(),
            Script {
                records: Some("fail\ta.c\t1\tx failed\ndone\n"),
                code: Some(1),
                stdout: "said once\n",
                stderr: "",
                cannot_start: false,
            },
        );
        let (_, text) = run_with(&fake, &[exe("C:/p/app.exe")], 1, true, None);
        assert!(text.contains("test bad ... FAILED\nsaid once\n"), "{text}");
        assert_eq!(text.matches("said once").count(), 1, "{text}");
    }

    /// A result file cuno cannot read fails the test, saying so, rather
    /// than passing it.
    ///
    /// Mutation: pass a test whose file cannot be read; this test fails.
    #[test]
    fn an_unreadable_result_fails_the_test() {
        let mut fake = Fake::default();
        fake.lists
            .insert(PathBuf::from("C:/p/app.exe"), Ok(listing(&["odd"])));
        fake.tests.insert(
            "odd".into(),
            Script {
                records: Some("pass\n"),
                code: Some(0),
                ..Script::default()
            },
        );
        let (summary, text) = run_with(&fake, &[exe("C:/p/app.exe")], 1, false, None);
        assert_eq!(summary.failed, 1, "{text}");
        assert!(
            text.contains("---- odd ----\na result record cuno cannot read: `pass`\n"),
            "{text}"
        );
    }

    /// An executable that lists with no file, or cannot be started, is an
    /// error naming it as the report shows it, and is counted; the help of
    /// a list cuno refuses is printed; the totals add up across every
    /// executable, and a run with every test passing ends `ok`.
    ///
    /// Mutation: take a missing list as an empty one (zero tests, success),
    /// drop the help, assign rather than add a count, or print `FAILED` for
    /// a passing run; this test fails.
    #[test]
    fn every_executable_counts() {
        let mut fake = Fake::default();
        for (exe, names) in [("C:/p/one.exe", ["a", "b"]), ("C:/p/two.exe", ["c", "d"])] {
            fake.lists.insert(PathBuf::from(exe), Ok(listing(&names)));
            for name in names {
                fake.tests.insert(name.into(), passing());
            }
        }
        fake.no_list_file.push(PathBuf::from("C:/p/silent.exe"));
        fake.unstartable.push(PathBuf::from("C:/p/gone.exe"));
        fake.lists.insert(
            PathBuf::from("C:/p/clash.exe"),
            Ok(
                "test\tx\tC:/p/src/a.cpp\t1\tmodern\t1\ntest\tx\tC:/p/src/b.cpp\t2\tmodern\t1\n"
                    .into(),
            ),
        );
        let exes = [
            exe("C:/p/one.exe"),
            exe("C:/p/silent.exe"),
            exe("C:/p/gone.exe"),
            exe("C:/p/clash.exe"),
            exe("C:/p/two.exe"),
        ];
        let (summary, text) = run_with(&fake, &exes, 2, false, None);
        assert_eq!(
            summary,
            Summary {
                passed: 4,
                failed: 0,
                skipped: 0,
                unlisted: 3,
            },
            "{text}"
        );
        assert!(
            text.contains("error: silent.exe wrote no list of its tests that cuno can read: "),
            "{text}"
        );
        assert!(
            text.contains("error: cannot run gone.exe: not there\n"),
            "{text}"
        );
        assert!(
            text.contains("error: two tests are named `x`: src/a.cpp:1 and src/b.cpp:2\nhelp: give one of them another name\n"),
            "{text}"
        );
        assert_eq!(
            text.matches("\ntest result: ok. 2 passed; 0 failed; 0 skipped; finished in T")
                .count(),
            2,
            "{text}"
        );
    }

    /// Results go in a directory of the run's own, removed after it: a run
    /// in a scratch directory another run used never reads that run's
    /// results, so a test that reports nothing fails rather than passing on
    /// a result left behind.
    ///
    /// Mutation: write results straight into the scratch directory, or
    /// leave the run's directory behind; this test fails.
    #[test]
    fn a_run_reads_only_its_own_results() {
        let scratch = tempfile::tempdir().unwrap();
        let mut fake = Fake::default();
        fake.lists
            .insert(PathBuf::from("C:/p/app.exe"), Ok(listing(&["t"])));
        fake.tests.insert("t".into(), passing());
        let (first, _) = run_in(
            &fake,
            &[exe("C:/p/app.exe")],
            1,
            false,
            None,
            scratch.path(),
        );
        assert_eq!(first.passed, 1);
        // Results left where a shared scratch directory would hold them.
        std::fs::write(scratch.path().join("result-0-0.txt"), "done\n").unwrap();
        fake.tests.insert(
            "t".into(),
            Script {
                records: None,
                code: Some(0),
                ..Script::default()
            },
        );
        let (second, text) = run_in(
            &fake,
            &[exe("C:/p/app.exe")],
            1,
            false,
            None,
            scratch.path(),
        );
        assert_eq!(second.failed, 1, "{text}");
        let left: Vec<_> = std::fs::read_dir(scratch.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(left, [std::ffi::OsString::from("result-0-0.txt")]);
    }

    /// Output that does not end with a line break is ended with one, so the
    /// report's next line starts on its own; a Windows crash code is shown
    /// in hexadecimal; a test that cannot be started fails saying so; a
    /// skip with no reason reads `skipped`; zero jobs runs one at a time.
    ///
    /// Mutation: print output as it came, show a crash code in decimal,
    /// drop the note of a test that cannot start, print `skipped ()`, or
    /// start no worker for zero jobs; this test fails.
    #[test]
    fn the_report_stays_readable() {
        let mut fake = Fake::default();
        fake.lists.insert(
            PathBuf::from("C:/p/app.exe"),
            Ok(listing(&["mid line", "crash", "quiet skip"])),
        );
        fake.tests.insert(
            "mid line".into(),
            Script {
                records: Some("fail\ta.c\t1\tx failed\ndone\n"),
                code: Some(1),
                stdout: "no newline",
                stderr: "",
                cannot_start: false,
            },
        );
        fake.tests.insert(
            "crash".into(),
            Script {
                records: None,
                code: Some(-1_073_741_819),
                ..Script::default()
            },
        );
        fake.tests.insert(
            "quiet skip".into(),
            Script {
                records: Some("skip\t\n"),
                code: Some(0),
                ..Script::default()
            },
        );
        let (summary, text) = run_with(&fake, &[exe("C:/p/app.exe")], 0, false, None);
        assert_eq!((summary.failed, summary.skipped), (2, 1), "{text}");
        assert!(text.contains("no newline\n---- crash ----\n"), "{text}");
        assert!(
            text.contains("the test ended without reporting a result (exit code 0xC0000005)"),
            "{text}"
        );
        assert!(text.contains("test quiet skip ... skipped\n"), "{text}");

        let mut fake = Fake::default();
        fake.lists
            .insert(PathBuf::from("C:/p/app.exe"), Ok(listing(&["t"])));
        fake.tests.insert(
            "t".into(),
            Script {
                cannot_start: true,
                ..passing()
            },
        );
        let (summary, text) = run_with(&fake, &[exe("C:/p/app.exe")], 1, false, None);
        assert_eq!(summary.failed, 1, "{text}");
        assert!(
            text.contains("---- t ----\ncannot run app.exe: gone\n"),
            "{text}"
        );
    }

    /// The system launcher gives a process exactly the environment it is
    /// handed and the directory it is told, and reports its exit code and
    /// both streams.
    ///
    /// Mutation: let the process inherit cuno's environment, run it
    /// elsewhere, or drop its exit code or stderr; this test fails.
    #[cfg(windows)]
    #[test]
    fn the_system_launcher_runs_as_told() {
        let dir = tempfile::tempdir().unwrap();
        let comspec =
            std::env::var_os("ComSpec").unwrap_or_else(|| "C:\\Windows\\System32\\cmd.exe".into());
        let mut pairs = vec![(OsString::from("CUNO_PROBE"), OsString::from("given"))];
        if let Some(root) = std::env::var_os("SystemRoot") {
            pairs.push((OsString::from("SystemRoot"), root));
        }
        let env = Env::from_pairs(pairs, NameCase::Insensitive);
        let args: Vec<OsString> = [
            "/D",
            "/C",
            "echo %CUNO_PROBE% %PATH%& cd& echo to stderr 1>&2& exit 3",
        ]
        .into_iter()
        .map(OsString::from)
        .collect();
        let out = SystemLauncher
            .launch(Path::new(&comspec), &args, &env, dir.path())
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n");
        let mut lines = stdout.lines();
        assert_eq!(lines.next(), Some("given %PATH%"), "{stdout}");
        let cwd = PathBuf::from(lines.next().unwrap());
        assert!(
            cuno_core::paths::same(&cwd, dir.path(), NameCase::Insensitive),
            "{stdout}"
        );
        assert!(String::from_utf8_lossy(&out.stderr).contains("to stderr"));
        assert_eq!(out.code, Some(3));
    }
}
