//! The build sequence once the toolchain is ready: classify the sources,
//! plan the graph, write compile_commands.json and run the graph against
//! the records. Every build command goes through [`build`]; reading the
//! settings and readying the toolchain stay with the commands, which share
//! them (`docs/en/specs/architecture.md`).

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::Instant;

use cuno_core::{Env, Shell};
use cuno_manifest::{Lang, Manifest, OptLevel, Profile};
use cuno_toolchain::compiler::CompilerKind;
use cuno_toolchain::flags::{Mode, ToolContext, include_dirs};
use cuno_toolchain::{HostOs, Version};

use crate::BuildError;
use crate::compdb;
use crate::exec::{self, Event, Spawner};
use crate::graph::{self, Plan, Products, Step, StepKind};
use crate::input::{compile_settings, editor_settings};
use crate::layout::{self, Layout};
use crate::sources::{self, Bin};
use crate::state::Records;

/// The compiler a build runs, once the toolchain is ready.
pub struct Toolchain<'a> {
    /// Which compiler.
    pub kind: CompilerKind,
    /// Its driver.
    pub executable: &'a Path,
    /// Its version.
    pub version: &'a Version,
    /// The cl version of the MSVC toolset in use, for compile_commands.json.
    pub msvc: Option<&'a Version>,
    /// The environment it runs in.
    pub env: &'a Env,
    /// What besides a step's arguments identifies its command: the
    /// compiler's version and the environment cuno builds for it.
    pub identity: Vec<OsString>,
}

/// What a build is for.
#[derive(Clone, Copy, Debug)]
pub enum Goal<'a> {
    /// `cuno build`: compile and link.
    Build,
    /// `cuno check`: check every source, without objects or links.
    Check,
    /// `cuno run`'s build: compile and link, having chosen the executable
    /// `bin` names, or the only one, before anything runs.
    Run {
        /// `--bin`.
        bin: Option<&'a str>,
    },
    /// `cuno test`'s build: the test executables, every source compiled
    /// with `CUNO_TEST` (`docs/en/specs/testing.md`, "Execution").
    Test,
}

/// One build.
pub struct Request<'a> {
    /// The project's root, where every step runs.
    pub root: &'a Path,
    /// Its `cuno.toml`.
    pub manifest: &'a Manifest,
    /// The output directory.
    pub target_dir: &'a Path,
    /// The profile built.
    pub profile: Profile,
    /// What for.
    pub goal: Goal<'a>,
    /// The system cuno runs on.
    pub host: HostOs,
    /// Where cuno's bundled headers are, on every compile's include path.
    pub bundled: &'a Path,
    /// How many steps run at once.
    pub jobs: usize,
    /// When the command started, for the `Finished` line.
    pub started: Instant,
    /// The compiler.
    pub toolchain: Toolchain<'a>,
    /// What runs the steps.
    pub spawner: &'a dyn Spawner,
}

/// What a build made that its command goes on to use.
#[derive(Debug, Default)]
pub struct Built {
    /// For [`Goal::Run`], the executable chosen.
    pub exe: Option<PathBuf>,
    /// For [`Goal::Test`], every test executable, the in-source tests'
    /// first and then one per source under `tests/` in path order.
    pub test_executables: Vec<BuiltTest>,
}

/// A test executable a build made: where
/// [`crate::sources::TestExecutable`] says what one is made from, this says
/// where it ended up, for the command that runs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuiltTest {
    /// Its name (`docs/en/specs/build.md`, "Output directory").
    pub name: String,
    /// Where it is.
    pub path: PathBuf,
    /// The source under `tests/` it is made from; `None` for the in-source
    /// tests.
    pub file: Option<PathBuf>,
}

