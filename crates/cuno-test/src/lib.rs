//! Running test executables and reporting their results: each test in a
//! process of its own, up to `jobs` at once, its output captured and shown
//! for failures (or always, with `--nocapture`), the report printed as the
//! tests finish. See `docs/en/specs/testing.md`, "Running" and "Output".
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

use cuno_core::{Env, NameCase, Shell};

use crate::record::{Ending, Report};

/// What a process printed and how it ended.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Finished {
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
    fn launch(&self, exe: &Path, args: &[OsString], env: &Env, cwd: &Path) -> io::Result<Finished>;
}

/// The [`Launcher`] that starts real processes.
pub struct SystemLauncher;

impl Launcher for SystemLauncher {
    fn launch(&self, exe: &Path, args: &[OsString], env: &Env, cwd: &Path) -> io::Result<Finished> {
        let output = std::process::Command::new(exe)
            .args(args)
            .env_clear()
            .envs(env.vars())
            .current_dir(cwd)
            .stdin(std::process::Stdio::null())
            .output()?;
        Ok(Finished {
            code: output.status.code(),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }
}

/// A test executable to run.
#[derive(Clone, Debug)]
pub struct Executable {
    /// Where it is.
    pub path: PathBuf,
    /// Whether it is made from a file under `tests/`, and so runs only the
    /// tests placed there (`testing.md`, "Running").
    pub from_tests: bool,
}

/// One run of a package's tests.
pub struct Run<'a> {
    /// The executables, in the order to run them.
    pub executables: &'a [Executable],
    /// Run only the tests whose names contain this.
    pub filter: Option<&'a str>,
    /// Show every test's output, not only a failed one's.
    pub nocapture: bool,
    /// How many tests run at once; at least one.
    pub jobs: usize,
    /// The project's root: where tests run, what places are shown relative
    /// to, and where `tests/` is.
    pub root: &'a Path,
    /// The environment the tests get, as cuno was given it.
    pub env: &'a Env,
    /// A directory of cuno's for the result files.
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
    /// Whether everything that was to run ran and passed.
    pub fn succeeded(&self) -> bool {
        self.failed == 0 && self.unlisted == 0
    }
}

/// The verdict on one test.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Verdict {
    Passed,
    Skipped(String),
    Failed,
}

/// What became of one test.
struct Outcome {
    name: String,
    finished: Finished,
    report: Report,
    verdict: Verdict,
    /// Why it failed, when its report does not say: it did not finish, or
    /// its file could not be read.
    note: Option<String>,
}

/// Runs every executable's tests, printing what `testing.md`, "Output",
/// shows, and returns how it went.
pub fn run(r: &Run<'_>, shell: &mut Shell) -> Summary {
    let mut summary = Summary::default();
    for (index, exe) in r.executables.iter().enumerate() {
        let shown = exe.path.strip_prefix(r.root).unwrap_or(&exe.path);
        shell.status("Running", format!("`{}`", shown.display()));
        match list(r, index, exe) {
            Ok(names) => run_one(r, index, &names, shell, &mut summary),
            Err((message, help, finished)) => {
                summary.unlisted += 1;
                if let Some(finished) = finished {
                    shell.raw(&finished.stdout);
                    shell.raw(&finished.stderr);
                }
                shell.error(message);
                if let Some(help) = help {
                    shell.help(help);
                }
            }
        }
    }
    summary
}

