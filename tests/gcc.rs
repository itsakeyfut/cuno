//! Building and running with MinGW GCC through the `cuno` command: the C
//! and C++ hello programs build and run with `--compiler gcc`, and what
//! they become starts outside cuno, with no GCC directory on `PATH`
//! (`docs/en/specs/build.md`, "Settings and flags", the runtime note).
#![cfg(windows)]

mod support;

use std::path::Path;
use std::process::Command;

use support::Project;
use support::compilers::{Compiler, Language, require_driver};

/// A package in `lang` built with `runtime`, of `source` at `file`.
fn hello(lang: &str, runtime: &str, file: &str, source: &str) -> Project {
    Project::new()
        .file(
            "cuno.toml",
            &format!(
                "[package]\nname = \"hello\"\nversion = \"0.1.0\"\nlang = \"{lang}\"\n\n[build]\nruntime = \"{runtime}\"\n"
            ),
        )
        .file(file, source)
}

/// Starts a thread, which pulls in winpthread even in C.
const C: &str = "#include <pthread.h>\n#include <stdio.h>\nstatic void *run(void *a) { return a; }\nint main(void) {\n    pthread_t t;\n    pthread_create(&t, 0, run, 0);\n    pthread_join(t, 0);\n    puts(\"hello from C\");\n    return 0;\n}\n";

/// Uses `std::thread` and `<iostream>`, which pull in libstdc++ and, through
/// it, winpthread: the DLLs a program must not need.
const CXX: &str = "#include <iostream>\n#include <thread>\nint main() {\n    std::thread t([] {});\n    t.join();\n    std::cout << \"hello from C++\" << std::endl;\n}\n";

/// The DLLs `exe` imports, lower-cased, from its import directory as
/// `objdump` beside GCC lists them.
fn imports(exe: &Path) -> Vec<String> {
    let objdump = support::compilers::gcc_dir().unwrap().join("objdump.exe");
    let out = Command::new(objdump).arg("-p").arg(exe).output().unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.trim().strip_prefix("DLL Name: "))
        .map(str::to_ascii_lowercase)
        .collect()
}

/// `cuno build` and `cuno run` with `--compiler gcc` build and run a C and
/// a C++ program, under both runtimes and both profiles; each executable
/// imports only Windows' own DLLs, and runs with `PATH` holding only the
/// system directory.
///
/// Mutation: leave GCC's directory off its `PATH` (its compiler proper then
/// fails silently), or leave libgcc or winpthread dynamic, for C or C++;
/// this test fails. (Linking C++ with `gcc` would still link here, as
/// libstdc++ is named; the unit tests hold that it is `g++`.)

#[test]
fn hello_builds_runs_and_starts_outside_cuno() {
    if require_driver(Compiler::Gcc, Language::C).is_none() {
        return;
    }
    let system = std::env::var_os("SystemRoot")
        .map(|root| Path::new(&root).join("System32"))
        .unwrap();
    for (lang, file, source, printed) in [
        ("c", "src/main.c", C, "hello from C"),
        ("c++", "src/main.cpp", CXX, "hello from C++"),
    ] {
        for runtime in ["dynamic", "static"] {
            let project = hello(lang, runtime, file, source);
            for (flags, dir) in [(&[][..], "debug"), (&["--release"][..], "release")] {
                let at = format!("{lang} {runtime} {dir}");
                let run = project.cuno(&[&["-q", "run", "--compiler", "gcc"][..], flags].concat());
                assert_eq!(run.code, Some(0), "{at}: {}", run.stderr);
                assert_eq!(run.stdout.trim(), printed, "{at}");

                let exe = project.path().join("target").join(dir).join("hello.exe");
                let foreign: Vec<String> = imports(&exe)
                    .into_iter()
                    .filter(|dll| dll != "kernel32.dll" && !dll.starts_with("api-ms-win-crt-"))
                    .collect();
                assert!(foreign.is_empty(), "{at} imports {foreign:?}");
                let alone = Command::new(&exe)
                    .env_clear()
                    .env("PATH", &system)
                    .env("SystemRoot", system.parent().unwrap())
                    .output()
                    .unwrap();
                assert!(alone.status.success(), "{at}: {alone:?}");
                assert_eq!(
                    String::from_utf8_lossy(&alone.stdout).trim(),
                    printed,
                    "{at}"
                );
            }
        }
    }
}

