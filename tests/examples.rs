//! Every project under `examples/` builds, checks and runs on every compiler
//! here, and each executable prints what its `expected/` file says. An
//! example with tests also passes `cuno test`, with the results its
//! `expected/test.txt` says.
//!
//! An example is a directory under `examples/c/` or `examples/cpp/`, by its
//! package's language.
//! Every example is a test of its own, so the examples run in parallel, and
//! a new one fails [`every_example_has_a_test`] until it is added to
//! `examples!`.
//!
//! An example is copied into a fresh directory per compiler: building in
//! place could find an earlier build's `target/` and compile nothing.

mod support;

use std::path::Path;

use support::Project;
use support::compilers::{Compiler, Language, require};

const COMPILERS: [(&str, Compiler); 4] = [
    ("msvc", Compiler::Msvc),
    ("clang", Compiler::Clang),
    ("clang-cl", Compiler::ClangCl),
    ("gcc", Compiler::Gcc),
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

/// The language directories under `examples/`, each holding that
/// language's examples.
const LANGUAGES: [&str; 2] = ["c", "cpp"];

/// Every example directory, as `<language>/<example>`, sorted. A directory
/// beside the language directories, or one holding no `cuno.toml`, is
/// named too, so an example put in the wrong place is not left unrun.
fn examples() -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    let mut found = Vec::new();
    for entry in std::fs::read_dir(&root).unwrap_or_else(|e| panic!("{}: {e}", root.display())) {
        let dir = entry.unwrap().path();
        let language = dir.file_name().unwrap().to_string_lossy().into_owned();
        if !dir.is_dir() {
            continue;
        }
        if !LANGUAGES.contains(&language.as_str()) {
            found.push(language);
            continue;
        }
        for entry in std::fs::read_dir(&dir).unwrap() {
            let example = entry.unwrap().path();
            let name = example.file_name().unwrap().to_string_lossy().into_owned();
            found.push(format!("{language}/{name}"));
        }
    }
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

/// The `test result:` lines `cuno test` printed, one per test executable,
/// each without its `; finished in ...` time, which changes from run to run.
fn test_results(stderr: &str) -> String {
    stderr
        .lines()
        .filter(|line| line.starts_with("test result:"))
        .map(|line| {
            line.split_once("; finished in ")
                .map_or(line, |(kept, _)| kept)
        })
        .map(|line| format!("{line}\n"))
        .collect()
}

/// Whether `example` has tests: a `tests/` directory, or a source that
/// includes a bundled test header. Such an example must say its results in
/// `expected/test.txt`, so one forgotten is an error rather than tests that
/// never run here.
fn has_tests(example: &Path) -> bool {
    let mut names = Vec::new();
    files(example, example, &mut names);
    names.iter().any(|name| {
        name.starts_with("tests/")
            || std::fs::read_to_string(example.join(name))
                .is_ok_and(|text| text.contains("<cuno/test.h"))
    })
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

/// Builds, checks, tests and runs `example` on every compiler here, as the
/// module comment says.
fn check(example: &Path) {
    let name = example
        .strip_prefix(Path::new(env!("CARGO_MANIFEST_DIR")).join("examples"))
        .unwrap_or(example)
        .to_string_lossy()
        .replace('\\', "/");
    let tests = std::fs::read_to_string(example.join("expected").join("test.txt")).ok();
    assert!(
        tests.is_some() || !has_tests(example),
        "{name} has tests and no expected/test.txt"
    );
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
        if let Some(want) = &tests {
            let run = project.cuno(&["-q", "test", "--compiler", compiler]);
            let err = run.stderr.replace("\r\n", "\n");
            assert_eq!(run.code, Some(0), "{}:\n{err}", at("test"));
            assert_eq!(
                &test_results(&err),
                want,
                "{} reported other results:\n{err}",
                at("test")
            );
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

/// One test per example, so the harness runs the examples in parallel;
/// [`every_example_has_a_test`] holds this list to `examples/`.
macro_rules! examples {
    ($($test:ident = $dir:literal),+ $(,)?) => {
        /// The example directories with a test of their own.
        const TESTED: &[&str] = &[$($dir),+];
        $(
            /// Mutation: make the example's source not compile, change what
            /// it prints, remove one of its expected files, break one of its
            /// tests, or change what `cuno test` counts; this test fails,
            /// naming the compiler and the profile or `test`.
            #[test]
            fn $test() {
                check(&Path::new(env!("CARGO_MANIFEST_DIR")).join("examples").join($dir));
            }
        )+
    };
}

examples! {
    c_edition_release = "c/edition-release",
    c_hello = "c/hello",
    c_multi_file = "c/multi-file",
    c_testing = "c/testing",
    cpp_bin_path = "cpp/bin-path",
    cpp_c_in_cpp = "cpp/c-in-cpp",
    cpp_edition_release = "cpp/edition-release",
    cpp_hello = "cpp/hello",
    cpp_include_dirs = "cpp/include-dirs",
    cpp_multi_file = "cpp/multi-file",
    cpp_per_compiler_flags = "cpp/per-compiler-flags",
    cpp_several_bins = "cpp/several-bins",
    cpp_testing = "cpp/testing",
}

/// Every example under `examples/c/` and `examples/cpp/` has a test of its
/// own above, and every test names one; a directory beside `c/` and `cpp/`
/// is named as missing too.
///
/// Mutation: add an example under either language without adding it to
/// `examples!`, name one that is not there, or search one language only;
/// this test fails, naming it.
#[test]
fn every_example_has_a_test() {
    let found = examples();
    assert!(!found.is_empty(), "no examples found");
    let mut tested: Vec<&str> = TESTED.to_vec();
    tested.sort_unstable();
    assert_eq!(found, tested, "examples/ and the tests differ");
}

/// Each example sits under its package's language: `c/` for `lang = "c"`,
/// `cpp/` for C++, the default.
///
/// Mutation: put a C package under `cpp/`, or a C++ one under `c/`; this
/// test fails, naming it.
#[test]
fn each_example_sits_under_its_language() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    for example in examples() {
        let Some((language, _)) = example.split_once('/') else {
            continue;
        };
        let manifest = std::fs::read_to_string(root.join(&example).join("cuno.toml"))
            .unwrap_or_else(|e| panic!("{example}: {e}"));
        let c = manifest.lines().any(|line| line.trim() == "lang = \"c\"");
        let want = if c { "c" } else { "cpp" };
        assert_eq!(language, want, "{example} is a {want} package");
    }
}
