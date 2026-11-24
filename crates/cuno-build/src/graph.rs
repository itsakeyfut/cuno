//! The build graph: every compile and link a build runs, with its command
//! line and what it must wait for, built without running anything
//! (`docs/en/specs/architecture.md`, "cuno-build"), so `compile_commands.json`
//! and a later backend can be made from it too.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use cuno_toolchain::compiler::{CompilerKind, Dependencies};
use cuno_toolchain::flags::{
    BuildSettings, Lang, LinkInput, Mode, ProfileSettings, Subsystem, color_args, compile_args,
    dependency_args, library_file, link_args,
};

use crate::deps::Headers;

use crate::input::CompileSettings;
use crate::layout::Layout;
use crate::sources::{Source, Sources, TestExecutable};

/// What a step does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepKind {
    /// Compiles one source.
    Compile {
        /// The source.
        source: PathBuf,
    },
    /// Checks one source without compiling it (`cuno check`); its output
    /// is an empty stamp, written when the check succeeds.
    Check {
        /// The source.
        source: PathBuf,
    },
    /// Links one executable.
    Link {
        /// The executable's name.
        bin: String,
    },
}

/// One process the build runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    /// What it does.
    pub kind: StepKind,
    /// The program run (the compiler driver, for links too).
    pub program: PathBuf,
    /// Its arguments.
    pub args: Vec<OsString>,
    /// The file it writes.
    pub output: PathBuf,
    /// The steps that must succeed first, by index.
    pub after: Vec<usize>,
    /// The line cl.exe prints first on every compile, the source's file
    /// name, which is not shown (`commands.md`, "Output").
    pub echo: Option<String>,
    /// Where a compile's headers are found once it has run; `None` for a
    /// link.
    pub headers: Option<Headers>,
    /// The files it reads besides the outputs of the steps it waits for
    /// and its headers: a compile's source, and the libraries of `links`
    /// a link finds in `lib-dirs` (`build.md`, "Deciding what changed").
    pub reads: Vec<PathBuf>,
    /// The directories cuno tells the tool to search, where a user's file
    /// can appear: a compile's include directories (cuno's own, then
    /// `include-dirs`), a link's `lib-dirs` (`build.md`, "Deciding what
    /// changed").
    pub searched: Vec<PathBuf>,
    /// The names a link looks for in `searched`: `<lib>.lib` for every
    /// entry of `links`, found there or not. Empty for a compile, which
    /// looks for the files it read.
    pub looked_for: Vec<PathBuf>,
    /// Arguments put before `args` only when the step runs, to make the
    /// compiler color its diagnostics: part of neither the command a step
    /// is judged by nor `compile_commands.json` (`commands.md`, "Format").
    pub color: Vec<OsString>,
}

/// Every step of a build, in an order where each comes after those it
/// waits for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Graph {
    /// The steps.
    pub steps: Vec<Step>,
}

/// The settings a source of each language compiles with.
pub type SettingsFor<'a> = &'a dyn Fn(Lang) -> CompileSettings;

/// What a plan makes.
#[derive(Clone, Copy)]
pub enum Products<'a> {
    /// The package's executables: each entry file and the body, one link
    /// per executable (`project-layout.md`, "Link rules").
    Programs,
    /// The test executables (`testing.md`, "Execution"): the body, the
    /// sources under `tests/` and cuno's bundled runtime, all compiled with
    /// `CUNO_TEST` and once each, entry files left out, one link per
    /// executable (`project-layout.md`, "Link rules"). Only with a test
    /// layout and [`Mode::Compile`].
    Tests {
        /// The executables, as [`crate::sources::test_executables`] gives.
        executables: &'a [TestExecutable],
        /// The runtime's source, `cuno/test_main.c` in the cuno home.
        runtime: &'a Path,
    },
    /// What `compile_commands.json` describes: the test build's sources
    /// but for the runtime, and the entry files, compiled and never linked
    /// (`build.md`, "Output"). Only with a test layout and
    /// [`Mode::Compile`].
    CompileCommands,
}