/// GCC needs no Visual Studio: with none to be found, `cuno build --compiler
/// gcc` builds, and `cuno doctor --compiler gcc` finds no problem with it
/// (the MSVC line says MSVC is not found).
///
/// Mutation: make a missing Visual Studio stop a GCC build, or report it as
/// a problem when GCC is asked for; this test fails.
#[test]
fn gcc_builds_without_visual_studio() {
    if require_driver(Compiler::Gcc, Language::C).is_none() {
        return;
    }
    let project = hello("c", "dynamic", "src/main.c", C);
    let empty = project.path().join("empty");
    std::fs::create_dir(&empty).unwrap();
    let without_vs = |args: &[&str]| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_cuno"));
        cmd.args(args).env("ProgramFiles(x86)", &empty);
        project.run_command(cmd)
    };
    let built = without_vs(&["build", "--compiler", "gcc"]);
    assert_eq!(built.code, Some(0), "{}", built.stderr);
    let doctor = without_vs(&["doctor", "--compiler", "gcc"]);
    assert!(
        doctor.stderr.contains("        Info msvc not found\n"),
        "{}",
        doctor.stderr
    );
    assert!(!doctor.stderr.contains("vswhere"), "{}", doctor.stderr);
    assert!(
        doctor.stderr.contains("    Selected gcc "),
        "{}",
        doctor.stderr
    );
}

/// A header a project puts in front of one of GCC's own, by its path
/// inside GCC's include directories (`sys/stat.h`), recompiles what read
/// GCC's (`docs/en/specs/build.md`, "Deciding what changed").
///
/// Mutation: give the build records no GCC include directories, so the
/// header is looked for by its file name alone; this test fails.
#[test]
fn a_header_put_in_front_of_gcc_s_own_recompiles() {
    if require_driver(Compiler::Gcc, Language::C).is_none() {
        return;
    }
    let source = "#include <stdio.h>\n#include <sys/stat.h>\nint main(void) {\n#ifdef SHADOWED\n    puts(\"shadowed\");\n#else\n    puts(\"gcc's\");\n#endif\n    return 0;\n}\n";
    let project = Project::new()
        .file(
            "cuno.toml",
            "[package]\nname = \"hello\"\nversion = \"0.1.0\"\nlang = \"c\"\n\n[build]\ninclude-dirs = [\"inc\"]\n",
        )
        .file("src/main.c", source);
    let run = || project.cuno(&["-q", "run", "--compiler", "gcc"]);
    assert_eq!(run().stdout.trim(), "gcc's");
    std::fs::create_dir_all(project.path().join("inc/sys")).unwrap();
    std::fs::write(
        project.path().join("inc/sys/stat.h"),
        "#define SHADOWED 1\n#include_next <sys/stat.h>\n",
    )
    .unwrap();
    // Older than the build's start by the margin the records keep.
    std::thread::sleep(std::time::Duration::from_secs(3));
    let after = run();
    assert_eq!(after.stdout.trim(), "shadowed", "{}", after.stderr);
}

/// The GCC written in `[toolchain.paths] gcc` builds; one with no file there
/// is an error naming the setting, exit 3.
///
/// Mutation: let the build ignore the setting, or take another GCC when
/// the written one is missing; this test fails.
#[test]
fn the_configured_gcc_is_the_one_used() {
    if require_driver(Compiler::Gcc, Language::C).is_none() {
        return;
    }
    let gcc = support::compilers::gcc_dir().unwrap().join("gcc.exe");
    let config = |path: &Path| format!("[toolchain.paths]\ngcc = '{}'\n", path.display());
    let project = hello("c", "dynamic", "src/main.c", C).file(".cuno/config.toml", &config(&gcc));
    let built = project.cuno(&["-v", "build", "--compiler", "gcc"]);
    assert_eq!(built.code, Some(0), "{}", built.stderr);
    assert!(
        built.stderr.contains(&gcc.display().to_string()),
        "{}",
        built.stderr
    );
    let missing = project.path().join("nowhere").join("gcc.exe");
    std::fs::write(project.path().join(".cuno/config.toml"), config(&missing)).unwrap();
    let refused = project.cuno(&["build", "--compiler", "gcc"]);
    assert_eq!(refused.code, Some(3), "{}", refused.stderr);
    assert!(
        refused.stderr.contains("[toolchain.paths] gcc"),
        "{}",
        refused.stderr
    );
}

