//! The build sequence once the toolchain is ready: classify the sources,
//! plan the graph, write compile_commands.json and run the graph against
//! the records. Every build command goes through [`build`]; reading the
//! settings and readying the toolchain stay with the commands, which share
//! them (`docs/en/specs/architecture.md`).

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::Instant;

use cuno_core::{Env, NameCase, Shell, paths};
use cuno_manifest::{Lang, Manifest, OptLevel, Profile};
use cuno_toolchain::HostOs;
use cuno_toolchain::flags::{Mode, ToolContext};
use cuno_toolchain::prepare::Prepared;
use cuno_toolchain::select::Selected;

use crate::BuildError;
use crate::compdb;
use crate::exec::{self, Event, Spawner};
use crate::graph::{self, Graph, Plan, Products, Purpose, Step, StepKind};
use crate::input::{compile_settings, editor_settings};
use crate::layout::{self, Layout};
use crate::sources::{self, Bin, Sources};
use crate::state::Records;

/// Writes compile_commands.json for the package at `root`, as every build
/// writes it, without building, and returns the sources it holds commands
/// for, each path once, in the plan's order: what `cuno lint` analyses (`docs/en/specs/dev-tools.md`),
/// so lint can never analyse fewer sources than the file describes. The
/// outer error is the package's layout, the inner one the write, which the
/// caller reports as it decides.
pub fn write_compile_commands(
    root: &Path,
    manifest: &Manifest,
    target_dir: &Path,
    host: HostOs,
    tool: &Toolchain<'_>,
    bundled: &Path,
) -> Result<std::io::Result<Vec<PathBuf>>, BuildError> {
    let sources = sources::classify(root, manifest, target_dir, host)?;
    Ok(compile_commands(
        root, manifest, target_dir, host, tool, bundled, &sources,
    ))
}

/// compile_commands.json from sources already classified, with the same
/// settings whatever is built (`editor_settings`); the sources it describes.
fn compile_commands(
    root: &Path,
    manifest: &Manifest,
    target_dir: &Path,
    host: HostOs,
    tool: &Toolchain<'_>,
    bundled: &Path,
    sources: &Sources,
) -> std::io::Result<Vec<PathBuf>> {
    let for_editors = |lang, _: Purpose| {
        editor_settings(
            Some((root, manifest)),
            tool.selected.kind,
            lang,
            host,
            bundled,
        )
    };
    // Never linked; the plan takes link settings all the same.
    let link = for_editors(
        manifest.package.lang,
        Purpose {
            test: true,
            mode: Mode::Compile,
        },
    );
    let editors = graph::plan(&Plan {
        package: &manifest.package.name,
        sources,
        products: Products::CompileCommands,
        // The test build's objects: what compile_commands.json describes is
        // compiled with `CUNO_TEST` (build.md, "Output").
        layout: &Layout::new(target_dir, Profile::Dev, root, host),
        kind: tool.selected.kind,
        program: &tool.selected.executable,
        compile: &for_editors,
        link: (&link.build, &link.profile),
        // compile_commands.json never carries the color flags.
        color: false,
    });
    let context = ToolContext::of(tool.selected, &tool.include);
    compdb::write(root, &editors, context)?;
    let mut described: Vec<PathBuf> = Vec::new();
    for step in &editors.steps {
        // Every kind answers, so a new kind of step that compiles a source
        // cannot be left out of what lint analyses unnoticed.
        let source = match &step.kind {
            StepKind::Compile { source } | StepKind::Check { source } => source,
            StepKind::Link { .. } => continue,
        };
        if !described.contains(source) {
            described.push(source.clone());
        }
    }
    Ok(described)
}

/// The compiler a build runs, once the toolchain is ready.
pub struct Toolchain<'a> {
    /// The compiler: which, its driver, its version and the MSVC toolset
    /// it builds with.
    pub selected: &'a Selected,
    /// The environment it runs in.
    pub env: &'a Env,
    /// The include directories its environment gives
    /// (`cuno_toolchain::prepare::Prepared::include_dirs`), for
    /// compile_commands.json and the build records.
    pub include: Vec<PathBuf>,
    /// What besides a step's arguments identifies its command: the
    /// compiler's version and the environment cuno builds for it.
    pub identity: Vec<OsString>,
}

impl<'a> Toolchain<'a> {
    /// The compiler `selected` names, in the environment `prepared` gives
    /// it: its include directories and identity are taken from that same
    /// environment, so a caller cannot pair one environment with another's
    /// include path.
    pub fn new(selected: &'a Selected, prepared: &'a Prepared) -> Toolchain<'a> {
        let version = selected.version.to_string();
        Toolchain {
            selected,
            env: &prepared.env,
            include: prepared.include_dirs(),
            identity: prepared.identity(&version),
        }
    }
}