/// What a build is planned from.
pub struct Plan<'a> {
    /// The package's name, which names its object directory.
    pub package: &'a str,
    /// Its classified sources.
    pub sources: &'a Sources,
    /// What is made from them.
    pub products: Products<'a>,
    /// Where outputs go.
    pub layout: &'a Layout,
    /// The compiler.
    pub kind: CompilerKind,
    /// Its driver, which compiles and links.
    pub program: &'a Path,
    /// Whether to build, or to check without objects or links. The
    /// settings `compile` gives must be for the same mode.
    pub mode: Mode,
    /// The settings a source of each language compiles with.
    pub compile: SettingsFor<'a>,
    /// The settings every link uses.
    pub link: (&'a BuildSettings, &'a ProfileSettings),
    /// Whether cuno colors its output, and so asks Clang to color its
    /// diagnostics.
    pub color: bool,
}

/// Plans a build of `p.products`. For the programs: one compile per source
/// of the package body and per entry file, each once, and one link per
/// executable of its entry file's object and every body object
/// (`project-layout.md`, "Link rules"); in [`Mode::Check`], one check per
/// source instead, each with its stamp where the object would be, and no
/// links (`commands.md`, "`cuno build` / `cuno check`"). For the tests and
/// compile_commands.json, as [`Products`] says.
pub fn plan(p: &Plan<'_>) -> Graph {
    let is_test_plan = match p.products {
        Products::Programs => false,
        Products::Tests { .. } | Products::CompileCommands => true,
    };
    // A test plan on a program's layout, or a check of one, is a bug in
    // cuno: its objects would mix with the program's, or nothing would link.
    assert_eq!(
        is_test_plan,
        p.layout.is_test(),
        "products for another layout"
    );
    assert!(
        !is_test_plan || p.mode == Mode::Compile,
        "a test plan in another mode than compile"
    );
    let mut steps = Vec::new();
    let mut compile = |source: &Source, object: Option<PathBuf>| -> usize {
        let settings = (p.compile)(source.lang);
        // A disagreement is a bug in cuno: a check planned with compile
        // flags would write objects and stamp nothing it checked.
        assert_eq!(settings.mode, p.mode, "compile settings for another mode");
        // So is a test layout with settings that leave out `CUNO_TEST`: its
        // objects would hold no tests, and a test run would find none.
        assert_eq!(
            settings.test,
            p.layout.is_test(),
            "compile settings for another build than the layout's"
        );
        let (kind, output) = match p.mode {
            Mode::Compile => (
                StepKind::Compile {
                    source: source.path.clone(),
                },
                object.unwrap_or_else(|| p.layout.object(p.package, &source.path)),
            ),
            Mode::Check => (
                StepKind::Check {
                    source: source.path.clone(),
                },
                p.layout.check_stamp(p.package, &source.path),
            ),
        };
        let echo = p
            .kind
            .echoes_source_name()
            .then(|| source.path.file_name())
            .flatten()
            .map(|name| name.to_string_lossy().into_owned());
        let headers = match p.kind.dependencies() {
            Dependencies::SourceDependencies => Headers::Json(output.with_extension("json")),
            Dependencies::DepFile => Headers::DepFile(output.with_extension("d")),
        };
        let mut args = compile_args(&settings.input(), &source.path, Some(&output));
        args.extend(dependency_args(p.kind, headers.file()));
        steps.push(Step {
            kind,
            program: p.program.to_owned(),
            args,
            output,
            after: Vec::new(),
            echo,
            headers: Some(headers),
            reads: vec![source.path.clone()],
            searched: settings
                .include_dirs
                .iter()
                .chain(&settings.build.include_dirs)
                .cloned()
                .collect(),
            looked_for: Vec::new(),
            color: color_args(p.kind, p.color),
        });
        steps.len() - 1
    };
    let body: Vec<usize> = p.sources.body.iter().map(|s| compile(s, None)).collect();
    // What is linked: the executable's name, its subsystem, its path and
    // the steps whose objects it takes, in link order.
    let mut links: Vec<(String, Subsystem, PathBuf, Vec<usize>)> = Vec::new();
    match p.products {
        Products::Programs => {
            let entries: Vec<usize> = p
                .sources
                .bins
                .iter()
                .map(|b| compile(&b.entry, None))
                .collect();
            // A match rather than an `if`, so a new mode has to say whether
            // it links.
            match p.mode {
                Mode::Compile => {}
                Mode::Check => return Graph { steps },
            }
            for (bin, entry) in p.sources.bins.iter().zip(entries) {
                let after = std::iter::once(entry).chain(body.iter().copied()).collect();
                links.push((
                    bin.name.clone(),
                    bin.subsystem,
                    p.layout.exe(&bin.name),
                    after,
                ));
            }
        }
        Products::CompileCommands => {
            for bin in &p.sources.bins {
                compile(&bin.entry, None);
            }
            for source in p
                .sources
                .integration_tests
                .iter()
                .chain(&p.sources.test_helpers)
            {
                compile(source, None);
            }
            return Graph { steps };
        }
        Products::Tests {
            executables,
            runtime,
        } => {
            let files: Vec<usize> = p
                .sources
                .integration_tests
                .iter()
                .map(|s| compile(s, None))
                .collect();
            let helpers: Vec<usize> = p
                .sources
                .test_helpers
                .iter()
                .map(|s| compile(s, None))
                .collect();
            let name = runtime
                .file_name()
                .expect("the runtime's source is a file")
                .to_string_lossy()
                .into_owned();
            let runtime = compile(
                &Source {
                    path: runtime.to_owned(),
                    lang: Lang::C,
                },
                Some(p.layout.runtime_object(p.package, &name)),
            );
            for exe in executables {
                // In-source tests: the body and the runtime. A file under
                // `tests/`: that file, the helpers, the body and the runtime
                // (`project-layout.md`, "Link rules").
                let own: Vec<usize> = match &exe.file {
                    None => Vec::new(),
                    Some(file) => {
                        let at = p
                            .sources
                            .integration_tests
                            .iter()
                            .position(|t| t == file)
                            .expect("a test executable's file is one of the integration tests");
                        std::iter::once(files[at])
                            .chain(helpers.iter().copied())
                            .collect()
                    }
                };
                let after = own
                    .into_iter()
                    .chain(body.iter().copied())
                    .chain([runtime])
                    .collect();
                links.push((
                    exe.name.clone(),
                    Subsystem::Console,
                    p.layout.test_exe(p.package, &exe.name),
                    after,
                ));
            }
        }
    }

    for (name, subsystem, exe, after) in links {
        let objects: Vec<PathBuf> = after.iter().map(|&i| steps[i].output.clone()).collect();
        let input = LinkInput {
            kind: p.kind,
            build: p.link.0,
            profile: p.link.1,
            subsystem,
        };
        steps.push(Step {
            kind: StepKind::Link { bin: name },
            program: p.program.to_owned(),
            args: link_args(&input, &objects, &exe),
            output: exe,
            after,
            echo: None,
            headers: None,
            reads: libraries(p.kind, p.link.0),
            searched: p.link.0.lib_dirs.clone(),
            looked_for: p
                .link
                .0
                .links
                .iter()
                .map(|lib| PathBuf::from(library_file(p.kind, lib)))
                .collect(),
            color: Vec::new(),
        });
    }
    Graph { steps }
}

