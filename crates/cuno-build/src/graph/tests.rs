use super::*;
use crate::input::compile_settings;
use crate::sources::Bin;
use cuno_manifest::Profile;
use cuno_toolchain::HostOs;
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
        warnings: Vec::new(),
    }
}

fn graph(kind: CompilerKind) -> Graph {
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
            false,
            Mode::Compile,
        )
    };
    let link = compile(Lang::Cxx);
    plan(&Plan {
        package: "app",
        sources: &sources,
        layout: &layout,
        kind,
        program: Path::new("C:/cl.exe"),
        compile: &compile,
        link: (&link.build, &link.profile),
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