/// What a build is for.
#[derive(Clone, Copy, Debug)]
pub enum Goal<'a> {
    /// `cuno build`: compile and link.
    Build,
    /// `cuno check`: check the program's sources, the test build's, or both
    /// (`--tests`, `--all-targets`), without links
    /// (`docs/en/specs/commands.md`, "`cuno build` / `cuno check`").
    Check {
        /// The program's sources: the body and the entry files.
        programs: bool,
        /// The test build's: the body with `CUNO_TEST`, and `tests/`.
        tests: bool,
    },
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
    /// The output directory, normalized as `Settings::target_dir` is.
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
        Goal::Build | Goal::Check { .. } | Goal::Run { .. } => false,
        Goal::Test => true,
    };
    // Whether the test executables are needed: to build them, or to check
    // what they are made of.
    let tests_planned = match goal {
        Goal::Check { tests, .. } => tests,
        Goal::Build | Goal::Run { .. } => false,
        Goal::Test => true,
    };
    let compile = |lang: Lang, purpose: Purpose| {
        compile_settings(
            Some((root, manifest)),
            tool.selected.kind,
            profile,
            lang,
            host,
            bundled,
            purpose.test,
            purpose.mode,
        )
    };

    let sources = sources::classify(root, manifest, target_dir, host)?;
    for warning in &sources.warnings {
        shell.warning(warning);
    }
    let package = &manifest.package.name;
    // The plan takes the program or test variant itself; this one names
    // the executables the build leaves.
    let layout = Layout::new(target_dir, profile, root, host);
    let layout = if test { layout.for_tests() } else { layout };
    // What the compiler's tools could not open, refused before anything
    // compiles (toolchain.md, "MinGW (GCC)"). `lib-dirs` and `lto` are the
    // same for every source.
    let link = compile(
        Lang::Cxx,
        Purpose {
            test,
            mode: Mode::Compile,
        },
    );
    // A check links nothing; a test build links no program.
    let linking = |bins| crate::ascii::Linking {
        lib_dirs: &link.build.lib_dirs,
        lto: link.profile.lto,
        bins,
    };
    let linking = match goal {
        Goal::Check { .. } => None,
        Goal::Build | Goal::Run { .. } => Some(linking(&sources.bins[..])),
        Goal::Test => Some(linking(&[][..])),
    };
    crate::ascii::check(tool.selected.kind, &layout, target_dir, linking, tool.env)?;
    // Chosen, and named, before anything is built, so a wrong `--bin` or
    // two test executables of one name cost nothing.
    let (mode, exe) = match goal {
        Goal::Build | Goal::Test => (Mode::Compile, None),
        Goal::Check { .. } => (Mode::Check, None),
        Goal::Run { bin } => {
            let exe = layout.exe(&choose(&sources.bins, bin, host.name_case())?.name);
            (Mode::Compile, Some(exe))
        }
    };
    let executables = if tests_planned {
        sources::test_executables(root, package, &sources, host)?
    } else {
        Vec::new()
    };
    let runtime = bundled.join("cuno").join("test_main.c");
    let tests_products = Products::Tests {
        executables: &executables,
        runtime: &runtime,
        mode,
    };
    // A match, so a new goal has to say what it makes.
    let products: Vec<Products<'_>> = match goal {
        Goal::Build | Goal::Run { .. } => vec![Products::Programs { mode }],
        Goal::Check { programs, tests } => {
            let mut products = Vec::new();
            if programs {
                products.push(Products::Programs { mode });
            }
            if tests {
                products.push(tests_products);
            }
            products
        }
        Goal::Test => vec![tests_products],
    };
    let verb = match mode {
        Mode::Compile => "Compiling",
        Mode::Check => "Checking",
    };
    let link = compile(
        manifest.package.lang,
        Purpose {
            test,
            mode: Mode::Compile,
        },
    );
    // One graph for all: a check plans no links, so the steps of a second
    // plan wait for none of the first's.
    let mut graph = Graph::default();
    for products in products {
        let planned = graph::plan(&Plan {
            package,
            sources: &sources,
            products,
            layout: &Layout::new(target_dir, profile, root, host),
            kind: tool.selected.kind,
            program: &tool.selected.executable,
            compile: &compile,
            link: (&link.build, &link.profile),
            color: shell.uses_color(),
        });
        let offset = graph.steps.len();
        graph
            .steps
            .extend(planned.steps.into_iter().map(|mut step| {
                for after in &mut step.after {
                    *after += offset;
                }
                step
            }));
    }

    // Written before anything runs, so it is current even when a compile
    // fails.
    let include = &tool.include;
    if let Err(e) = compile_commands(root, manifest, target_dir, host, tool, bundled, &sources) {
        shell.warning(format!("could not write {}: {e}", compdb::FILE));
    }

    // So that `cuno clean` knows the output directory is cuno's; one it
    // cannot mark is only one `cuno clean` will refuse. Normalized again, as
    // the layout normalizes it, so a caller that hands a path as written
    // still marks the directory the layout fills.
    let _ = layout::claim(&paths::lexical(target_dir));
    let records = Records::new(
        tool.identity.iter().map(OsString::as_os_str),
        include,
        host.name_case(),
    );
    // Printed when the first step starts, so a build with nothing to do
    // shows only its Finished line (commands.md, "Output").
    let mut first_status = Some(format!(
        "{package} v{} ({} {})",
        manifest.package.version,
        tool.selected.kind.name(),
        tool.selected.version
    ));
    let mut dated_ahead = std::collections::BTreeSet::new();
    let mut replayed: Vec<Vec<u8>> = Vec::new();
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
                    for saved in replayed.drain(..) {
                        shell.raw(&saved);
                    }
                }
                if shell.verbose() {
                    shell.status("Running", format!("`{}`", command_line(step)));
                }
            }
            Event::Finished(_, output) => {
                shell.raw(&output.stdout);
                shell.raw(&output.stderr);
            }
            // Held until the status line, which comes before the first
            // step that runs, so it shows where a step that ran would
            // (commands.md, "Format").
            Event::Replayed(_, saved) => {
                if first_status.is_some() {
                    replayed.push(saved.to_vec());
                } else {
                    shell.raw(saved);
                }
            }
            // Once a file per build, though every step that reads it is
            // left unrecorded (build.md, "Deciding what changed").
            // A header's path is as its compiler spelled it, so it is shown
            // as the file system names it.
            Event::DatedAhead(path) => {
                let shown = cuno_core::paths::relative(path, root, host.name_case())
                    .unwrap_or_else(|| cuno_core::paths::lexical(path));
                if dated_ahead.insert(shown.clone()) {
                    shell.warning(format!(
                        "{} is dated in the future, so what reads it is built again on every build until then",
                        shown.display()
                    ));
                }
            }
        },
    );
    // A build that ran nothing shows them before its Finished line.
    for saved in replayed {
        shell.raw(&saved);
    }
    if let Err(failed) = result {
        let link = failed
            .0
            .first()
            // A match, so a new kind has to say whether its failure is a
            // link's (`architecture.md`, "Crate layout").
            .is_some_and(|&i| match graph.steps[i].kind {
                StepKind::Link { .. } => true,
                StepKind::Compile { .. } | StepKind::Check { .. } => false,
            });
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
    // Only a test build leaves executables; a check of the tests links none.
    let test_executables = executables
        .into_iter()
        .filter(|_| test)
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

/// The executable `cuno run` runs: the one `bin` names, compared as `case`
/// compares the names classification found unique
/// (`docs/en/specs/commands.md`, "`cuno run`"), or the only one.
fn choose<'a>(bins: &'a [Bin], bin: Option<&str>, case: NameCase) -> Result<&'a Bin, BuildError> {
    let available = || {
        bins.iter()
            .map(|b| b.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    match (bin, bins) {
        (Some(name), _) => bins
            .iter()
            .find(|b| case.same(&b.name, name))
            .ok_or_else(|| BuildError::NoSuchBin {
                name: name.to_owned(),
                available: available(),
            }),
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
        .chain(step.color.iter().chain(step.run_args()).map(|a| quote(a)))
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
            dependency: Vec::new(),
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
    use cuno_toolchain::Version;
    use cuno_toolchain::compiler::CompilerKind;

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

    /// A Clang without a toolset, as the plan's tests build with; leaked,
    /// since a test's `Toolchain` borrows it for the whole test.
    fn clang(version: Version) -> &'static Selected {
        Box::leak(Box::new(Selected {
            kind: CompilerKind::Clang,
            executable: PathBuf::from("clang.exe"),
            version,
            msvc: None,
            source: cuno_toolchain::select::Source::CommandLine,
        }))
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
                selected: clang(version),
                env: &env,
                include: Vec::new(),
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

    /// `--bin` compares names as the host's duplicate check does: without
    /// regard to case on Windows, exactly elsewhere (`commands.md`, "`cuno
    /// run`").
    ///
    /// Mutation: compare with `==`, ignore `case`, or have the build pass
    /// another case than the host's; this test fails.
    #[test]
    fn bin_names_are_compared_as_the_host_compares_them() {
        let (root, manifest) = package();
        let built = run(
            root.path(),
            &manifest,
            Goal::Run { bin: Some("APP") },
            &Fake::default(),
        )
        .unwrap();
        assert!(built.exe.unwrap().ends_with("app.exe"));
        let bins = [Bin {
            name: "app".to_owned(),
            entry: crate::sources::Source {
                path: root.path().join("src/main.cpp"),
                lang: Lang::Cxx,
            },
            subsystem: cuno_toolchain::flags::Subsystem::Console,
        }];
        assert_eq!(
            choose(&bins, Some("APP"), NameCase::Insensitive)
                .unwrap()
                .name,
            "app"
        );
        assert!(matches!(
            choose(&bins, Some("APP"), NameCase::Sensitive),
            Err(BuildError::NoSuchBin { .. })
        ));
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

    /// `write_compile_commands` alone writes the file a build writes, byte for
    /// byte, and a layout the build refuses is its error too.
    ///
    /// Mutation: plan the file with another profile, without the manifest's
    /// settings, or without classifying; this test fails.
    #[test]
    fn compile_commands_are_written_alone_as_a_build_writes_them() {
        let (root, manifest) = package_with(
            "[build]
defines = [\"X=1\"]
",
            &[],
        );
        write_old(root.path(), "tests/t.cpp");
        run(root.path(), &manifest, Goal::Build, &Fake::default()).unwrap();
        let file = root.path().join(compdb::FILE);
        let built = std::fs::read_to_string(&file).unwrap();
        // What both describe: the manifest's settings, and the dev profile's
        // test objects (`build.md`, "Output").
        assert!(built.contains("X=1"), "{built}");
        // A backslash is escaped in JSON.
        assert!(built.contains(r"target\\debug"), "{built}");
        assert!(!built.contains("release"), "{built}");
        std::fs::remove_file(&file).unwrap();
        let env = Env::from_pairs([], NameCase::Insensitive);
        let version = Version::find_in("22.1.8").unwrap();
        let tool = Toolchain {
            selected: clang(version),
            env: &env,
            include: Vec::new(),
            identity: vec![OsString::from("22.1.8")],
        };
        let write = |root: &Path, manifest: &Manifest| {
            write_compile_commands(
                root,
                manifest,
                &root.join("target"),
                HostOs::Windows,
                &tool,
                Path::new(BUNDLED),
            )
        };
        write(root.path(), &manifest).unwrap().unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), built);

        let (empty, manifest) = package_with("", &[]);
        std::fs::remove_dir_all(empty.path().join("src")).unwrap();
        let result = write(empty.path(), &manifest);
        assert!(
            matches!(result, Err(BuildError::NoExecutable { .. })),
            "{result:?}"
        );
    }

    /// The sources `write_compile_commands` returns are exactly those the
    /// file it wrote has entries for: entry files, the body, the tests in
    /// `tests/` and their helpers, so lint analyses no fewer and no more.
    ///
    /// Mutation: return no sources, or leave compiles out of those returned;
    /// this test fails.
    #[test]
    fn the_sources_returned_are_the_files_entries() {
        let (root, manifest) = package_with("", &["util.cpp", "bin/tool.cpp"]);
        write_old(root.path(), "tests/t.cpp");
        write_old(root.path(), "tests/support/help.cpp");
        let env = Env::from_pairs([], NameCase::Insensitive);
        let version = Version::find_in("22.1.8").unwrap();
        let tool = Toolchain {
            selected: clang(version),
            env: &env,
            include: Vec::new(),
            identity: vec![OsString::from("22.1.8")],
        };
        let returned = write_compile_commands(
            root.path(),
            &manifest,
            &root.path().join("target"),
            HostOs::Windows,
            &tool,
            Path::new(BUNDLED),
        )
        .unwrap()
        .unwrap();
        let text = std::fs::read_to_string(root.path().join(compdb::FILE)).unwrap();
        let entries: serde_json::Value = serde_json::from_str(&text).unwrap();
        let mut in_file: Vec<PathBuf> = entries
            .as_array()
            .unwrap()
            .iter()
            .map(|e| PathBuf::from(e["file"].as_str().unwrap()))
            .collect();
        let mut returned = returned;
        in_file.sort();
        returned.sort();
        assert_eq!(returned, in_file);
        let names: Vec<String> = returned
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        for name in ["main.cpp", "util.cpp", "tool.cpp", "t.cpp", "help.cpp"] {
            assert!(names.iter().any(|n| n == name), "{name} in {names:?}");
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
