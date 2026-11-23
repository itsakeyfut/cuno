//! A second `cuno build` runs only what changed, on the real compilers
//! (`docs/en/specs/build.md`, "Deciding what changed").
#![cfg(windows)]

mod support;

use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime};

use support::compilers::{Compiler, Language, require};
use support::{Project, Run};

const COMPILERS: [(&str, Compiler); 3] = [
    ("msvc", Compiler::Msvc),
    ("clang", Compiler::Clang),
    ("clang-cl", Compiler::ClangCl),
];

fn here(compiler: Compiler) -> bool {
    require(compiler, Language::C, "x.c", "x.exe").is_some()
}

/// A package whose `main.cpp` reads `inner.h` through `outer.h`, whose
/// `one.cpp` reads it directly, and whose `two.cpp` reads neither. The
/// inner header sits in a directory with a space and a non-ASCII name.
fn package(extra: &str) -> Project {
    Project::new()
        .file(
            "cuno.toml",
            &format!("[package]\nname = \"app\"\nversion = \"0.1.0\"\n{extra}"),
        )
        .file("src/inc dir/内側.h", "#pragma once\n#define INNER 1\n")
        .file("src/outer.h", "#pragma once\n#include \"inc dir/内側.h\"\n")
        .file(
            "src/main.cpp",
            "#include <cstdio>\n#include \"outer.h\"\nint one();\nint two();\n\
             int main() { std::printf(\"%d %d %d\\n\", INNER, one(), two()); }\n",
        )
        .file(
            "src/one.cpp",
            "#include \"inc dir/内側.h\"\nint one() { return INNER; }\n",
        )
        .file("src/two.cpp", "int two() { return 2; }\n")
}

/// The sources the `-v` output shows compiled, sorted, and whether it
/// shows a link.
fn ran(run: &Run) -> (Vec<&'static str>, bool) {
    let mut compiled = Vec::new();
    let mut linked = false;
    for line in run.stderr.lines().filter(|l| l.contains("Running `")) {
        let compile = line.contains(" /c ") || line.contains(" -c ");
        if !compile {
            linked = true;
            continue;
        }
        for name in ["main", "one", "two"] {
            let source = line.split(' ').any(|arg| {
                let arg = arg.trim_matches('"');
                [".cpp", ".c"]
                    .iter()
                    .any(|ext| arg.ends_with(&format!("\\src\\{name}{ext}")))
            });
            if source {
                compiled.push(name);
            }
        }
    }
    compiled.sort();
    (compiled, linked)
}

fn build(project: &Project, compiler: &str) -> Run {
    let run = project.cuno(&["-v", "build", "--compiler", compiler]);
    assert_eq!(run.code, Some(0), "{compiler}: {}", run.stderr);
    run
}

fn output(project: &Project) -> String {
    let run = project.run(project.path().join("target/debug/app.exe"), &[]);
    run.stdout.replace("\r\n", "\n")
}

/// A second build with nothing changed runs nothing and prints only its
/// `Finished` line; editing a header read through another header
/// recompiles exactly the two sources that read it, and relinks.
///
/// Mutation: record only the headers a source includes directly, rebuild
/// everything, relink nothing, or print `Compiling` when nothing runs;
/// this test fails.
#[test]
fn only_what_a_change_reaches_is_rebuilt() {
    for (name, compiler) in COMPILERS {
        if !here(compiler) {
            continue;
        }
        let project = package("");
        assert_eq!(
            ran(&build(&project, name)),
            (vec!["main", "one", "two"], true)
        );

        let again = build(&project, name);
        assert_eq!(ran(&again), (vec![], false), "{name}");
        let lines: Vec<&str> = again.stderr.lines().collect();
        assert_eq!(lines.len(), 1, "{name}: {}", again.stderr);
        assert!(lines[0].starts_with("    Finished `dev` profile"), "{name}");

        std::fs::write(
            project.path().join("src/inc dir/内側.h"),
            "#pragma once\n#define INNER 7\n",
        )
        .unwrap();
        assert_eq!(
            ran(&build(&project, name)),
            (vec!["main", "one"], true),
            "{name}"
        );
        assert_eq!(output(&project), "7 7 2\n", "{name}");
    }
}

/// Changing `[build] defines` changes every compile's command line, so
/// every source is recompiled.
///
/// Mutation: ignore the command line; this test fails.
#[test]
fn a_changed_setting_recompiles_what_it_affects() {
    for (name, compiler) in COMPILERS {
        if !here(compiler) {
            continue;
        }
        let project = package("");
        build(&project, name);
        std::fs::write(
            project.path().join("cuno.toml"),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\n[build]\ndefines = [\"X=1\"]\n",
        )
        .unwrap();
        assert_eq!(
            ran(&build(&project, name)),
            (vec!["main", "one", "two"], true),
            "{name}"
        );
    }
}

/// A source put back with an older time, as a restore from a backup does,
/// is rebuilt: what is recorded is compared, not which file is newer.
///
/// Mutation: judge by "newer than the output"; this test fails.
#[test]
fn a_restored_older_file_is_rebuilt() {
    if !here(Compiler::Msvc) {
        return;
    }
    let project = package("");
    build(&project, "msvc");
    let two = project.path().join("src/two.cpp");
    std::fs::write(&two, "int two() { return 9; }\n").unwrap();
    set_time(&two, SystemTime::now() - Duration::from_secs(86_400 * 365));
    assert_eq!(ran(&build(&project, "msvc")), (vec!["two"], true));
    assert_eq!(output(&project), "1 1 9\n");
}

fn set_time(path: &Path, time: SystemTime) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(time)
        .unwrap();
}

