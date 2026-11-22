//! `compile_commands.json`: the compile commands of a build graph, for
//! clang-tidy and editors (`docs/en/specs/build.md`, "Output").

use std::io;
use std::path::{Path, PathBuf};

use cuno_toolchain::compiler::{CompilerKind, Syntax};
use cuno_toolchain::flags::dependency_args;
use serde_json::{Value, json};

use crate::graph::{Graph, StepKind};

/// The file's name, written at the project root.
pub const FILE: &str = "compile_commands.json";

/// Writes `<root>/compile_commands.json` from `graph`: one entry per
/// compile, none for links. The graph is the one `build.md`, "Output",
/// fixes (the `dev` profile with `CUNO_TEST` defined), whatever is being
/// built. `include` (the resolved `INCLUDE`) is written out as system
/// include directories, so the file resolves outside a Developer Prompt.
///
/// The file is written beside and renamed over the old one, so a reader
/// never sees half of it.
pub fn write(
    root: &Path,
    graph: &Graph,
    include: &[PathBuf],
    kind: CompilerKind,
) -> io::Result<()> {
    let text = serde_json::to_string_pretty(&entries(root, graph, include, kind))
        .map_err(io::Error::other)?;
    let path = root.join(FILE);
    let partial = root.join(format!("{FILE}.partial"));
    std::fs::write(&partial, text)?;
    std::fs::rename(&partial, &path)
}

/// The entries [`write()`] writes.
fn entries(root: &Path, graph: &Graph, include: &[PathBuf], kind: CompilerKind) -> Value {
    let system: Vec<String> = include
        .iter()
        .flat_map(|dir| match kind.syntax() {
            Syntax::Cl => vec![format!("/imsvc{}", dir.display())],
            Syntax::Gnu => vec!["-isystem".to_owned(), dir.display().to_string()],
        })
        .collect();
    let entries: Vec<Value> = graph
        .steps
        .iter()
        .filter_map(|step| {
            let source = match &step.kind {
                StepKind::Compile { source } => source,
                StepKind::Check { .. } | StepKind::Link { .. } => return None,
            };
            // The dependency file is the build's own bookkeeping, not part
            // of what a source means; a tool reading this file is left to
            // write none.
            let mut args = step.args.clone();
            if let Some(headers) = &step.headers {
                let tail = dependency_args(kind, headers.file());
                if args.ends_with(&tail) {
                    args.truncate(args.len() - tail.len());
                }
            }
            let arguments: Vec<String> = std::iter::once(step.program.display().to_string())
                .chain(args.iter().map(|a| a.to_string_lossy().into_owned()))
                .chain(system.iter().cloned())
                .collect();
            Some(json!({
                "directory": root.display().to_string(),
                "file": source.display().to_string(),
                "arguments": arguments,
                "output": step.output.display().to_string(),
            }))
        })
        .collect();
    Value::Array(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deps::Headers;
    use crate::graph::Step;
    use std::ffi::OsString;

    fn compile(kind: CompilerKind, source: &str) -> Step {
        let object = PathBuf::from(format!("C:/p/target/debug/obj/app/{source}.obj"));
        let headers = match kind.syntax() {
            Syntax::Cl if kind == CompilerKind::Msvc => {
                Headers::Json(object.with_extension("json"))
            }
            _ => Headers::DepFile(object.with_extension("d")),
        };
        let mut args: Vec<OsString> = vec!["/c".into(), format!("C:/p/{source}").into()];
        args.extend(dependency_args(kind, headers.file()));
        Step {
            kind: StepKind::Compile {
                source: format!("C:/p/{source}").into(),
            },
            program: "C:/bin/cc.exe".into(),
            args,
            output: object,
            after: Vec::new(),
            echo: None,
            headers: Some(headers),
            reads: Vec::new(),
        }
    }

    /// Each compile is an entry, with its command but not its dependency
    /// file, and the include directories in the compiler's syntax; a link
    /// is not.
    ///
    /// Mutation: write the links too, keep the dependency arguments, drop
    /// the include directories, or write them in the other syntax; this
    /// test fails.
    #[test]
    fn every_compile_and_no_link_with_the_include_directories() {
        let include = [
            PathBuf::from("C:/sdk/ucrt"),
            PathBuf::from("C:/msvc/include"),
        ];
        for (kind, system) in [
            (
                CompilerKind::Msvc,
                vec!["/imsvcC:/sdk/ucrt", "/imsvcC:/msvc/include"],
            ),
            (
                CompilerKind::ClangCl,
                vec!["/imsvcC:/sdk/ucrt", "/imsvcC:/msvc/include"],
            ),
            (
                CompilerKind::Clang,
                vec!["-isystem", "C:/sdk/ucrt", "-isystem", "C:/msvc/include"],
            ),
        ] {
            let mut link = compile(kind, "src/main.cpp");
            link.kind = StepKind::Link { bin: "app".into() };
            link.headers = None;
            let graph = Graph {
                steps: vec![
                    compile(kind, "src/main.cpp"),
                    compile(kind, "src/a.c"),
                    link,
                ],
            };
            let value = entries(Path::new("C:/p"), &graph, &include, kind);
            let entries = value.as_array().unwrap();
            assert_eq!(entries.len(), 2, "{kind:?}");
            assert_eq!(entries[0]["file"], "C:/p/src/main.cpp");
            assert_eq!(entries[0]["directory"], "C:/p");
            let arguments: Vec<&str> = entries[0]["arguments"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| a.as_str().unwrap())
                .collect();
            let mut expected = vec!["C:/bin/cc.exe", "/c", "C:/p/src/main.cpp"];
            expected.extend(system);
            assert_eq!(arguments, expected, "{kind:?}");
        }
    }

    /// The file is written whole at the root, replacing what was there.
    ///
    /// Mutation: write it anywhere else, or leave the partial file; this
    /// test fails.
    #[test]
    fn the_file_is_written_at_the_root() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join(FILE), "old").unwrap();
        let graph = Graph {
            steps: vec![compile(CompilerKind::Clang, "src/main.cpp")],
        };
        write(root.path(), &graph, &[], CompilerKind::Clang).unwrap();
        let text = std::fs::read_to_string(root.path().join(FILE)).unwrap();
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value.as_array().unwrap().len(), 1);
        let names: Vec<_> = std::fs::read_dir(root.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, [FILE]);
    }
}