/// Builds `request`, printing what a build prints to `shell`
/// (`docs/en/specs/commands.md`, "`cuno build` / `cuno check`").
pub fn build(request: &Request<'_>, shell: &mut Shell) -> Result<Built, BuildError> {
    let Request {
        root,
        manifest,
        target_dir,
        profile,
        goal,
        host,
        bundled,
        ..
    } = *request;
    let tool = &request.toolchain;
    let test = match goal {
        Goal::Build | Goal::Check | Goal::Run { .. } => false,
        Goal::Test => true,
    };
    let compile = |lang: Lang, mode: Mode| {
        compile_settings(
            Some((root, manifest)),
            tool.kind,
            profile,
            lang,
            host,
            bundled,
            test,
            mode,
        )
    };

    let sources = sources::classify(root, manifest, target_dir, host)?;
    for warning in &sources.warnings {
        shell.warning(warning);
    }
    let package = &manifest.package.name;
    let layout = Layout::new(target_dir, profile, root, host);
    let layout = if test { layout.for_tests() } else { layout };
    // Chosen, and named, before anything is built, so a wrong `--bin` or
    // two test executables of one name cost nothing.
    let (mode, exe) = match goal {
        Goal::Build | Goal::Test => (Mode::Compile, None),
        Goal::Check => (Mode::Check, None),
        Goal::Run { bin } => {
            let exe = layout.exe(&choose(&sources.bins, bin)?.name);
            (Mode::Compile, Some(exe))
        }
    };
    let executables = if test {
        sources::test_executables(root, package, &sources, host)?
    } else {
        Vec::new()
    };
    let runtime = bundled.join("cuno").join("test_main.c");
    // A match, so a new goal has to say what it makes.
    let products = match goal {
        Goal::Build | Goal::Check | Goal::Run { .. } => Products::Programs,
        Goal::Test => Products::Tests {
            executables: &executables,
            runtime: &runtime,
        },
    };
    let verb = match mode {
        Mode::Compile => "Compiling",
        Mode::Check => "Checking",
    };
    let link = compile(manifest.package.lang, Mode::Compile);
    let compile_for = |lang| compile(lang, mode);
    let graph = graph::plan(&Plan {
        package,
        sources: &sources,
        products,
        layout: &layout,
        kind: tool.kind,
        program: tool.executable,
        mode,
        compile: &compile_for,
        link: (&link.build, &link.profile),
        color: shell.uses_color(),
    });

    // Written before anything runs, so it is current even when a compile
    // fails, with the same settings whatever is built (`editor_settings`).
    let for_editors =
        |lang| editor_settings(Some((root, manifest)), tool.kind, lang, host, bundled);
    let editors = graph::plan(&Plan {
        package,
        sources: &sources,
        products: Products::CompileCommands,
        // The test build's objects: what compile_commands.json describes is
        // compiled with `CUNO_TEST` (build.md, "Output").
        layout: &Layout::new(target_dir, Profile::Dev, root, host).for_tests(),
        kind: tool.kind,
        program: tool.executable,
        mode: Mode::Compile,
        compile: &for_editors,
        link: (&link.build, &link.profile),
        // compile_commands.json never carries the color flags.
        color: false,
    });
    let include = include_dirs(tool.env.get("INCLUDE"));
    let context = ToolContext {
        kind: tool.kind,
        include: &include,
        msvc: tool.msvc,
    };
    if let Err(e) = compdb::write(root, &editors, context) {
        shell.warning(format!("could not write {}: {e}", compdb::FILE));
    }

    // So that `cuno clean` knows the output directory is cuno's; one it
    // cannot mark is only one `cuno clean` will refuse.
    let _ = layout::claim(target_dir);
    let records = Records::new(
        tool.identity.iter().map(OsString::as_os_str),
        &include,
        host.name_case(),
    );
    // Printed when the first step starts, so a build with nothing to do
    // shows only its Finished line (commands.md, "Output").
    let mut first_status = Some(format!(
        "{package} v{} ({} {})",
        manifest.package.version,
        tool.kind.name(),
        tool.version
    ));
    let result = exec::execute(
        &graph,
        tool.env,
        root,
        request.jobs,
        request.spawner,
        Some(&records),
        &mut |event| match event {
            Event::Started(step) => {
                if let Some(line) = first_status.take() {
                    shell.status(verb, line);
                }
                if shell.verbose() {
                    shell.status("Running", format!("`{}`", command_line(step)));
                }
            }
            Event::Finished(_, output) => {
                shell.raw(&output.stdout);
                shell.raw(&output.stderr);
            }
        },
    );
    if let Err(failed) = result {
        let link = failed
            .0
            .first()
            .is_some_and(|&i| matches!(graph.steps[i].kind, StepKind::Link { .. }));
        return Err(BuildError::Failed {
            package: package.clone(),
            link,
        });
    }

    let settings = manifest.profile_settings(profile);
    let optimized = match settings.opt_level {
        OptLevel::O0 => "unoptimized",
        OptLevel::O1 | OptLevel::O2 | OptLevel::O3 | OptLevel::Size => "optimized",
    };
    let debuginfo = if settings.debug_info {
        " + debuginfo"
    } else {
        ""
    };
    shell.status(
        "Finished",
        format!(
            "`{}` profile [{optimized}{debuginfo}] in {:.2}s",
            profile.as_str(),
            request.started.elapsed().as_secs_f64()
        ),
    );
    let test_executables = executables
        .into_iter()
        .map(|exe| BuiltTest {
            path: layout.test_exe(package, &exe.name),
            name: exe.name,
            file: exe.file.map(|f| f.path),
        })
        .collect();
    Ok(Built {
        exe,
        test_executables,
    })
}

