//! The build graph: every compile and link a build runs, with its command
//! line and what it must wait for, built without running anything
//! (`docs/en/specs/architecture.md`, "cuno-build"), so `compile_commands.json`
//! and a later backend can be made from it too.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use cuno_toolchain::compiler::{CompilerKind, Dependencies, Family};
use cuno_toolchain::flags::{
    BuildSettings, Lang, LinkInput, ProfileSettings, compile_args, dependency_args, link_args,
};

use crate::deps::Headers;

use crate::input::CompileSettings;
use crate::layout::Layout;
use crate::sources::{Source, Sources};

/// What a step does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepKind {
    /// Compiles one source.
    Compile {
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
}

/// Every step of a build, in an order where each comes after those it
/// waits for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Graph {
    /// The steps.
    pub steps: Vec<Step>,
}

/// What a build is planned from.
pub struct Plan<'a> {
    /// The package's name, which names its object directory.
    pub package: &'a str,
    /// Its classified sources.
    pub sources: &'a Sources,
    /// Where outputs go.
    pub layout: &'a Layout,
    /// The compiler.
    pub kind: CompilerKind,
    /// Its driver, which compiles and links.
    pub program: &'a Path,
    /// The settings a source of each language compiles with.
    pub compile: &'a dyn Fn(Lang) -> CompileSettings,
    /// The settings every link uses.
    pub link: (&'a BuildSettings, &'a ProfileSettings),
}

/// Plans a build: one compile per source of the package body and per entry
/// file, each once, and one link per executable of its entry file's object
/// and every body object (`project-layout.md`, "Link rules").
pub fn plan(p: &Plan<'_>) -> Graph {
    let mut steps = Vec::new();
    let mut compile = |source: &Source| -> usize {
        let settings = (p.compile)(source.lang);
        let object = p.layout.object(p.package, &source.path);
        let echo = match p.kind.family() {
            Family::Msvc => source
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned()),
            Family::Llvm => None,
        };
        let headers = match p.kind.dependencies() {
            Dependencies::SourceDependencies => Headers::Json(object.with_extension("json")),
            Dependencies::DepFile => Headers::DepFile(object.with_extension("d")),
        };
        let mut args = compile_args(&settings.input(), &source.path, Some(&object));
        args.extend(dependency_args(p.kind, headers.file()));
        steps.push(Step {
            kind: StepKind::Compile {
                source: source.path.clone(),
            },
            program: p.program.to_owned(),
            args,
            output: object,
            after: Vec::new(),
            echo,
            headers: Some(headers),
            reads: vec![source.path.clone()],
        });
        steps.len() - 1
    };
    let body: Vec<usize> = p.sources.body.iter().map(&mut compile).collect();
    let entries: Vec<usize> = p.sources.bins.iter().map(|b| compile(&b.entry)).collect();

    for (bin, entry) in p.sources.bins.iter().zip(entries) {
        let after: Vec<usize> = std::iter::once(entry).chain(body.iter().copied()).collect();
        let objects: Vec<PathBuf> = after.iter().map(|&i| steps[i].output.clone()).collect();
        let exe = p.layout.exe(&bin.name);
        let input = LinkInput {
            kind: p.kind,
            build: p.link.0,
            profile: p.link.1,
            subsystem: bin.subsystem,
        };
        steps.push(Step {
            kind: StepKind::Link {
                bin: bin.name.clone(),
            },
            program: p.program.to_owned(),
            args: link_args(&input, &objects, &exe),
            output: exe,
            after,
            echo: None,
            headers: None,
            reads: libraries(p.link.0),
        });
    }
    Graph { steps }
}

/// The libraries of `links` found in `lib-dirs`: for each, the first
/// `<lib>.lib` searching `lib-dirs` in order, as the linker searches them.
/// A library found nowhere there is a system one, found through `LIB`.
fn libraries(build: &BuildSettings) -> Vec<PathBuf> {
    build
        .links
        .iter()
        .filter_map(|lib| {
            build
                .lib_dirs
                .iter()
                .map(|dir| dir.join(format!("{lib}.lib")))
                .find(|path| path.is_file())
        })
        .collect()
}

#[cfg(test)]
mod tests;