/// The whole flow with GCC, on a package `cuno new` makes: build, run,
/// test, fmt and lint all succeed, the test run finds the tests the
/// template holds, and clang-tidy reads GCC's commands as MinGW's.
///
/// Mutation: give clang-tidy GCC's commands without the MinGW target, keep
/// `#pragma comment` for GCC (with `warnings-as-errors`), or register no C
/// test under GCC; this test fails.
#[test]
fn the_whole_flow_works_with_gcc() {
    if require_driver(Compiler::Gcc, Language::C).is_none()
        || support::compilers::require_llvm_tool("clang-tidy").is_none()
    {
        return;
    }
    for lang in ["c", "c++"] {
        let parent = Project::new();
        let made = parent.cuno(&["new", "app", "--lang", lang, "--vcs", "none"]);
        assert_eq!(made.code, Some(0), "{}", made.stderr);
        let app = parent.path().join("app");
        let manifest = app.join("cuno.toml");
        let text = std::fs::read_to_string(&manifest).unwrap();
        std::fs::write(
            &manifest,
            format!("{text}\n[build]\nwarnings = \"all\"\nwarnings-as-errors = true\n"),
        )
        .unwrap();
        let cuno = |args: &[&str]| {
            let mut cmd = Command::new(env!("CARGO_BIN_EXE_cuno"));
            cmd.args(args).current_dir(&app);
            parent.run_command(cmd)
        };
        for args in [
            &["build", "--compiler", "gcc"][..],
            &["run", "--compiler", "gcc"],
            &["fmt", "--check"],
            &["lint", "--compiler", "gcc"],
        ] {
            let run = cuno(args);
            assert_eq!(run.code, Some(0), "{lang} {args:?}: {}", run.stderr);
        }
        // clang-tidy is told the `__GNUC__` the build sees.
        let commands = std::fs::read_to_string(app.join("compile_commands.json")).unwrap();
        assert!(commands.contains("\"-fgnuc-version="), "{commands}");
        let tested = cuno(&["test", "--compiler", "gcc"]);
        assert_eq!(tested.code, Some(0), "{lang}: {}", tested.stderr);
        assert!(
            tested.stderr.contains("test result: ok. 1 passed"),
            "{lang}: {}",
            tested.stderr
        );
    }
}