/// The libraries of `links` found in `lib-dirs`: for each, the first
/// [`library_file`] searching `lib-dirs` in order, as the linker searches
/// them. A library found nowhere there is a system one, found through
/// `LIB`.
fn libraries(kind: CompilerKind, build: &BuildSettings) -> Vec<PathBuf> {
    build
        .links
        .iter()
        .filter_map(|lib| {
            build
                .lib_dirs
                .iter()
                .map(|dir| dir.join(library_file(kind, lib)))
                .find(|path| path.is_file())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::compile_settings;
    use crate::input::tests::BUNDLED;
    use crate::sources::Bin;
    use cuno_manifest::Profile;
    use cuno_toolchain::HostOs;
    use cuno_toolchain::compiler::Dependencies;
    use cuno_toolchain::flags::{Mode, Subsystem};

    fn source(path: &str, lang: Lang) -> Source {
        Source {
            path: PathBuf::from(path),
            lang,
        }
    }

    fn sources() -> Sources {
        Sources {
            bins: vec![
                Bin {
                    name: "app".into(),
                    entry: source("C:/p/src/main.cpp", Lang::Cxx),
                    subsystem: Subsystem::Console,
                },
                Bin {
                    name: "tool".into(),
                    entry: source("C:/p/src/bin/tool.c", Lang::C),
                    subsystem: Subsystem::Windows,
                },
            ],
            body: vec![
                source("C:/p/src/a.cpp", Lang::Cxx),
                source("C:/p/src/b.c", Lang::C),
            ],
            integration_tests: Vec::new(),
            test_helpers: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn graph(kind: CompilerKind) -> Graph {
        planned(kind, Mode::Compile)
    }

    fn planned(kind: CompilerKind, mode: Mode) -> Graph {
        let sources = sources();
        let layout = Layout::new(
            Path::new("C:/p/target"),
            Profile::Dev,
            Path::new("C:/p"),
            HostOs::Windows,
        );
        let compile = |lang| {
            compile_settings(
                None,
                kind,
                Profile::Dev,
                lang,
                HostOs::Windows,
                Path::new(BUNDLED),
                false,
                mode,
            )
        };
        let link = compile(Lang::Cxx);
        plan(&Plan {
            package: "app",
            sources: &sources,
            products: Products::Programs,
            layout: &layout,
            kind,
            program: Path::new("C:/cl.exe"),
            mode,
            compile: &compile,
            link: (&link.build, &link.profile),
            color: false,
        })
    }

    fn args(step: &Step) -> Vec<String> {
        step.args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    /// Each source compiles once, in its own language, to its own object; each
    /// executable links its own entry's object and every body object, after
    /// those compiles, with its own subsystem.
    ///
    /// Mutation: compile a body source per executable, link a bin without the
    /// body or with another bin's entry, compile in the package's language, or
    /// order a link before its compiles; this test fails.
    #[test]
    fn each_source_compiles_once_and_each_bin_links_its_own() {
        let g = graph(CompilerKind::Msvc);
        let kinds: Vec<&StepKind> = g.steps.iter().map(|s| &s.kind).collect();
        assert_eq!(
            kinds,
            [
                &StepKind::Compile {
                    source: "C:/p/src/a.cpp".into()
                },
                &StepKind::Compile {
                    source: "C:/p/src/b.c".into()
                },
                &StepKind::Compile {
                    source: "C:/p/src/main.cpp".into()
                },
                &StepKind::Compile {
                    source: "C:/p/src/bin/tool.c".into()
                },
                &StepKind::Link { bin: "app".into() },
                &StepKind::Link { bin: "tool".into() },
            ]
        );
        assert!(args(&g.steps[1]).contains(&"/TC".to_owned()));
        assert!(args(&g.steps[0]).contains(&"/TP".to_owned()));
        assert_eq!(
            g.steps[0].output,
            Path::new("C:/p/target/debug/obj/app/src/a.cpp.obj")
        );

        let (app, tool) = (&g.steps[4], &g.steps[5]);
        assert_eq!(app.after, [2, 0, 1]);
        assert_eq!(tool.after, [3, 0, 1]);
        for (link, own, other) in [(app, 2, 3), (tool, 3, 2)] {
            let a = args(link);
            for i in [own, 0, 1] {
                assert!(
                    a.contains(&g.steps[i].output.display().to_string()),
                    "{a:?}"
                );
            }
            assert!(
                !a.contains(&g.steps[other].output.display().to_string()),
                "{a:?}"
            );
        }
        assert_eq!(app.output, Path::new("C:/p/target/debug/app.exe"));
        assert!(args(app).contains(&"/SUBSYSTEM:CONSOLE".to_owned()));
        assert!(args(tool).contains(&"/SUBSYSTEM:WINDOWS".to_owned()));
        for (i, step) in g.steps.iter().enumerate() {
            assert!(step.after.iter().all(|&a| a < i), "{i}");
        }
    }

    /// Only cl.exe echoes the source's file name, so only its compiles say
    /// which line to drop.
    ///
    /// Mutation: drop a line for clang or clang-cl, give links an echo, or
    /// name the path instead of the file; this test fails.
    #[test]
    fn only_cl_compiles_have_an_echo() {
        let msvc = graph(CompilerKind::Msvc);
        assert_eq!(msvc.steps[0].echo.as_deref(), Some("a.cpp"));
        assert_eq!(msvc.steps[4].echo, None);
        for kind in [CompilerKind::Clang, CompilerKind::ClangCl] {
            assert!(
                graph(kind).steps.iter().all(|s| s.echo.is_none()),
                "{kind:?}"
            );
        }
    }

    /// Each compile writes its headers to its own file beside its object,
    /// named by the dependency arguments that come after everything else, and
    /// reads its source; a link has no header file.
    ///
    /// Mutation: share one dependency file between compiles, put the
    /// dependency arguments before the user's flags, give a link a header
    /// file, or leave the source out of what a compile reads; this test fails.
    #[test]
    fn each_compile_names_its_own_header_file() {
        for kind in [
            CompilerKind::Msvc,
            CompilerKind::Clang,
            CompilerKind::ClangCl,
        ] {
            let g = graph(kind);
            let mut files = Vec::new();
            for step in &g.steps[..4] {
                let headers = step.headers.as_ref().unwrap();
                assert_eq!(headers.file().parent(), step.output.parent(), "{kind:?}");
                let tail = dependency_args(kind, headers.file());
                assert!(step.args.ends_with(&tail), "{kind:?}: {:?}", step.args);
                let StepKind::Compile { source } = &step.kind else {
                    panic!("{kind:?}");
                };
                assert_eq!(step.reads, std::slice::from_ref(source));
                files.push(headers.file().to_owned());
            }
            files.sort();
            files.dedup();
            assert_eq!(files.len(), 4, "{kind:?}");
            let expected = match kind.dependencies() {
                Dependencies::SourceDependencies => "json",
                Dependencies::DepFile => "d",
            };
            assert_eq!(files[0].extension().unwrap(), expected, "{kind:?}");
            assert!(g.steps[4..].iter().all(|s| s.headers.is_none()), "{kind:?}");
        }
    }

    /// A link reads the libraries of `links` found in `lib-dirs`, the first
    /// directory that has one winning; a library found in none is left to
    /// `LIB`.
    ///
    /// Mutation: read every directory's copy, the last one, or a library found
    /// nowhere; this test fails.
    #[test]
    fn a_link_reads_the_libraries_found_in_lib_dirs() {
        let root = tempfile::tempdir().unwrap();
        let (first, second) = (root.path().join("one"), root.path().join("two"));
        for dir in [&first, &second] {
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(dir.join("foo.lib"), "").unwrap();
        }
        std::fs::write(second.join("bar.lib"), "").unwrap();
        let mut build = compile_settings(
            None,
            CompilerKind::Clang,
            Profile::Dev,
            Lang::Cxx,
            HostOs::Windows,
            Path::new(BUNDLED),
            false,
            Mode::Compile,
        )
        .build;
        build.links = vec!["foo".into(), "bar".into(), "kernel32".into()];
        build.lib_dirs = vec![first.clone(), second.clone()];
        assert_eq!(
            libraries(CompilerKind::Msvc, &build),
            [first.join("foo.lib"), second.join("bar.lib")]
        );
    }

    /// A compile searches cuno's own include directories, then
    /// `include-dirs`; a link searches `lib-dirs` for every library of
    /// `links`, found there or not (`build.md`, "Deciding what changed").
    ///
    /// Mutation: leave cuno's own directories or `include-dirs` out of a
    /// compile's search, give a link no `lib-dirs`, or look only for the
    /// libraries found; this test fails.
    #[test]
    fn a_step_says_where_its_tool_searches() {
        let sources = sources();
        let layout = Layout::new(
            Path::new("C:/p/target"),
            Profile::Dev,
            Path::new("C:/p"),
            HostOs::Windows,
        );
        let kind = CompilerKind::Msvc;
        let compile = |lang| {
            let mut settings = compile_settings(
                None,
                kind,
                Profile::Dev,
                lang,
                HostOs::Windows,
                Path::new(BUNDLED),
                false,
                Mode::Compile,
            );
            settings.include_dirs = vec!["C:/p/src".into()];
            settings.build.include_dirs = vec!["C:/p/hdr".into()];
            settings
        };
        let mut link = compile(Lang::Cxx);
        link.build.lib_dirs = vec!["C:/p/lib".into()];
        link.build.links = vec!["kernel32".into()];
        let g = plan(&Plan {
            package: "app",
            sources: &sources,
            products: Products::Programs,
            layout: &layout,
            kind,
            program: Path::new("C:/cl.exe"),
            mode: Mode::Compile,
            compile: &compile,
            link: (&link.build, &link.profile),
            color: false,
        });
        let (first, last) = (&g.steps[0], g.steps.last().unwrap());
        assert_eq!(
            first.searched,
            [PathBuf::from("C:/p/src"), PathBuf::from("C:/p/hdr")]
        );
        assert!(first.looked_for.is_empty());
        assert_eq!(last.searched, [PathBuf::from("C:/p/lib")]);
        assert_eq!(last.looked_for, [PathBuf::from("kernel32.lib")]);
    }

    /// With color, a compile by clang or clang-cl gets the color flags apart
    /// from its arguments, which stay as without color; MSVC and links get
    /// none (`commands.md`, "Format").
    ///
    /// Mutation: give MSVC or a link the flags, put them among the
    /// arguments, or give them without color; this test fails.
    #[test]
    fn only_clangs_compiles_are_asked_for_color() {
        let colored = |kind, color| {
            let sources = sources();
            let layout = Layout::new(
                Path::new("C:/p/target"),
                Profile::Dev,
                Path::new("C:/p"),
                HostOs::Windows,
            );
            let compile = |lang| {
                compile_settings(
                    None,
                    kind,
                    Profile::Dev,
                    lang,
                    HostOs::Windows,
                    Path::new(BUNDLED),
                    false,
                    Mode::Compile,
                )
            };
            let link = compile(Lang::Cxx);
            plan(&Plan {
                package: "app",
                sources: &sources,
                products: Products::Programs,
                layout: &layout,
                kind,
                program: Path::new("C:/clang.exe"),
                mode: Mode::Compile,
                compile: &compile,
                link: (&link.build, &link.profile),
                color,
            })
        };
        let flags = ["-fcolor-diagnostics", "-fansi-escape-codes"].map(OsString::from);
        for kind in [CompilerKind::Clang, CompilerKind::ClangCl] {
            let (on, off) = (colored(kind, true), colored(kind, false));
            assert_eq!(on.steps[0].color, flags, "{kind:?}");
            assert_eq!(on.steps[0].args, off.steps[0].args, "{kind:?}");
            assert!(off.steps[0].color.is_empty(), "{kind:?}");
            assert!(on.steps.last().unwrap().color.is_empty(), "{kind:?}");
        }
        assert!(colored(CompilerKind::Msvc, true).steps[0].color.is_empty());
    }

    /// In check mode every source is checked, with the check switch and no
    /// object, each leaving its stamp beside where its object would be, and
    /// nothing is linked.
    ///
    /// Mutation: keep the compile switch or the object, give the checks the
    /// objects' paths, or keep the links; this test fails.
    #[test]
    fn a_check_checks_every_source_and_links_nothing() {
        for (kind, switch, object) in [
            (CompilerKind::Msvc, "/Zs", "/Fo"),
            (CompilerKind::ClangCl, "/Zs", "/Fo"),
            (CompilerKind::Clang, "-fsyntax-only", "-o"),
        ] {
            let g = planned(kind, Mode::Check);
            assert_eq!(g.steps.len(), 4, "{kind:?}");
            for step in &g.steps {
                let StepKind::Check { source } = &step.kind else {
                    panic!("{kind:?}: {:?}", step.kind);
                };
                let a = args(step);
                assert_eq!(a[0], switch, "{kind:?}");
                assert!(!a.iter().any(|x| x.starts_with(object)), "{kind:?}: {a:?}");
                let name = source.file_name().unwrap().to_string_lossy();
                assert!(
                    step.output.ends_with(format!("{name}.check")),
                    "{}",
                    step.output.display()
                );
            }
            assert!(
                g.steps[0]
                    .output
                    .starts_with("C:/p/target/debug/obj/app/src"),
                "{}",
                g.steps[0].output.display()
            );
        }
    }

    /// A test layout planned with settings that leave out `CUNO_TEST`, or
    /// the reverse, is a bug in cuno and stops the plan.
    ///
    /// Mutation: plan without comparing the settings' test build with the
    /// layout's; this test fails.
    #[test]
    #[should_panic(expected = "compile settings for another build than the layout's")]
    fn a_test_layout_needs_test_settings() {
        let sources = sources();
        let layout = Layout::new(
            Path::new("C:/p/target"),
            Profile::Dev,
            Path::new("C:/p"),
            HostOs::Windows,
        )
        .for_tests();
        let kind = CompilerKind::Clang;
        let compile = |lang| {
            compile_settings(
                None,
                kind,
                Profile::Dev,
                lang,
                HostOs::Windows,
                Path::new(BUNDLED),
                false,
                Mode::Compile,
            )
        };
        let link = compile(Lang::Cxx);
        plan(&Plan {
            package: "app",
            sources: &sources,
            products: Products::CompileCommands,
            layout: &layout,
            kind,
            program: Path::new("C:/clang.exe"),
            mode: Mode::Compile,
            compile: &compile,
            link: (&link.build, &link.profile),
            color: false,
        });
    }

    /// A package with an executable, a body source, a test file and a
    /// helper under `tests/`.
    fn test_sources() -> Sources {
        Sources {
            bins: vec![Bin {
                name: "app".into(),
                entry: source("C:/p/src/main.cpp", Lang::Cxx),
                subsystem: Subsystem::Console,
            }],
            body: vec![source("C:/p/src/a.cpp", Lang::Cxx)],
            integration_tests: vec![source("C:/p/tests/t.cpp", Lang::Cxx)],
            test_helpers: vec![source("C:/p/tests/common/h.c", Lang::C)],
            warnings: Vec::new(),
        }
    }

    const RUNTIME: &str = "C:/home/include/0.0.0/cuno/test_main.c";

    fn test_plan(products: Products<'_>, sources: &Sources) -> Graph {
        let layout = Layout::new(
            Path::new("C:/p/target"),
            Profile::Dev,
            Path::new("C:/p"),
            HostOs::Windows,
        )
        .for_tests();
        let kind = CompilerKind::Clang;
        let compile = |lang| {
            compile_settings(
                None,
                kind,
                Profile::Dev,
                lang,
                HostOs::Windows,
                Path::new(BUNDLED),
                true,
                Mode::Compile,
            )
        };
        let link = compile(Lang::Cxx);
        plan(&Plan {
            package: "app",
            sources,
            products,
            layout: &layout,
            kind,
            program: Path::new("C:/clang.exe"),
            mode: Mode::Compile,
            compile: &compile,
            link: (&link.build, &link.profile),
            color: false,
        })
    }

    fn compiled(g: &Graph) -> Vec<PathBuf> {
        g.steps
            .iter()
            .filter_map(|s| match &s.kind {
                StepKind::Compile { source } => Some(source.clone()),
                _ => None,
            })
            .collect()
    }

    /// The test build compiles the body, the sources under `tests/` and the
    /// runtime with `CUNO_TEST`, each once and the runtime as C, never an
    /// entry file. It links the in-source tests from the body and the
    /// runtime, and each test file with the helpers, the same body and the
    /// runtime, each into its own place as a console program
    /// (`project-layout.md`, "Link rules"; `build.md`, "Output directory").
    ///
    /// Mutation: compile an entry file, compile the body twice, leave out
    /// the helpers, the body or the runtime from a link, give a helper an
    /// executable of its own, link a test file into the in-source tests,
    /// take the first test file for every `tests/` executable, compile the
    /// runtime as C++, link a test executable as a Windows program, or put
    /// one anywhere but `test/<package>/bin/`; this test fails.
    #[test]
    fn a_test_build_links_each_test_executable_as_the_spec_says() {
        let mut sources = test_sources();
        sources
            .integration_tests
            .push(source("C:/p/tests/u.cpp", Lang::Cxx));
        let executables =
            crate::sources::test_executables(Path::new("C:/p"), "app", &sources, HostOs::Windows)
                .unwrap();
        let g = test_plan(
            Products::Tests {
                executables: &executables,
                runtime: Path::new(RUNTIME),
            },
            &sources,
        );
        assert_eq!(
            compiled(&g),
            [
                PathBuf::from("C:/p/src/a.cpp"),
                PathBuf::from("C:/p/tests/t.cpp"),
                PathBuf::from("C:/p/tests/u.cpp"),
                PathBuf::from("C:/p/tests/common/h.c"),
                PathBuf::from(RUNTIME),
            ]
        );
        for step in &g.steps {
            if matches!(step.kind, StepKind::Compile { .. }) {
                assert!(args(step).contains(&"-DCUNO_TEST".to_owned()), "{step:?}");
            }
        }
        // The runtime is C, compiled to the C standard.
        let runtime_args = args(&g.steps[4]);
        assert!(
            runtime_args.iter().any(|a| a.starts_with("-std=c1")),
            "{runtime_args:?}"
        );
        let object = |i: usize| g.steps[i].output.clone();
        assert_eq!(
            object(4),
            PathBuf::from("C:/p/target/debug/test/app/cuno/test_main.c.obj")
        );
        let links: Vec<(&str, &Path, Vec<PathBuf>)> = g
            .steps
            .iter()
            .filter_map(|s| match &s.kind {
                StepKind::Link { bin } => Some((
                    bin.as_str(),
                    s.output.as_path(),
                    s.after.iter().map(|&i| object(i)).collect(),
                )),
                _ => None,
            })
            .collect();
        assert_eq!(
            links,
            [
                (
                    "app",
                    Path::new("C:/p/target/debug/test/app/bin/app.exe"),
                    vec![object(0), object(4)]
                ),
                (
                    "tests-t",
                    Path::new("C:/p/target/debug/test/app/bin/tests-t.exe"),
                    vec![object(1), object(3), object(0), object(4)]
                ),
                (
                    "tests-u",
                    Path::new("C:/p/target/debug/test/app/bin/tests-u.exe"),
                    vec![object(2), object(3), object(0), object(4)]
                ),
            ]
        );
        for step in &g.steps[5..] {
            assert!(
                args(step).contains(&"-Wl,/subsystem:console".to_owned()),
                "{step:?}"
            );
        }
    }

    /// What compile_commands.json describes: every source of the test
    /// build but the runtime, and the entry files, with no link (`build.md`,
    /// "Output").
    ///
    /// Mutation: leave out the entry files or the sources under `tests/`,
    /// add the runtime, or link; this test fails.
    #[test]
    fn editors_see_every_source_but_the_runtime() {
        let g = test_plan(Products::CompileCommands, &test_sources());
        assert_eq!(
            compiled(&g),
            [
                PathBuf::from("C:/p/src/a.cpp"),
                PathBuf::from("C:/p/src/main.cpp"),
                PathBuf::from("C:/p/tests/t.cpp"),
                PathBuf::from("C:/p/tests/common/h.c"),
            ]
        );
        assert_eq!(g.steps.len(), 4, "no links");
    }

    /// Mutation: let the programs be planned on a test layout; this test
    /// fails.
    #[test]
    #[should_panic(expected = "products for another layout")]
    fn programs_need_a_program_layout() {
        test_plan(Products::Programs, &test_sources());
    }

    /// Mutation: let compile_commands.json, or the tests, be planned on a
    /// program's layout; this test fails.
    #[test]
    #[should_panic(expected = "products for another layout")]
    fn tests_need_a_test_layout() {
        let sources = test_sources();
        let layout = Layout::new(
            Path::new("C:/p/target"),
            Profile::Dev,
            Path::new("C:/p"),
            HostOs::Windows,
        );
        let compile = |lang| {
            compile_settings(
                None,
                CompilerKind::Clang,
                Profile::Dev,
                lang,
                HostOs::Windows,
                Path::new(BUNDLED),
                false,
                Mode::Compile,
            )
        };
        let link = compile(Lang::Cxx);
        plan(&Plan {
            package: "app",
            sources: &sources,
            products: Products::CompileCommands,
            layout: &layout,
            kind: CompilerKind::Clang,
            program: Path::new("C:/clang.exe"),
            mode: Mode::Compile,
            compile: &compile,
            link: (&link.build, &link.profile),
            color: false,
        });
    }

    /// Mutation: let a test plan check rather than compile; this test fails.
    #[test]
    #[should_panic(expected = "a test plan in another mode than compile")]
    fn a_test_plan_compiles() {
        let sources = test_sources();
        let layout = Layout::new(
            Path::new("C:/p/target"),
            Profile::Dev,
            Path::new("C:/p"),
            HostOs::Windows,
        )
        .for_tests();
        let compile = |lang| {
            compile_settings(
                None,
                CompilerKind::Clang,
                Profile::Dev,
                lang,
                HostOs::Windows,
                Path::new(BUNDLED),
                true,
                Mode::Check,
            )
        };
        let link = compile(Lang::Cxx);
        plan(&Plan {
            package: "app",
            sources: &sources,
            products: Products::CompileCommands,
            layout: &layout,
            kind: CompilerKind::Clang,
            program: Path::new("C:/clang.exe"),
            mode: Mode::Check,
            compile: &compile,
            link: (&link.build, &link.profile),
            color: false,
        });
    }
}
