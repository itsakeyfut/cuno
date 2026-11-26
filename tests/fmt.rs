//! `cuno fmt` run as a process: what it formats, leaves, prints and exits
//! with (`docs/en/specs/commands.md`, "`cuno fmt`";
//! `docs/en/specs/dev-tools.md`, "Target files"). It needs clang-format
//! and no compiler.

mod support;

use support::Project;
use support::compilers::require_llvm_tool;

const MANIFEST: &str = "[package]\nname = \"app\"\nversion = \"0.1.0\"\n";

/// Unformatted C++ that clang-format rewrites.
const UGLY: &str = "int  main( ){int* p=0;return 0;}\n";

/// A package with an unformatted entry file, an excluded file, and a
/// header beside a `[[bin]] path` outside `src/`.
fn package() -> Project {
    Project::new()
        .file(
            "cuno.toml",
            &format!(
                "{MANIFEST}[[bin]]\nname = \"app\"\npath = \"src/main.cpp\"\n\
                 [[bin]]\nname = \"tool\"\npath = \"tools/tool.cpp\"\n\
                 [fmt]\nexclude = [\"src/third_party/**\"]\n"
            ),
        )
        .file("src/main.cpp", UGLY)
        .file("src/third_party/x.cpp", "int  x;\n")
        .file("tools/tool.cpp", "int main() { return 0; }\n")
        .file("tools/tool.hpp", "int  f( );\n")
}

/// `cuno fmt --check` writes nothing, names each unformatted file and exits
/// 1; `cuno fmt` formats those and only those, counts them, and writes
/// `.clang-format`; afterwards both find nothing to do.
///
/// Mutation: ignore `exclude`, leave out the `[[bin]]` directory, write
/// under `--check`, exit 0 with an unformatted file, or count unchanged
/// files; this test fails.
#[test]
fn fmt_formats_the_package_s_code_and_check_names_what_is_not() {
    if require_llvm_tool("clang-format").is_none() {
        return;
    }
    let project = package();
    let root = project.path();
    let read = |path: &str| std::fs::read_to_string(root.join(path)).unwrap();

    project.cuno(&["fmt", "--check"]).assert(
        1,
        "",
        "error: `src/main.cpp` is not formatted\n\
         error: `tools/tool.hpp` is not formatted\n\
         help: run cuno fmt to format them\n",
    );
    assert_eq!(read("src/main.cpp"), UGLY);
    assert!(!root.join(".clang-format").exists());

    project
        .cuno(&["fmt"])
        .assert(0, "", "   Formatted 2 files\n");
    assert_eq!(
        read("src/main.cpp"),
        "int main() {\n    int* p = 0;\n    return 0;\n}\n"
    );
    assert_eq!(read("tools/tool.hpp"), "int f();\n");
    assert_eq!(read("src/third_party/x.cpp"), "int  x;\n");
    assert!(root.join(".clang-format").is_file());

    project
        .cuno(&["fmt"])
        .assert(0, "", "   Formatted 0 files\n");
    project.cuno(&["fmt", "--check"]).assert(0, "", "");
    project.cuno(&["fmt", "-q"]).assert(0, "", "");
}