/// A package `cuno new` makes under a directory that is not ASCII, with a
/// source and a test file named so too, builds, runs and tests with GCC,
/// also when the temporary directory is not ASCII (a Japanese user name,
/// given to GCC by its short name, LTO included), and a second build runs
/// nothing. What cannot be named in ASCII, a target directory outside the
/// project or an executable's name, is refused before compiling (`docs/en/specs/toolchain.md`, "MinGW
/// (GCC)").
///
/// Mutation: drop `-pipe`, name GCC's objects or executable absolutely,
/// leave the output names unencoded, skip the checks, refuse a check for
/// the temporary directory, give GCC the temporary directory's long name
/// (caught where the volume keeps short names), or let an executable named
/// in another script through; this test fails.
#[test]
fn paths_that_are_not_ascii_build_or_are_refused() {
    if require_driver(Compiler::Gcc, Language::C).is_none() {
        return;
    }
    let parent = Project::new();
    let japanese = parent.path().join("日本語");
    std::fs::create_dir(&japanese).unwrap();
    let temp = parent.path().join("一時");
    std::fs::create_dir(&temp).unwrap();
    let cuno = |dir: &Path, args: &[&str], vars: &[(&str, &Path)]| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_cuno"));
        cmd.args(args).current_dir(dir);
        for (name, value) in vars {
            cmd.env(name, value);
        }
        parent.run_command(cmd)
    };
    let made = cuno(
        &japanese,
        &["new", "app", "--lang", "c", "--vcs", "none"],
        &[],
    );
    assert_eq!(made.code, Some(0), "{}", made.stderr);
    let app = japanese.join("app");
    std::fs::write(
        app.join("src/漢字.c"),
        "int twice(int x) { return 2 * x; }\n",
    )
    .unwrap();
    std::fs::create_dir_all(app.join("tests")).unwrap();
    std::fs::write(
        app.join("tests/試験.c"),
        "#include <cuno/test.h>\nint twice(int x);\ncuno_test(doubles) { cuno_check_eq(twice(2), 4); }\n",
    )
    .unwrap();
    let temps: &[(&str, &Path)] = &[("TMP", &temp), ("TEMP", &temp)];
    // Older than a build's start by the margin the records keep, so the
    // first build records them (build.md, "Deciding what changed").
    std::thread::sleep(std::time::Duration::from_secs(3));
    for args in [
        &["build", "--compiler", "gcc"][..],
        &["run", "--compiler", "gcc"],
        &["test", "--compiler", "gcc"],
    ] {
        let run = cuno(&app, args, &[]);
        assert_eq!(run.code, Some(0), "{args:?}: {}", run.stderr);
    }
    let again = cuno(&app, &["-v", "build", "--compiler", "gcc"], &[]);
    assert_eq!(again.code, Some(0), "{}", again.stderr);
    assert!(
        !again.stderr.contains("Running"),
        "built again:\n{}",
        again.stderr
    );

    std::fs::write(
        app.join("cuno.toml"),
        std::fs::read_to_string(app.join("cuno.toml")).unwrap()
            + "\n[profile.release]\nlto = true\n",
    )
    .unwrap();
    // A temporary directory that is not ASCII (a Japanese user name) is
    // given to GCC by its short name, so a link, LTO's included, builds.
    // Where the volume keeps no short names, a link builds when the code
    // page holds the path and is refused when it does not, and LTO's is
    // refused: never left to fail in the linker (exit 1).
    for args in [
        &["build", "--compiler", "gcc"][..],
        &["build", "--release", "--compiler", "gcc"],
    ] {
        let run = cuno(&app, args, temps);
        if cuno_toolchain::short_path::ascii_short_path(&temp).is_some() {
            assert_eq!(run.code, Some(0), "{args:?}: {}", run.stderr);
        } else {
            assert!(
                run.code == Some(0)
                    || (run.code == Some(3) && run.stderr.contains("has no short name")),
                "{args:?}: {}",
                run.stderr
            );
        }
    }

    let outside = parent.path().join("出力");
    let target = cuno(
        &app,
        &[
            "build",
            "--compiler",
            "gcc",
            "--target-dir",
            outside.to_str().unwrap(),
        ],
        &[],
    );
    assert_eq!(target.code, Some(3), "{}", target.stderr);
    assert!(
        target.stderr.contains("the target directory"),
        "{}",
        target.stderr
    );
    // A check links nothing, so LTO's temporary directory does not stop it.
    let check = cuno(&app, &["check", "--release", "--compiler", "gcc"], temps);
    assert_eq!(check.code, Some(0), "{}", check.stderr);
    // An executable named after a file in `src/bin/` that is not ASCII
    // cannot be written by the linker, and is refused.
    std::fs::create_dir_all(app.join("src/bin")).unwrap();
    std::fs::write(app.join("src/bin/道具.c"), "int main(void) { return 0; }\n").unwrap();
    let bin = cuno(&app, &["build", "--compiler", "gcc"], temps);
    assert_eq!(bin.code, Some(3), "{}", bin.stderr);
    assert!(
        bin.stderr.contains("names an executable that is not ASCII"),
        "{}",
        bin.stderr
    );
}
