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
use cuno_toolchain::flags::Mode;
use cuno_toolchain::{HostOs, Version};

use crate::BuildError;
use crate::compdb;
use crate::exec::{self, Event, Spawner};
use crate::graph::{self, Plan, Step, StepKind};
use crate::input::compile_settings;
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
    /// How many steps run at once.
    pub jobs: usize,
    /// `-v`: show each step's command.
    pub verbose: bool,
    /// `-q`: show errors, warnings and the compilers' output only.
    pub quiet: bool,
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
        quiet,
        ..
    } = *request;
    let tool = &request.toolchain;
    let compile = |lang: Lang, mode: Mode| {
        compile_settings(
            Some((root, manifest)),
            tool.kind,
            profile,
            lang,
            host,
            false,
            mode,
        )
    };

    let sources = sources::classify(root, manifest, target_dir, host)?;
    for warning in &sources.warnings {
        shell.warning(warning);
    }
    let layout = Layout::new(target_dir, profile, root, host);
    // Chosen before anything is built, so a wrong `--bin` costs nothing.
    let (mode, exe) = match goal {
        Goal::Build => (Mode::Compile, None),
        Goal::Check => (Mode::Check, None),
        Goal::Run { bin } => {
            let exe = layout.exe(&choose(&sources.bins, bin)?.name);
            (Mode::Compile, Some(exe))
        }
    };
    let verb = match mode {
        Mode::Compile => "Compiling",
        Mode::Check => "Checking",
    };
    let link = compile(manifest.package.lang, Mode::Compile);
    let compile_for = |lang| compile(lang, mode);
    let package = &manifest.package.name;
    let graph = graph::plan(&Plan {
        package,
        sources: &sources,
        layout: &layout,
        kind: tool.kind,
        program: tool.executable,
        mode,
        compile: &compile_for,
        link: (&link.build, &link.profile),
        color: shell.uses_color(),
    });

    // Written before anything runs, so it is current even when a compile
    // fails, and always from the `dev` profile with `CUNO_TEST` defined,
    // whatever is built (build.md, "Output").
    let for_editors = |lang| {
        compile_settings(
            Some((root, manifest)),
            tool.kind,
            Profile::Dev,
            lang,
            host,
            true,
            Mode::Compile,
        )
    };
    let editors = graph::plan(&Plan {
        package,
        sources: &sources,
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
    let include = compdb::include_dirs(tool.env.get("INCLUDE"));
    let compiler = compdb::Compiler {
        kind: tool.kind,
        include: &include,
        msvc: tool.msvc,
    };
    if let Err(e) = compdb::write(root, &editors, compiler) {
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
                if let Some(line) = first_status.take()
                    && !quiet
                {
                    shell.status(verb, line);
                }
                if request.verbose && !quiet {
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

    if !quiet {
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
                profile_name(profile),
                request.started.elapsed().as_secs_f64()
            ),
        );
    }
    Ok(Built { exe })
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

/// The profile's name as `cuno.toml` writes it.
fn profile_name(profile: Profile) -> &'static str {
    match profile {
        Profile::Dev => "dev",
        Profile::Release => "release",
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

    use crate::exec::Output;

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
    }

    impl Spawner for Fake {
        fn run(&self, step: &Step, _env: &Env, cwd: &Path) -> io::Result<Output> {
            self.runs.fetch_add(1, Ordering::SeqCst);
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
            profile: Profile::Dev,
            goal: options.goal,
            host: HostOs::Windows,
            jobs: options.jobs,
            verbose: options.verbose,
            quiet: options.quiet,
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
}
