//! Every project under `examples/` builds, checks and runs on every compiler
//! here, and each executable prints what its `expected/` file says.
//!
//! An example is any directory under `examples/` holding a `cuno.toml`, so a
//! new one is tested without editing this file. Each is copied into a fresh
//! directory per compiler: building in place could find an earlier build's
//! `target/` and compile nothing.

mod support;

use std::path::{Path, PathBuf};

use support::Project;
use support::compilers::{Compiler, Language, require};

const COMPILERS: [(&str, Compiler); 3] = [
    ("msvc", Compiler::Msvc),
    ("clang", Compiler::Clang),
    ("clang-cl", Compiler::ClangCl),
];

/// The profiles every example is run in, with the flag that picks each and
/// the directory its executables land in.
const PROFILES: [(&str, &[&str], &str); 2] = [
    ("dev", &[], "debug"),
    ("release", &["--release"], "release"),
];

fn here(compiler: Compiler) -> bool {
    require(compiler, Language::C, "x.c", "x.exe").is_some()
}

/// Every example directory, sorted.
fn examples() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    let mut found: Vec<PathBuf> = std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("{}: {e}", root.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|dir| dir.join("cuno.toml").is_file())
        .collect();
    found.sort();
    found
}

/// Every file under `dir`, as a `/`-separated path relative to it, leaving
/// out what a build there leaves behind and `expected/`.
fn files(dir: &Path, under: &Path, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let relative = path
            .strip_prefix(under)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if ["target", "expected", "compile_commands.json"].contains(&relative.as_str()) {
            continue;
        }
        if path.is_dir() {
            files(&path, under, out);
        } else {
            out.push(relative);
        }
    }
}

/// `example` copied into a project of its own.
fn copied(example: &Path) -> Project {
    let mut names = Vec::new();
    files(example, example, &mut names);
    names.into_iter().fold(Project::new(), |project, name| {
        let text = std::fs::read_to_string(example.join(&name)).unwrap();
        project.file(&name, &text)
    })
}

/// What `bin` should print, built by `compiler` in `profile`: the most
/// specific of `<bin>.<compiler>.<profile>.txt`, `<bin>.<compiler>.txt`,
/// `<bin>.<profile>.txt` and `<bin>.txt` in `expected/`.
fn expected(example: &Path, bin: &str, compiler: &str, profile: &str) -> Option<String> {
    [
        format!("{bin}.{compiler}.{profile}.txt"),
        format!("{bin}.{compiler}.txt"),
        format!("{bin}.{profile}.txt"),
        format!("{bin}.txt"),
    ]
    .iter()
    .find_map(|name| std::fs::read_to_string(example.join("expected").join(name)).ok())
}

/// The executables a build left in `dir`.
fn executables(dir: &Path) -> Vec<String> {
    let mut found: Vec<String> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "exe"))
        .map(|path| path.file_stem().unwrap().to_string_lossy().into_owned())
        .collect();
    found.sort();
    found
}

/// Mutation: make an example's source not compile, change what one prints,
/// or remove one of its expected files; this test fails, naming the
/// example, the compiler and the profile.
#[test]
fn every_example_builds_checks_and_runs() {
    let examples = examples();
    assert!(!examples.is_empty(), "no examples found");
    for example in &examples {
        let name = example.file_name().unwrap().to_string_lossy();
        for (compiler, kind) in COMPILERS {
            if !here(kind) {
                continue;
            }
            let project = copied(example);
            let at = |what: &str| format!("{name} on {compiler}, {what}");
            for command in ["build", "check"] {
                let run = project.cuno(&[command, "--compiler", compiler]);
                assert_eq!(run.code, Some(0), "{}: {}", at(command), run.stderr);
            }
            for (profile, flags, dir) in PROFILES {
                let built = project.cuno(&[&["build", "--compiler", compiler][..], flags].concat());
                assert_eq!(built.code, Some(0), "{}: {}", at(profile), built.stderr);
                let bins = executables(&project.path().join("target").join(dir));
                assert!(!bins.is_empty(), "{}: nothing was built", at(profile));
                for bin in bins {
                    let want = expected(example, &bin, compiler, profile).unwrap_or_else(|| {
                        panic!(
                            "{}: no expected output for `{bin}` in expected/",
                            at(profile)
                        )
                    });
                    let args = [
                        &["-q", "run", "--compiler", compiler, "--bin", &bin][..],
                        flags,
                    ]
                    .concat();
                    let run = project.cuno(&args);
                    assert_eq!(run.code, Some(0), "{} `{bin}`: {}", at(profile), run.stderr);
                    assert_eq!(
                        run.stdout.replace("\r\n", "\n"),
                        want,
                        "{} `{bin}` printed something else",
                        at(profile)
                    );
                }
            }
        }
    }
}