/// The executable `cuno run` runs: the one `bin` names, or the only one.
fn choose<'a>(bins: &'a [Bin], bin: Option<&str>) -> Result<&'a Bin, BuildError> {
    let available = || {
        bins.iter()
            .map(|b| b.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    match (bin, bins) {
        (Some(name), _) => {
            bins.iter()
                .find(|b| b.name == name)
                .ok_or_else(|| BuildError::NoSuchBin {
                    name: name.to_owned(),
                    available: available(),
                })
        }
        (None, [only]) => Ok(only),
        (None, [_, _, ..]) => Err(BuildError::SeveralBins {
            available: available(),
        }),
        // Classifying refuses a package with no executable before this.
        (None, []) => unreachable!("a package to run has an executable"),
    }
}

/// A step's command as `-v` shows it: the program and its arguments,
/// each quoted when it holds a space.
fn command_line(step: &Step) -> String {
    // Quoted as the Windows command line reads it back: an argument with a
    // space, a tab or a quote, or an empty one, in quotes, with each quote
    // escaped and the backslashes before it doubled.
    let quote = |s: &OsStr| {
        let s = s.to_string_lossy();
        if !s.is_empty() && !s.contains([' ', '\t', '"']) {
            return s.into_owned();
        }
        let mut quoted = String::from('"');
        let mut backslashes = 0;
        for c in s.chars() {
            match c {
                '\\' => backslashes += 1,
                '"' => {
                    quoted.push_str(&"\\".repeat(backslashes * 2 + 1));
                    backslashes = 0;
                }
                _ => {
                    quoted.push_str(&"\\".repeat(backslashes));
                    backslashes = 0;
                }
            }
            if c != '\\' {
                quoted.push(c);
            }
        }
        quoted.push_str(&"\\".repeat(backslashes * 2));
        quoted.push('"');
        quoted
    };
    // As it runs: the color flags first (`commands.md`, "Format").
    std::iter::once(quote(step.program.as_os_str()))
        .chain(step.color.iter().chain(&step.args).map(|a| quote(a)))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use cuno_core::Verbosity;

    fn shown(program: &str, args: &[&str]) -> String {
        command_line(&Step {
            kind: StepKind::Link { bin: "app".into() },
            program: program.into(),
            args: args.iter().map(OsString::from).collect(),
            output: PathBuf::new(),
            after: Vec::new(),
            echo: None,
            headers: None,
            reads: Vec::new(),
            searched: Vec::new(),
            looked_for: Vec::new(),
            color: Vec::new(),
        })
    }

    /// `-v` shows each argument as the Windows command line would read it
    /// back: plain when it can be, otherwise quoted with its quotes escaped
    /// and the backslashes before a quote doubled.
    ///
    /// Mutation: quote only on a space, leave inner quotes as they are, or
    /// drop an empty argument; this test fails.
    #[test]
    fn verbose_quotes_as_the_command_line_reads() {
        assert_eq!(shown("cl.exe", &["/c", "a.cpp"]), "cl.exe /c a.cpp");
        assert_eq!(
            shown(r"C:\Program Files\cl.exe", &[r#"/DGREETING="hi there""#]),
            r#""C:\Program Files\cl.exe" "/DGREETING=\"hi there\"""#
        );
        assert_eq!(shown("x", &[r"C:\a b\"]), r#"x "C:\a b\\""#);
        assert_eq!(shown("x", &["a\tb", ""]), "x \"a\tb\" \"\"");
    }

    use std::io::{self, Write};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use cuno_core::NameCase;
    use cuno_core::paths;

    use crate::exec::Output;
    use crate::input::tests::BUNDLED;

    /// Stands in for clang: writes each step's output, and a compile's
    /// dependency file naming its source, and fails when told to. It notes
    /// whether compile_commands.json was there when the first step ran, and
    /// how many steps ran at once.
    #[derive(Default)]
    struct Fake {
        runs: AtomicUsize,
        fail: AtomicBool,
        compdb_first: Mutex<Option<bool>>,
        running: AtomicUsize,
        most: AtomicUsize,
        hold: Duration,
        compiles: Mutex<Vec<Vec<OsString>>>,
    }

    impl Spawner for Fake {
        fn run(&self, step: &Step, _env: &Env, cwd: &Path) -> io::Result<Output> {
            self.runs.fetch_add(1, Ordering::SeqCst);
            if matches!(step.kind, StepKind::Compile { .. }) {
                self.compiles.lock().unwrap().push(step.args.clone());
            }
            self.compdb_first
                .lock()
                .unwrap()
                .get_or_insert_with(|| cwd.join(compdb::FILE).is_file());
            let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
            self.most.fetch_max(now, Ordering::SeqCst);
            std::thread::sleep(self.hold);
            self.running.fetch_sub(1, Ordering::SeqCst);
            std::fs::create_dir_all(step.output.parent().unwrap())?;
            std::fs::write(&step.output, "out")?;
            if let (Some(headers), StepKind::Compile { source }) = (&step.headers, &step.kind) {
                std::fs::write(headers.file(), format!("out: {}\n", source.display()))?;
            }
            Ok(Output {
                success: !self.fail.load(Ordering::SeqCst),
                stdout: Vec::new(),
                stderr: Vec::new(),
            })
        }
    }

    /// What a shell wrote, for reading back.
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

    impl Captured {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    /// A package of `src/main.cpp` and the `sources` named besides it, with
    /// `extra` appended to its `cuno.toml`, dated an hour back so a build
    /// may record it (`build.md`, "Deciding what changed").
    fn package_with(extra: &str, sources: &[&str]) -> (tempfile::TempDir, Manifest) {
        let root = tempfile::tempdir().unwrap();
        for name in std::iter::once("main.cpp").chain(sources.iter().copied()) {
            let source = root.path().join("src").join(name);
            std::fs::create_dir_all(source.parent().unwrap()).unwrap();
            std::fs::write(&source, "int x;\n").unwrap();
            std::fs::File::options()
                .write(true)
                .open(&source)
                .unwrap()
                .set_modified(std::time::SystemTime::now() - Duration::from_secs(3600))
                .unwrap();
        }
        let toml = format!("[package]\nname = \"app\"\nversion = \"0.1.0\"\n{extra}");
        let manifest =
            cuno_manifest::parse_manifest(&toml, &root.path().join("cuno.toml"), root.path())
                .unwrap()
                .value;
        (root, manifest)
    }

    fn package() -> (tempfile::TempDir, Manifest) {
        package_with("", &[])
    }

    /// How a test runs a build.
    struct Options<'a> {
        goal: Goal<'a>,
        profile: Profile,
        jobs: usize,
        verbose: bool,
        quiet: bool,
        /// How long ago the command started.
        ago: Duration,
    }

    impl Default for Options<'_> {
        fn default() -> Self {
            Options {
                goal: Goal::Build,
                profile: Profile::Dev,
                jobs: 1,
                verbose: false,
                quiet: true,
                ago: Duration::ZERO,
            }
        }
    }

    /// Builds the package at `root` with `fake`, returning the result and
    /// what the shell printed, uncolored.
    fn build_with(
        root: &Path,
        manifest: &Manifest,
        fake: &Fake,
        options: Options<'_>,
    ) -> (Result<Built, BuildError>, String) {
        let env = Env::from_pairs([], NameCase::Insensitive);
        let version = Version::find_in("22.1.8").unwrap();
        let target = root.join("target");
        let request = Request {
            root,
            manifest,
            target_dir: &target,
            profile: options.profile,
            goal: options.goal,
            host: HostOs::Windows,
            bundled: Path::new(BUNDLED),
            jobs: options.jobs,
            started: Instant::now() - options.ago,
            toolchain: Toolchain {
                kind: CompilerKind::Clang,
                executable: Path::new("clang.exe"),
                version: &version,
                msvc: None,
                env: &env,
                identity: vec![OsString::from("22.1.8")],
            },
            spawner: fake,
        };
        let out = Captured::default();
        let mut shell = Shell::new(Box::new(out.clone()), None, &env, false);
        shell.set_verbosity(if options.quiet {
            Verbosity::Quiet
        } else if options.verbose {
            Verbosity::Verbose
        } else {
            Verbosity::Normal
        });
        let result = build(&request, &mut shell);
        (result, out.text())
    }

    fn run(
        root: &Path,
        manifest: &Manifest,
        goal: Goal<'_>,
        fake: &Fake,
    ) -> Result<Built, BuildError> {
        let options = Options {
            goal,
            ..Options::default()
        };
        build_with(root, manifest, fake, options).0
    }

    /// compile_commands.json is written before anything runs, so a failed
    /// compile still leaves it current (`build.md`, "Output").
    ///
    /// Mutation: write compile_commands.json after the graph has run,
    /// whether before or after the failure returns; this test fails.
    #[test]
    fn compile_commands_are_written_before_anything_runs() {
        let (root, manifest) = package();
        let fake = Fake::default();
        fake.fail.store(true, Ordering::SeqCst);
        let result = run(root.path(), &manifest, Goal::Build, &fake);
        assert!(
            matches!(result, Err(BuildError::Failed { link: false, .. })),
            "{result:?}"
        );
        assert_eq!(*fake.compdb_first.lock().unwrap(), Some(true));
    }

    /// Every compile, and compile_commands.json, has cuno's bundled headers
    /// on the include path (`build.md`, "Default include paths").
    ///
    /// Mutation: drop `bundled` from either `compile_settings` call in
    /// `build`; this test fails.
    #[test]
    fn the_bundled_headers_are_on_every_include_path() {
        let (root, manifest) = package();
        let fake = Fake::default();
        run(root.path(), &manifest, Goal::Build, &fake).unwrap();
        let flag = OsString::from(format!("-I{BUNDLED}"));
        let compiles = fake.compiles.lock().unwrap();
        assert!(!compiles.is_empty());
        for args in compiles.iter() {
            assert!(args.contains(&flag), "{args:?}");
        }
        let compdb = std::fs::read_to_string(root.path().join(compdb::FILE)).unwrap();
        assert!(compdb.contains(&format!("-I{BUNDLED}")), "{compdb}");
    }

    /// A build runs against the records: what the last one did is not done
    /// again.
    ///
    /// Mutation: execute without the records; this test fails.
    #[test]
    fn a_second_build_with_nothing_changed_runs_nothing() {
        let (root, manifest) = package();
        let fake = Fake::default();
        run(root.path(), &manifest, Goal::Build, &fake).unwrap();
        let first = fake.runs.load(Ordering::SeqCst);
        assert_eq!(first, 2, "a compile and a link");
        run(root.path(), &manifest, Goal::Build, &fake).unwrap();
        assert_eq!(fake.runs.load(Ordering::SeqCst), first);
    }

    /// `--bin` is chosen before anything is written or run, and the build
    /// for `cuno run` names the executable it chose.
    ///
    /// Mutation: choose the executable after building, or return none for a
    /// run; this test fails.
    #[test]
    fn the_executable_to_run_is_chosen_before_anything_runs() {
        let (root, manifest) = package();
        let fake = Fake::default();
        let wrong = run(
            root.path(),
            &manifest,
            Goal::Run { bin: Some("nope") },
            &fake,
        );
        assert!(
            matches!(wrong, Err(BuildError::NoSuchBin { .. })),
            "{wrong:?}"
        );
        assert_eq!(fake.runs.load(Ordering::SeqCst), 0);
        assert!(!root.path().join(compdb::FILE).exists());
        let built = run(root.path(), &manifest, Goal::Run { bin: None }, &fake).unwrap();
        assert_eq!(
            built.exe.as_deref(),
            Some(
                root.path()
                    .join("target")
                    .join("debug")
                    .join("app.exe")
                    .as_path()
            )
        );
    }

    /// Steps run as many at once as `jobs` says, and no more.
    ///
    /// Mutation: run the graph with one job whatever was asked, or ignore
    /// the limit; this test fails.
    #[test]
    fn as_many_steps_run_at_once_as_jobs_says() {
        for jobs in [1, 2] {
            let (root, manifest) = package_with("", &["a.cpp"]);
            let fake = Fake {
                hold: Duration::from_millis(300),
                ..Fake::default()
            };
            let options = Options {
                jobs,
                ..Options::default()
            };
            build_with(root.path(), &manifest, &fake, options)
                .0
                .unwrap();
            assert_eq!(fake.most.load(Ordering::SeqCst), jobs, "jobs {jobs}");
        }
    }

    /// Without `-q`, a build says what it builds with, each command under
    /// `-v`, and how long the whole command took; with `-q` it says none of
    /// it, `-v` or not (`commands.md`, "Output").
    ///
    /// Mutation: drop or change the first status line, show `Running` under
    /// `-q`, or time the Finished line from anything but the command's
    /// start; this test fails.
    #[test]
    fn what_a_build_prints_and_what_quiet_leaves_out() {
        let (root, manifest) = package();
        let options = Options {
            verbose: true,
            quiet: false,
            ago: Duration::from_secs(90),
            ..Options::default()
        };
        let (result, out) = build_with(root.path(), &manifest, &Fake::default(), options);
        result.unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "   Compiling app v0.1.0 (clang 22.1.8)", "{out}");
        assert!(lines[1].starts_with("     Running `clang.exe "), "{out}");
        let finished = lines.last().unwrap();
        assert!(
            finished.starts_with("    Finished `dev` profile [unoptimized + debuginfo] in 90."),
            "{out}"
        );

        let (root, manifest) = package();
        let options = Options {
            verbose: true,
            quiet: true,
            ..Options::default()
        };
        let (result, out) = build_with(root.path(), &manifest, &Fake::default(), options);
        result.unwrap();
        assert_eq!(out, "");
    }

    /// The Finished line calls every optimization level but 0 optimized
    /// (`commands.md`, "Output").
    ///
    /// Mutation: call any of levels 1, 2, 3 or "s" unoptimized, or 0
    /// optimized; this test fails.
    #[test]
    fn the_finished_line_names_each_optimization_level() {
        for (level, label) in [
            ("0", "unoptimized"),
            ("1", "optimized"),
            ("2", "optimized"),
            ("3", "optimized"),
            ("\"s\"", "optimized"),
        ] {
            let (root, manifest) =
                package_with(&format!("\n[profile.dev]\nopt-level = {level}\n"), &[]);
            let options = Options {
                quiet: false,
                ..Options::default()
            };
            let (result, out) = build_with(root.path(), &manifest, &Fake::default(), options);
            result.unwrap();
            assert!(
                out.contains(&format!("profile [{label} + debuginfo] in ")),
                "opt-level {level}: {out}"
            );
        }
    }

    /// Writes `file` under the project at `root`, dated an hour back.
    fn write_old(root: &Path, file: &str) {
        let path = root.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "int y;\n").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - Duration::from_secs(3600))
            .unwrap();
    }

    /// `cuno test`'s build makes the in-source tests' executable and one per
    /// source under `tests/`, in their own place, and says where; it
    /// compiles everything with `CUNO_TEST`, the entry file not at all, and
    /// nothing of the program's (`build.md`, "Output directory").
    ///
    /// Mutation: plan the programs for `Goal::Test`, compile a test source
    /// without `CUNO_TEST` or the body twice, report no executables or
    /// another path, or drop the `tests/` file from what one is made from;
    /// this test fails.
    #[test]
    fn a_test_build_makes_and_names_the_test_executables() {
        let (root, manifest) = package_with("", &["a.cpp"]);
        write_old(root.path(), "tests/t.cpp");
        let fake = Fake::default();
        let built = run(root.path(), &manifest, Goal::Test, &fake).unwrap();
        let bin = paths::lexical(root.path()).join("target/debug/test/app/bin");
        assert_eq!(
            built.test_executables,
            [
                BuiltTest {
                    name: "app".into(),
                    path: bin.join("app.exe"),
                    file: None,
                },
                BuiltTest {
                    name: "tests-t".into(),
                    path: bin.join("tests-t.exe"),
                    file: Some(root.path().join("tests").join("t.cpp")),
                },
            ]
        );
        for test in &built.test_executables {
            assert!(test.path.is_file(), "{}", test.path.display());
        }
        let target = root.path().join("target").join("debug");
        assert!(!target.join("app.exe").exists());
        // Nothing of the program's.
        assert!(!target.join("obj").exists());
        let compiles = fake.compiles.lock().unwrap();
        // The body once, the test file and the runtime.
        assert_eq!(compiles.len(), 3);
        for args in compiles.iter() {
            assert!(args.contains(&OsString::from("-DCUNO_TEST")), "{args:?}");
            assert!(
                !args
                    .iter()
                    .any(|a| a.to_string_lossy().ends_with("main.cpp")),
                "{args:?}"
            );
        }
    }

    /// `cuno test --release` builds the test executables with the release
    /// profile's settings, in the release profile's directory.
    ///
    /// Mutation: lay out or compile a test build in `dev` whatever the
    /// profile; this test fails.
    #[test]
    fn a_release_test_build_is_a_release_build() {
        let (root, manifest) = package_with("", &["a.cpp"]);
        write_old(root.path(), "tests/t.cpp");
        let fake = Fake::default();
        let built = build_with(
            root.path(),
            &manifest,
            &fake,
            Options {
                goal: Goal::Test,
                profile: Profile::Release,
                ..Options::default()
            },
        )
        .0
        .unwrap();
        let release = paths::lexical(root.path()).join("target/release/test/app/bin");
        for test in &built.test_executables {
            assert!(test.path.starts_with(&release), "{}", test.path.display());
        }
        for args in fake.compiles.lock().unwrap().iter() {
            assert!(args.contains(&OsString::from("-O3")), "{args:?}");
        }
    }

    /// compile_commands.json lists the sources under `tests/` and leaves
    /// out cuno's runtime, whatever is built (`build.md`, "Output").
    ///
    /// Mutation: plan the editors' file as the test build, or leave the
    /// sources under `tests/` out of it; this test fails.
    #[test]
    fn compile_commands_list_the_tests_and_not_the_runtime() {
        let (root, manifest) = package();
        write_old(root.path(), "tests/t.cpp");
        for goal in [Goal::Build, Goal::Test] {
            run(root.path(), &manifest, goal, &Fake::default()).unwrap();
            let compdb = std::fs::read_to_string(root.path().join(compdb::FILE)).unwrap();
            assert!(compdb.contains("t.cpp"), "{goal:?}: {compdb}");
            assert!(compdb.contains("main.cpp"), "{goal:?}: {compdb}");
            assert!(!compdb.contains("test_main"), "{goal:?}: {compdb}");
        }
    }

    /// Two test executables of one name stop the build before anything
    /// runs or is written (`build.md`, "Output directory").
    ///
    /// Mutation: name the test executables after the build has run, or not
    /// at all; this test fails.
    #[test]
    fn two_test_executables_of_one_name_stop_the_build_first() {
        let (root, manifest) = package();
        write_old(root.path(), "tests/foo.c");
        write_old(root.path(), "tests/foo.cpp");
        let fake = Fake::default();
        let result = run(root.path(), &manifest, Goal::Test, &fake);
        assert!(
            matches!(result, Err(BuildError::DuplicateTestName { .. })),
            "{result:?}"
        );
        assert_eq!(fake.runs.load(Ordering::SeqCst), 0);
        assert!(!root.path().join(compdb::FILE).exists());
    }
}
