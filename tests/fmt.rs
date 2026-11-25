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
