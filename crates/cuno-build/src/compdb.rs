//! `compile_commands.json`: the compile commands of a build graph, for
//! clang-tidy and editors (`docs/en/specs/build.md`, "Output").

use std::io;
use std::path::Path;

use cuno_toolchain::flags::{ToolContext, for_clang_tools};
use serde_json::{Value, json};

use crate::graph::{Graph, StepKind};

/// The file's name, written at the project root.
pub const FILE: &str = "compile_commands.json";

/// Writes `<root>/compile_commands.json` from `graph`: one entry per
/// compile, none for a link. The graph is the one `build.md`, "Output",
/// fixes (the `dev` profile with `CUNO_TEST` defined), whatever is being
/// built.
///
/// The file is written beside and renamed over the old one, so a reader
/// never sees half of it, and not at all when it would not change, so an
/// editor watching it does not analyse everything again after every
/// build. A write that fails leaves no partial file behind; the caller
/// warns and the build goes on (`build.md`, "Output").
pub fn write(root: &Path, graph: &Graph, context: ToolContext<'_>) -> io::Result<()> {
    let text =
        serde_json::to_string_pretty(&entries(root, graph, context)).map_err(io::Error::other)?;
    let path = root.join(FILE);
    if std::fs::read_to_string(&path).is_ok_and(|old| old == text) {
        return Ok(());
    }
    let partial = root.join(format!("{FILE}.partial"));
    let written = std::fs::write(&partial, text).and_then(|()| std::fs::rename(&partial, &path));
    if written.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    written
}