/// Editing a source recompiles it alone, and relinks.
///
/// Mutation: leave the source out of what a compile records; this test
/// fails on clang-cl and msvc, whose dependency files do not name it.
#[test]
fn an_edited_source_alone_is_recompiled() {
    for (name, compiler) in COMPILERS {
        if !here(compiler) {
            continue;
        }
        let project = package("");
        build(&project, name);
        std::fs::write(
            project.path().join("src/two.cpp"),
            "int two() { return 22; }\n",
        )
        .unwrap();
        assert_eq!(ran(&build(&project, name)), (vec!["two"], true), "{name}");
        assert_eq!(output(&project), "1 1 22\n", "{name}");
    }
}

/// A file read by `#embed` is an input like a header, on clang and
/// clang-cl (cl has no `#embed`).
///
/// Mutation: ask clang-cl for `/showIncludes`, which does not name it, or
/// leave it unable to write the file, so the step runs every time; this
/// test fails.
#[test]
fn a_file_read_by_embed_is_an_input() {
    for (name, compiler) in [("clang", Compiler::Clang), ("clang-cl", Compiler::ClangCl)] {
        if !here(compiler) {
            continue;
        }
        let project = Project::new()
            .file(
                "cuno.toml",
                "[package]\nname = \"app\"\nversion = \"0.1.0\"\nlang = \"c\"\nedition = \"c23\"\n",
            )
            .file("src/data.bin", "A")
            .file(
                "src/main.c",
                "#include <stdio.h>\nstatic const char d[] = {\n#embed \"data.bin\"\n};\n\
                 int main(void) { printf(\"%c\\n\", d[0]); return 0; }\n",
            );
        build(&project, name);
        let data = project.path().join("src/data.bin");
        std::fs::write(&data, "B").unwrap();
        // Out of the margin a build gives a file saved just before it, so
        // the rebuild is recorded (build.md, "Deciding what changed").
        set_time(&data, SystemTime::now() - Duration::from_secs(60));
        assert!(ran(&build(&project, name)).1, "{name}");
        assert_eq!(output(&project), "B\n", "{name}");
        // Recorded with the file, not left to run every time.
        assert_eq!(ran(&build(&project, name)), (vec![], false), "{name}");
    }
}

/// Rebuilding a library of `links` found in `lib-dirs` relinks, without
/// recompiling.
///
/// Mutation: leave the libraries out of what a link reads; this test
/// fails.
#[test]
fn a_rebuilt_library_is_relinked() {
    let Some(cl) = require(Compiler::Msvc, Language::Cxx, "foo.cpp", "foo.exe") else {
        return;
    };
    let project = Project::new()
        .file(
            "cuno.toml",
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\n[build]\nlinks = [\"foo\"]\nlib-dirs = [\"lib\"]\n",
        )
        .file(
            "src/main.cpp",
            "#include <cstdio>\nint foo();\nint main() { std::printf(\"%d\\n\", foo()); }\n",
        );
    let lib_dir = project.path().join("lib");
    std::fs::create_dir_all(&lib_dir).unwrap();
    let library = |value: u32| {
        std::fs::write(
            lib_dir.join("foo.cpp"),
            format!("int foo() {{ return {value}; }}\n"),
        )
        .unwrap();
        let mut compile = Command::new(cl.get_program());
        let mut archive = Command::new(Path::new(cl.get_program()).with_file_name("lib.exe"));
        for cmd in [&mut compile, &mut archive] {
            for (key, value) in cl.get_envs() {
                match value {
                    Some(value) => cmd.env(key, value),
                    None => cmd.env_remove(key),
                };
            }
            cmd.current_dir(&lib_dir);
        }
        let compiled = compile.args(["/nologo", "/c", "foo.cpp"]).output().unwrap();
        assert!(compiled.status.success(), "{compiled:?}");
        let archived = archive.args(["/nologo", "foo.obj"]).output().unwrap();
        assert!(archived.status.success(), "{archived:?}");
    };
    library(1);
    build(&project, "msvc");
    assert_eq!(output(&project), "1\n");
    library(2);
    assert_eq!(ran(&build(&project, "msvc")), (vec![], true));
    assert_eq!(output(&project), "2\n");
}

/// A user's `-MP`, which adds a rule per header, does not make every build
/// recompile.
///
/// Mutation: read every rule of the dependency file; this test fails.
#[test]
fn a_users_extra_dependency_rules_are_ignored() {
    if !here(Compiler::Clang) {
        return;
    }
    let project = package("[build.clang]\nflags = [\"-MP\"]\n");
    build(&project, "clang");
    assert_eq!(ran(&build(&project, "clang")), (vec![], false));
}

/// A build started below the root records what a build at the root
/// reads, so nothing runs again.
///
/// Mutation: resolve a relative header against the directory cuno was
/// started in; this test fails.
#[test]
fn a_build_below_the_root_is_up_to_date_with_one_at_it() {
    for (name, compiler) in COMPILERS {
        if !here(compiler) {
            continue;
        }
        let project = package("");
        let below = |args: &[&str]| {
            let mut cmd = Command::new(env!("CARGO_BIN_EXE_cuno"));
            cmd.args(args).current_dir(project.path().join("src"));
            project.run_command(cmd)
        };
        let first = below(&["-v", "build", "--compiler", name]);
        assert_eq!(first.code, Some(0), "{name}: {}", first.stderr);
        assert_eq!(ran(&build(&project, name)), (vec![], false), "{name}");
        let again = below(&["-v", "build", "--compiler", name]);
        assert_eq!(ran(&again), (vec![], false), "{name}");
    }
}
