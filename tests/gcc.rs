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

const C: &str = "#include <stdio.h>\nint main(void) { puts(\"hello from C\"); return 0; }\n";

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
/// fails silently), link C++ with `gcc`, or leave libstdc++ or winpthread
/// dynamic; this test fails.
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