/// Why an executable's tests could not be listed: the message, a help, and
/// what it printed when that is part of the story.
type Unlisted = (String, Option<&'static str>, Option<Finished>);

/// The names of `exe`'s tests to run.
fn list(r: &Run<'_>, index: usize, exe: &Executable) -> Result<Vec<String>, Unlisted> {
    let file = r.scratch.join(format!("list-{index}.txt"));
    let _ = std::fs::remove_file(&file);
    let env = r
        .env
        .with("CUNO_TEST_RESULT", file.clone().into_os_string());
    let shown = exe.path.display();
    let finished = r
        .launcher
        .launch(&exe.path, &[OsString::from("--list")], &env, r.root)
        .map_err(|e| (format!("cannot run {shown}: {e}"), None, None))?;
    if finished.code != Some(0) {
        let code = finished
            .code
            .map_or("no exit code".to_owned(), |c| format!("exit code {c}"));
        return Err((
            format!("{shown} ended with {code} listing its tests"),
            None,
            Some(finished),
        ));
    }
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    let registrations =
        record::registrations(&text).map_err(|e| (format!("{shown}: {e}"), None, None))?;
    select::tests_to_run(&registrations, exe.from_tests, r.filter, r.root, r.case)
        .map_err(|e| (e.to_string(), Some(e.help()), None))
}

/// Runs `names` from executable `index`, up to `r.jobs` at once, printing
/// each test's line as it finishes, then the failures and the result line.
fn run_one(r: &Run<'_>, index: usize, names: &[String], shell: &mut Shell, summary: &mut Summary) {
    let exe = &r.executables[index];
    let started = Instant::now();
    shell.raw(format!("\nrunning {} tests\n", names.len()).as_bytes());
    let queue: Mutex<VecDeque<(usize, &String)>> = Mutex::new(names.iter().enumerate().collect());
    let (sender, receiver) = mpsc::channel::<Outcome>();
    let mut failures: Vec<Outcome> = Vec::new();
    let (mut passed, mut failed, mut skipped) = (0, 0, 0);
    std::thread::scope(|scope| {
        for _ in 0..r.jobs.max(1).min(names.len().max(1)) {
            let sender = sender.clone();
            let queue = &queue;
            scope.spawn(move || {
                loop {
                    let next = queue.lock().unwrap().pop_front();
                    let Some((at, name)) = next else { break };
                    let _ = sender.send(one_test(r, exe, index, at, name));
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
                shell.raw(&outcome.finished.stdout);
                shell.raw(&outcome.finished.stderr);
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
            if !r.nocapture {
                shell.raw(&outcome.finished.stdout);
                shell.raw(&outcome.finished.stderr);
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

/// Runs test `name`, the `at`th of executable `index`, alone, and judges it
/// by its result file (`testing.md`, "Contract between cuno and test
/// executables").
fn one_test(r: &Run<'_>, exe: &Executable, index: usize, at: usize, name: &str) -> Outcome {
    let file = r.scratch.join(format!("result-{index}-{at}.txt"));
    let _ = std::fs::remove_file(&file);
    let env = r
        .env
        .with("CUNO_TEST_RESULT", file.clone().into_os_string());
    let args = [OsString::from("--exact"), OsString::from(name)];
    let finished = match r.launcher.launch(&exe.path, &args, &env, r.root) {
        Ok(finished) => finished,
        Err(e) => {
            return Outcome {
                name: name.to_owned(),
                finished: Finished::default(),
                report: empty_report(),
                verdict: Verdict::Failed,
                note: Some(format!("cannot run {}: {e}", exe.path.display())),
            };
        }
    };
    let text = std::fs::read_to_string(&file).ok();
    let (report, note) = match record::report(text.as_deref()) {
        Ok(report) => (report, None),
        Err(e) => (empty_report(), Some(e.to_string())),
    };
    let code = finished
        .code
        .map_or("no exit code".to_owned(), |c| format!("exit code {c}"));
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
        (_, true, None) if finished.code != Some(0) => (
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
        finished,
        report,
        verdict,
        note,
    }
}

fn empty_report() -> Report {
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
    }

    /// A launcher that plays scripts instead of starting processes.
    #[derive(Default)]
    struct Fake {
        /// Per executable path: its `--list` records, or `Err` for an exit
        /// code to end listing with.
        lists: HashMap<PathBuf, Result<String, i32>>,
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
        ) -> io::Result<Finished> {
            let result = PathBuf::from(env.get("CUNO_TEST_RESULT").unwrap());
            if args[0] == "--list" {
                return match &self.lists[exe] {
                    Ok(records) => {
                        std::fs::write(result, records)?;
                        Ok(Finished {
                            code: Some(0),
                            ..Finished::default()
                        })
                    }
                    Err(code) => Ok(Finished {
                        code: Some(*code),
                        stderr: b"static init failed\n".to_vec(),
                        ..Finished::default()
                    }),
                };
            }
            let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
            self.most.fetch_max(now, Ordering::SeqCst);
            std::thread::sleep(self.hold);
            self.running.fetch_sub(1, Ordering::SeqCst);
            let script = self.tests[args[1].to_str().unwrap()].clone();
            if let Some(records) = script.records {
                std::fs::write(result, records)?;
            }
            Ok(Finished {
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
            .map(|(i, n)| format!("test\t{n}\tC:/p/src/a.cpp\t{}\tmodern\n", i + 1))
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
        exes: &[Executable],
        jobs: usize,
        nocapture: bool,
        filter: Option<&str>,
    ) -> (Summary, String) {
        let scratch = tempfile::tempdir().unwrap();
        let env = Env::from_pairs([], NameCase::Insensitive);
        let out = Captured::default();
        let mut shell = Shell::new(Box::new(out.clone()), None, &env, false);
        let summary = run(
            &Run {
                executables: exes,
                filter,
                nocapture,
                jobs,
                root: Path::new(ROOT),
                env: &env,
                scratch: scratch.path(),
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

    fn exe(path: &str) -> Executable {
        Executable {
            path: PathBuf::from(path),
            from_tests: false,
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
            Ok("test\tin source\tC:/p/src/a.cpp\t1\tmodern\n\
                test\tadds one\tC:/p/tests/a.cpp\t2\tmodern\n\
                test\tadds two\tC:/p/tests/a.cpp\t3\tmodern\n"
                .into()),
        );
        fake.tests.insert("adds one".into(), passing());
        fake.tests.insert("adds two".into(), passing());
        let exes = [
            exe("C:/p/broken.exe"),
            Executable {
                path: PathBuf::from("C:/p/tests-a.exe"),
                from_tests: true,
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
            text.contains("static init failed\nerror: C:/p/broken.exe ended with exit code 3 listing its tests\n"),
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
}