/// The entries, apart from writing them, so they can be tested without a
/// file. Each command is what [`for_clang_tools`] makes of the compile.
fn entries(root: &Path, graph: &Graph, context: ToolContext<'_>) -> Value {
    let entries: Vec<Value> = graph
        .steps
        .iter()
        .filter_map(|step| {
            // Only compiles: the graph this is planned from builds, so it
            // has no checks, and a link has no source to analyse.
            let source = match &step.kind {
                StepKind::Compile { source } => source,
                StepKind::Check { .. } | StepKind::Link { .. } => return None,
            };
            // `args` alone: the dependency arguments are the build's own
            // bookkeeping, not part of what a source means (`Step::dependency`).
            let args = &step.args;
            let arguments: Vec<String> = std::iter::once(step.program.display().to_string())
                .chain(
                    for_clang_tools(context, args)
                        .args()
                        .iter()
                        .map(|a| a.to_string_lossy().into_owned()),
                )
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
    use cuno_toolchain::Version;
    use cuno_toolchain::compiler::{CompilerKind, Dependencies};
    use cuno_toolchain::flags::dependency_args;
    use std::ffi::OsString;
    use std::path::PathBuf;

    /// A compile of `source` with `kind`: its arguments `/c`, `extra` and
    /// the source, and its dependency arguments apart, as `graph::plan`
    /// makes them.
    fn compile(kind: CompilerKind, source: &str, extra: &[&str]) -> Step {
        let object = PathBuf::from(format!("C:/p/target/debug/obj/app/{source}.obj"));
        let headers = match kind.dependencies() {
            Dependencies::SourceDependencies => Headers::Json(object.with_extension("json")),
            Dependencies::DepFile => Headers::DepFile(object.with_extension("d")),
        };
        let mut args: Vec<OsString> = vec!["/c".into()];
        args.extend(extra.iter().map(OsString::from));
        args.push(format!("C:/p/{source}").into());
        let dependency = dependency_args(kind, headers.file());
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
            searched: Vec::new(),
            looked_for: Vec::new(),
            color: Vec::new(),
            dependency,
        }
    }

    fn arguments(entry: &Value) -> Vec<&str> {
        entry["arguments"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a.as_str().unwrap())
            .collect()
    }

    /// Each compile is an entry, with its source, object and command but
    /// not its dependency file, then `INCLUDE` after the tool's own headers
    /// in the compiler's syntax and the MSVC version; a link is not.
    ///
    /// Mutation: write the links too, drop `output`, keep the dependency
    /// arguments, drop the include directories or the version, write them
    /// with `-isystem` or in the other syntax; this test fails.
    #[test]
    fn every_compile_and_no_link_with_what_resolves_it() {
        let include = [
            PathBuf::from("C:/sdk/ucrt"),
            PathBuf::from("C:/msvc/include"),
        ];
        let version = Version {
            major: 19,
            minor: 52,
            patch: 37029,
        };
        for (kind, mode, system) in [
            (
                CompilerKind::Msvc,
                "-c",
                vec!["/imsvcC:/sdk/ucrt", "/imsvcC:/msvc/include"],
            ),
            (
                CompilerKind::ClangCl,
                "-c",
                vec!["/imsvcC:/sdk/ucrt", "/imsvcC:/msvc/include"],
            ),
            (
                CompilerKind::Clang,
                "/c",
                vec![
                    "-nostdlibinc",
                    "-idirafter",
                    "C:/sdk/ucrt",
                    "-idirafter",
                    "C:/msvc/include",
                ],
            ),
        ] {
            let mut link = compile(kind, "src/main.cpp", &[]);
            link.kind = StepKind::Link { bin: "app".into() };
            link.headers = None;
            let graph = Graph {
                steps: vec![
                    compile(kind, "src/main.cpp", &[]),
                    compile(kind, "src/a.c", &[]),
                    link,
                ],
            };
            let context = ToolContext {
                kind,
                include: &include,
                msvc: Some(&version),
            };
            let value = entries(Path::new("C:/p"), &graph, context);
            let entries = value.as_array().unwrap();
            assert_eq!(entries.len(), 2, "{kind:?}");
            assert_eq!(entries[0]["file"], "C:/p/src/main.cpp");
            assert_eq!(entries[0]["directory"], "C:/p");
            assert_eq!(
                entries[0]["output"],
                "C:/p/target/debug/obj/app/src/main.cpp.obj"
            );
            let mut expected = vec!["C:/bin/cc.exe"];
            expected.extend(system);
            expected.extend([mode, "C:/p/src/main.cpp"]);
            expected.push("-fms-compatibility-version=19.52.37029");
            assert_eq!(arguments(&entries[0]), expected, "{kind:?}");
        }
    }

    /// An MSVC command's standard switch that clang-cl does not know is
    /// written as clang-cl's, so a tool analyses the standard the build
    /// compiles; one it knows, and clang-cl's own, stay.
    ///
    /// Mutation: leave cl's `/std:c++23` or `/std:clatest`, keep its `/GL`,
    /// or rewrite a clang-cl command; this test fails.
    #[test]
    fn an_msvc_standard_is_written_as_clang_cl_reads_it() {
        let cases = [
            (CompilerKind::Msvc, "/std:c++23", "/clang:-std=c++23"),
            (CompilerKind::Msvc, "/std:clatest", "/clang:-std=c2x"),
            (CompilerKind::Msvc, "/std:c++20", "/std:c++20"),
            (
                CompilerKind::ClangCl,
                "/clang:-std=c++23",
                "/clang:-std=c++23",
            ),
        ];
        for (kind, written, read) in cases {
            let graph = Graph {
                steps: vec![compile(kind, "src/main.cpp", &[written])],
            };
            let context = ToolContext {
                kind,
                include: &[],
                msvc: None,
            };
            let value = entries(Path::new("C:/p"), &graph, context);
            assert_eq!(arguments(&value[0])[2], read, "{kind:?} {written}");
        }
        let graph = Graph {
            steps: vec![compile(CompilerKind::Msvc, "src/main.cpp", &["/GL"])],
        };
        let msvc = ToolContext {
            kind: CompilerKind::Msvc,
            include: &[],
            msvc: None,
        };
        let value = entries(Path::new("C:/p"), &graph, msvc);
        assert!(!arguments(&value[0]).contains(&"/GL"));
    }

    /// A compile's dependency arguments, the build's own bookkeeping, are
    /// not in its entry, and its own arguments all are, in order, the last
    /// included.
    ///
    /// Mutation: write the arguments a step runs with (`run_args`) rather
    /// than `args`, or drop the last argument; this test fails.
    #[test]
    fn the_dependency_arguments_are_left_out() {
        let mut step = compile(CompilerKind::Clang, "src/main.cpp", &[]);
        step.args.push("-DLAST".into());
        let file = step.headers.as_ref().unwrap().file().to_owned();
        let graph = Graph { steps: vec![step] };
        let value = entries(Path::new("C:/p"), &graph, clang());
        let args = arguments(&value[0]);
        let file = file.to_string_lossy();
        assert!(!args.iter().any(|a| a.contains(&*file)), "{args:?}");
        assert_eq!(args.last(), Some(&"-DLAST"));
    }

    fn clang() -> ToolContext<'static> {
        ToolContext {
            kind: CompilerKind::Clang,
            include: &[],
            msvc: None,
        }
    }

    fn names(dir: &Path) -> Vec<OsString> {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect()
    }

    /// The file is written whole at the root, replacing what was there,
    /// and not touched when it would not change.
    ///
    /// Mutation: write it anywhere else, leave the partial file, or write
    /// it again unchanged; this test fails.
    #[test]
    fn the_file_is_written_at_the_root_only_when_it_changes() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(FILE);
        std::fs::write(&path, "old").unwrap();
        let graph = Graph {
            steps: vec![compile(CompilerKind::Clang, "src/main.cpp", &[])],
        };
        write(root.path(), &graph, clang()).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value.as_array().unwrap().len(), 1);
        assert_eq!(names(root.path()), [FILE]);

        let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(old)
            .unwrap();
        write(root.path(), &graph, clang()).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), old);
    }

    /// A write that fails leaves nothing behind beside the file.
    ///
    /// Mutation: keep the partial file when the rename fails; this test
    /// fails.
    #[test]
    fn a_failed_write_leaves_no_partial_file() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join(FILE)).unwrap();
        let graph = Graph {
            steps: vec![compile(CompilerKind::Clang, "src/main.cpp", &[])],
        };
        assert!(write(root.path(), &graph, clang()).is_err());
        assert_eq!(names(root.path()), [FILE]);
    }
}