/// A C package's code, `.h` included, takes the C pointer side, whatever
/// `.clang-format` the root holds (`dev-tools.md`, "Target files").
///
/// Mutation: style C as C++, or format with the root's file; this test
/// fails.
#[test]
fn fmt_styles_in_the_package_s_language_from_cuno_toml() {
    if require_llvm_tool("clang-format").is_none() {
        return;
    }
    let project = Project::new()
        .file("cuno.toml", &format!("{MANIFEST}lang = \"c\"\n"))
        .file("src/main.c", "int  main(void){int* p=0;return 0;}\n")
        .file("src/util.h", "int* f(void);\n")
        .file(".clang-format", "BasedOnStyle: Google\n");
    let run = project.cuno(&["fmt"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    let read = |path: &str| std::fs::read_to_string(project.path().join(path)).unwrap();
    assert_eq!(
        read("src/main.c"),
        "int main(void) {\n    int *p = 0;\n    return 0;\n}\n"
    );
    assert_eq!(read("src/util.h"), "int *f(void);\n");
    assert_eq!(read(".clang-format"), "BasedOnStyle: Google\n");
}

/// A source that is a link to a file is left as it is and warned about,
/// since its target may be outside the project (`dev-tools.md`, "Target
/// files"). Skipped where this account may not make file links.
///
/// Mutation: format through links; this test fails.
#[cfg(windows)]
#[test]
fn fmt_leaves_a_link_and_says_so() {
    if require_llvm_tool("clang-format").is_none() {
        return;
    }
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("shared.cpp");
    std::fs::write(&target, UGLY).unwrap();
    let project = Project::new()
        .file("cuno.toml", MANIFEST)
        .file("src/main.cpp", "int main() { return 0; }\n");
    if std::os::windows::fs::symlink_file(&target, project.path().join("src/shared.cpp")).is_err() {
        eprintln!("skipped: this account cannot make file links");
        return;
    }
    project.cuno(&["fmt"]).assert(
        0,
        "",
        "warning: `src/shared.cpp` is a link; cuno fmt leaves it\n   Formatted 0 files\n",
    );
    assert_eq!(std::fs::read_to_string(&target).unwrap(), UGLY);
}

/// A directory under `src/` with its own `cuno.toml` is another package:
/// fmt names it with the build's warning and leaves its files alone
/// (`project-layout.md`, "Source collection and linking").
///
/// Mutation: stop printing the code's warnings in `cuno fmt`, or format the
/// nested package; this test fails.
#[test]
fn fmt_leaves_a_nested_package_and_says_so() {
    if require_llvm_tool("clang-format").is_none() {
        return;
    }
    let project = Project::new()
        .file("cuno.toml", MANIFEST)
        .file(
            "src/main.cpp",
            "int main() { return 0; }
",
        )
        .file("src/sub/cuno.toml", MANIFEST)
        .file("src/sub/x.cpp", UGLY);
    project.cuno(&["fmt", "--check"]).assert(
        0,
        "",
        "warning: src/sub is not built: it has its own cuno.toml (another package)
",
    );
}

/// A temporary directory that does not exist stops fmt with the error
/// every command gives for it: what the directory was for, and how to fix
/// it (`architecture.md`, "What goes in `cuno-core`"). No source is rewritten.
///
/// Mutation: give fmt its own error again, without the help; this test
/// fails.
#[test]
fn a_missing_temporary_directory_stops_fmt_with_the_shared_help() {
    if require_llvm_tool("clang-format").is_none() {
        return;
    }
    let project = package();
    let temp = project.home().join("missing").join("temp");
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_cuno"));
    cmd.arg("fmt").env("TEMP", &temp).env("TMP", &temp);
    let run = project.run_command(cmd);
    assert_eq!(run.code, Some(3), "{}", run.stderr);
    let error = format!("error: cannot create {}\\cuno-fmt-", temp.display());
    assert!(run.stderr.contains(&error), "{}", run.stderr);
    assert!(run.stderr.contains(" to format: "), "{}", run.stderr);
    assert!(
        run.stderr.ends_with(
            "help: set TMP and TEMP to a directory that exists and can be written
"
        ),
        "{}",
        run.stderr
    );
    assert_eq!(
        std::fs::read_to_string(project.path().join("src/main.cpp")).unwrap(),
        UGLY
    );
}

/// A `[fmt.clang-format]` clang-format refuses is a settings problem, exit
/// 3, before any file is formatted, and names neither a source nor a
/// scratch file (`commands.md`, "`cuno fmt`").
///
/// Mutation: format without checking the configuration first; this test
/// fails.
#[test]
fn a_refused_configuration_is_exit_3_before_formatting() {
    if require_llvm_tool("clang-format").is_none() {
        return;
    }
    let project = package().file(".cuno/config.toml", "");
    std::fs::write(
        project.path().join("cuno.toml"),
        format!("{MANIFEST}[fmt.clang-format]\nBogusKey = 1\n"),
    )
    .unwrap();
    let run = project.cuno(&["fmt"]);
    assert_eq!(run.code, Some(3), "{}", run.stderr);
    assert!(
        run.stderr
            .starts_with("error: clang-format refuses the configuration from [fmt] in cuno.toml: "),
        "{}",
        run.stderr
    );
    assert!(run.stderr.contains("BogusKey"), "{}", run.stderr);
    assert!(!run.stderr.contains("cuno-fmt-"), "{}", run.stderr);
    assert!(
        run.stderr
            .ends_with("help: correct [fmt.clang-format] in cuno.toml\n"),
        "{}",
        run.stderr
    );
    assert_eq!(
        std::fs::read_to_string(project.path().join("src/main.cpp")).unwrap(),
        UGLY
    );
}

/// A clang-format that is not there is an environment problem, exit 3,
/// before anything is formatted.
///
/// Mutation: format without a clang-format, or exit with another code;
/// this test fails.
#[test]
fn fmt_without_clang_format_is_exit_3() {
    let project = package().file(
        ".cuno/config.toml",
        "[toolchain.paths]\nclang-format = \"C:/nope/clang-format.exe\"\n",
    );
    let run = project.cuno(&["fmt"]);
    assert_eq!(run.code, Some(3), "{}", run.stderr);
    assert!(run.stderr.contains("clang-format"), "{}", run.stderr);
    assert_eq!(
        std::fs::read_to_string(project.path().join("src/main.cpp")).unwrap(),
        UGLY
    );
}
