//! cuno's bundled test headers and runtime (`headers/cuno/`), compiled with
//! each compiler here: tests register, run one at a time and report through
//! the result file, in C, C++17 and C++20, in one executable; and a normal
//! build of a file holding tests compiles none of them.
//! See docs/en/specs/testing.md.
//!
//! Every compile here turns warnings into errors at a high level (`/W4`,
//! `-Wall -Wextra`), since the spec promises the headers raise none.

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;

use support::Project;
use support::compilers::{Compiler, Language, require_driver};

const COMPILERS: [Compiler; 3] = [Compiler::Msvc, Compiler::Clang, Compiler::ClangCl];

/// The headers in this repository, which cuno embeds and writes to the cuno
/// home unchanged (`src/bundled.rs`).
fn headers() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("headers")
}

#[derive(Clone, Copy, PartialEq)]
enum Std {
    C11,
    Cxx17,
    Cxx20,
}

impl Std {
    fn language(self) -> Language {
        match self {
            Std::C11 => Language::C,
            Std::Cxx17 | Std::Cxx20 => Language::Cxx,
        }
    }
}

/// How a file is compiled.
#[derive(Clone, Copy)]
struct Flags {
    std: Std,
    /// Whether `CUNO_TEST` is defined.
    test: bool,
    /// Whether C++ exceptions are enabled.
    exceptions: bool,
}

const TEST: Flags = Flags {
    std: Std::C11,
    test: true,
    exceptions: true,
};

/// The compile of `source` to `object` with `compiler`, or `None` when the
/// compiler is missing and that is allowed.
fn compile(compiler: Compiler, flags: Flags, source: &Path, object: &str) -> Option<Command> {
    let mut cmd = require_driver(compiler, flags.std.language())?;
    let cxx = flags.std.language() == Language::Cxx;
    let include = headers();
    match compiler {
        Compiler::Msvc | Compiler::ClangCl => {
            cmd.args(["/nologo", "/c", "/W4", "/WX"]);
            cmd.arg(format!("/I{}", include.display()));
            cmd.arg(match flags.std {
                Std::C11 => "/std:c11",
                Std::Cxx17 => "/std:c++17",
                Std::Cxx20 => "/std:c++20",
            });
            if cxx && flags.exceptions {
                cmd.arg("/EHsc");
            }
            if cxx && !flags.exceptions {
                cmd.arg("/D_HAS_EXCEPTIONS=0");
            }
            if flags.test {
                cmd.arg("/DCUNO_TEST");
            }
            cmd.arg(source).arg(format!("/Fo{object}"));
        }
        Compiler::Clang => {
            cmd.args(["-c", "-Wall", "-Wextra", "-Werror", "-I"]);
            cmd.arg(&include);
            cmd.arg(match flags.std {
                Std::C11 => "-std=c11",
                Std::Cxx17 => "-std=c++17",
                Std::Cxx20 => "-std=c++20",
            });
            if cxx && !flags.exceptions {
                cmd.arg("-fno-exceptions");
            }
            if flags.test {
                cmd.arg("-DCUNO_TEST");
            }
            cmd.arg(source).arg("-o").arg(object);
        }
    }
    Some(cmd)
}

/// Compiles each of `sources` (in the project) with its flags, and the
/// runtime when `runtime`, then links them into `exe`. Panics on any
/// compiler error or warning; `None` when the compiler is missing.
fn build(
    p: &Project,
    compiler: Compiler,
    sources: &[(&str, Flags)],
    runtime: bool,
    exe: &str,
) -> Option<PathBuf> {
    let main = headers().join("cuno").join("test_main.c");
    let mut all: Vec<(PathBuf, Flags)> = sources
        .iter()
        .map(|(source, flags)| (p.path().join(source), *flags))
        .collect();
    if runtime {
        all.push((main, TEST));
    }
    let mut objects = Vec::new();
    for (i, (source, flags)) in all.iter().enumerate() {
        let object = format!("{exe}-{i}.obj");
        let run = p.run_command(compile(compiler, *flags, source, &object)?);
        assert_eq!(
            run.code,
            Some(0),
            "{} {}:\n{}{}",
            compiler.name(),
            source.display(),
            run.stdout,
            run.stderr
        );
        objects.push(object);
    }
    let mut link = require_driver(compiler, Language::Cxx)?;
    match compiler {
        Compiler::Msvc | Compiler::ClangCl => {
            link.arg("/nologo")
                .args(&objects)
                .arg(format!("/Fe:{exe}.exe"));
        }
        Compiler::Clang => {
            link.args(&objects).arg("-o").arg(format!("{exe}.exe"));
        }
    }
    let run = p.run_command(link);
    assert_eq!(
        run.code,
        Some(0),
        "{} link:\n{}{}",
        compiler.name(),
        run.stdout,
        run.stderr
    );
    Some(p.path().join(format!("{exe}.exe")))
}

/// Runs one test of `exe` as cuno does, returning its exit code and its
/// result file, with each failure's file reduced to its name (compilers
/// spell `__FILE__` differently).
fn exact(p: &Project, exe: &Path, name: &str) -> (Option<i32>, String) {
    let result = p.path().join("result.txt");
    let _ = std::fs::remove_file(&result);
    let mut cmd = Command::new(exe);
    cmd.args(["--exact", name]);
    let run = p.run_command_with(cmd, &[("CUNO_TEST_RESULT", result.to_str().unwrap())]);
    let text = std::fs::read_to_string(&result).unwrap_or_default();
    let events: Vec<String> = text
        .lines()
        .map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            if fields[0] == "fail" {
                let file = fields[1].rsplit(['/', '\\']).next().unwrap();
                format!("fail {file}:{} {}", fields[2], fields[3])
            } else {
                fields.join(" ")
            }
        })
        .collect();
    (run.code, events.join("\n"))
}

fn list(p: &Project, exe: &Path) -> String {
    let run = p.run(exe, &["--list"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    run.stdout.replace("\r\n", "\n")
}

const C_TESTS: &str = r#"#include <cuno/test.h>

static int add(int a, int b) { return a + b; }

cuno_test(c_passes) {
    cuno_check_eq(add(1, 2), 3);
    cuno_check(add(1, 1) == 2);
    cuno_check_false(add(1, 1) == 3);
    cuno_check_approx(0.1 + 0.2, 0.3, 1e-9);
    cuno_check_lt(-1, 1u);
    cuno_check_ne((const char *)0, "x");
}

cuno_test(c_check_goes_on) {
    cuno_info("context %d", 7);
    cuno_check_eq(add(1, 2), 4);
    cuno_check_ge(1, 2);
}

cuno_test(c_require_ends) {
    cuno_require_eq(add(0, 0), 1);
    cuno_check(1 == 2);
}

cuno_test(c_skips) {
    cuno_skip("not today");
}
"#;

const CXX17_TESTS: &str = r#"#include <cuno/test.h>
#include <vector>

cuno_test(cxx17_passes) {
    std::vector<int> v{1};
    cuno_check_eq(v.size(), 1u);
    cuno_check_eq(nullptr, nullptr);
}

cuno_test(cxx17_fails) {
    cuno_require_false(true);
}
"#;

const CXX20_TESTS: &str = r#"#include <cuno/test.hpp>
#include <string>
#include <vector>

static cuno::test::suite tests = [] {
    using namespace cuno::test;
    test("cxx20 passes", [] {
        std::vector<int> v{1, 2, 3};
        expect(eq(v.size(), 3));
        expect(true);
        expect(approx(0.1 + 0.2, 0.3, 1e-9));
        expect(ne(std::string("a"), "b"));
    });
    test("cxx20 check goes on", [] {
        info("n = ", 42);
        expect(eq(std::string("abc"), "abd"));
        expect(lt(2, 1));
    });
    test("cxx20 require ends", [] {
        require(false);
        expect(false);
    });
    test("cxx20 skips", [] { skip("later"); });
#if defined(__cpp_exceptions) || defined(_CPPUNWIND)
    test("cxx20 exceptions", [] {
        expect(throws<int>([] { throw 1; }));
        expect(nothrow([] {}));
        expect(throws<int>([] {}));
    });
#endif
};
"#;

/// Every test, in C, C++17 and C++20 in one executable (#80), is listed
/// once, runs alone, and reports through the result file: failures with
/// their place and values, `info`, skips, a `require` that ends the test, and
/// the exit code.
///
/// Mutations, each failing this test:
/// - register nothing in `cuno_test` or in `test` (the list is short);
/// - drop the Clang branch's `constructor` attribute (Clang registers no C
///   test);
/// - let `cuno_test_compare` or `detail::compare` pass a failed comparison;
/// - let a fatal failure return (the check after a `require` reports);
/// - write `done` before running the test, or not at all;
/// - compare a negative signed value as unsigned (`-1 < 1u` fails).
#[test]
fn tests_register_run_and_report_on_every_compiler() {
    for compiler in COMPILERS {
        let p = Project::new()
            .file("c_tests.c", C_TESTS)
            .file("cxx17_tests.cpp", CXX17_TESTS)
            .file("cxx20_tests.cpp", CXX20_TESTS);
        let c = TEST;
        let cxx17 = Flags {
            std: Std::Cxx17,
            ..TEST
        };
        let cxx20 = Flags {
            std: Std::Cxx20,
            ..TEST
        };
        let Some(exe) = build(
            &p,
            compiler,
            &[
                ("c_tests.c", c),
                ("cxx17_tests.cpp", cxx17),
                ("cxx20_tests.cpp", cxx20),
            ],
            true,
            "tests",
        ) else {
            continue;
        };
        let who = compiler.name();
        assert_eq!(
            list(&p, &exe),
            "c_passes\nc_check_goes_on\nc_require_ends\nc_skips\n\
             cxx17_passes\ncxx17_fails\n\
             cxx20 passes\ncxx20 check goes on\ncxx20 require ends\ncxx20 skips\n\
             cxx20 exceptions\n",
            "{who}"
        );
        let expected: [(&str, i32, &str); 11] = [
            ("c_passes", 0, "done"),
            (
                "c_check_goes_on",
                1,
                "info context 7\n\
                 fail c_tests.c:16 eq(add(1, 2), 4) failed\\n  left:  3\\n  right: 4\n\
                 fail c_tests.c:17 ge(1, 2) failed\\n  left:  1\\n  right: 2\n\
                 done",
            ),
            (
                "c_require_ends",
                1,
                "fail c_tests.c:21 eq(add(0, 0), 1) failed\\n  left:  0\\n  right: 1\ndone",
            ),
            ("c_skips", 0, "skip not today"),
            ("cxx17_passes", 0, "done"),
            (
                "cxx17_fails",
                1,
                "fail cxx17_tests.cpp:11 require_false(true) failed\ndone",
            ),
            ("cxx20 passes", 0, "done"),
            (
                "cxx20 check goes on",
                1,
                "info n = 42\n\
                 fail cxx20_tests.cpp:16 eq failed\\n  left:  \"abc\"\\n  right: \"abd\"\n\
                 fail cxx20_tests.cpp:17 lt failed\\n  left:  2\\n  right: 1\n\
                 done",
            ),
            (
                "cxx20 require ends",
                1,
                "fail cxx20_tests.cpp:20 require failed\ndone",
            ),
            ("cxx20 skips", 0, "skip later"),
            (
                "cxx20 exceptions",
                1,
                "fail cxx20_tests.cpp:28 throws failed: nothing was thrown\ndone",
            ),
        ];
        for (name, code, events) in expected {
            assert_eq!(
                exact(&p, &exe, name),
                (Some(code), events.to_owned()),
                "{who}: {name}"
            );
        }
    }
}

/// `require` ends a test with exceptions disabled, as games build
/// (testing.md, "Common rules").
///
/// Mutation: end a fatal failure by throwing; the file does not compile
/// without exceptions, or the check after `require` reports.
#[test]
fn require_ends_a_test_without_exceptions() {
    for compiler in COMPILERS {
        let p = Project::new().file("t.cpp", CXX20_TESTS);
        let flags = Flags {
            std: Std::Cxx20,
            test: true,
            exceptions: false,
        };
        let Some(exe) = build(&p, compiler, &[("t.cpp", flags)], true, "t") else {
            continue;
        };
        assert!(
            !list(&p, &exe).contains("exceptions"),
            "{}",
            compiler.name()
        );
        assert_eq!(
            exact(&p, &exe, "cxx20 require ends"),
            (Some(1), "fail t.cpp:20 require failed\ndone".to_owned()),
            "{}",
            compiler.name()
        );
    }
}

const SHARED_HPP: &str = r#"#include <cuno/test.hpp>
inline int twice(int x) { return 2 * x; }
static cuno::test::suite shared_tests = [] {
    cuno::test::test("twice doubles", [] { cuno::test::expect(cuno::test::eq(twice(2), 4)); });
};
"#;

/// A C++ test in a header that two files include is one test; two tests of
/// one name in different places are an error naming both (testing.md,
/// "Common rules").
///
/// Mutation: drop the same-place check in `cuno_test_first` (the test is
/// listed twice), or treat a name alone as the identity (the header's test
/// becomes an error, or the duplicate passes).
#[test]
fn tests_are_told_apart_by_name_and_place() {
    for compiler in COMPILERS {
        let p = Project::new()
            .file("shared.hpp", SHARED_HPP)
            .file("a.cpp", "#include \"shared.hpp\"\n")
            .file("b.cpp", "#include \"shared.hpp\"\n");
        let cxx20 = Flags {
            std: Std::Cxx20,
            ..TEST
        };
        let Some(exe) = build(
            &p,
            compiler,
            &[("a.cpp", cxx20), ("b.cpp", cxx20)],
            true,
            "shared",
        ) else {
            continue;
        };
        assert_eq!(list(&p, &exe), "twice doubles\n", "{}", compiler.name());
        assert_eq!(
            exact(&p, &exe, "twice doubles"),
            (Some(0), "done".to_owned())
        );

        let same = "#include <cuno/test.hpp>\n\
                    static cuno::test::suite s = [] { cuno::test::test(\"same\", [] {}); };\n";
        let p = Project::new()
            .file("a.cpp", same)
            .file("b.cpp", &format!("\n{same}"));
        let exe = build(
            &p,
            compiler,
            &[("a.cpp", cxx20), ("b.cpp", cxx20)],
            true,
            "same",
        )
        .unwrap();
        let run = p.run(&exe, &["--list"]);
        assert_eq!(run.code, Some(1), "{}", compiler.name());
        assert!(
            run.stderr
                .starts_with("error: two tests are named `same`: ")
                && run.stderr.contains("a.cpp:2 and ")
                && run.stderr.contains("b.cpp:3"),
            "{}: {}",
            compiler.name(),
            run.stderr
        );
    }
}

/// The macro API does not support a test in a header: compiled by two
/// files, it is an error that says what to do (testing.md, "Common rules").
///
/// Mutation: register macro tests as `test.hpp`'s are (the header's test is
/// listed once); this test fails.
#[test]
fn a_macro_test_in_a_header_is_an_error() {
    for compiler in COMPILERS {
        let p = Project::new()
            .file(
                "shared.h",
                "#include <cuno/test.h>\ncuno_test(in_header) {}\n",
            )
            .file("a.c", "#include \"shared.h\"\n")
            .file("b.c", "#include \"shared.h\"\n");
        let Some(exe) = build(&p, compiler, &[("a.c", TEST), ("b.c", TEST)], true, "h") else {
            continue;
        };
        let run = p.run(&exe, &["--list"]);
        assert_eq!(run.code, Some(1), "{}", compiler.name());
        assert!(
            run.stderr.starts_with("error: the test `in_header` at ")
                && run.stderr.contains(
                    "help: cuno_test does not support tests in headers; move it into a source \
                     file, or use cuno/test.hpp"
                ),
            "{}: {}",
            compiler.name(),
            run.stderr
        );
    }
}

/// Without `CUNO_TEST`, a file holding tests compiles with no warning and no
/// test code: it links without the runtime (testing.md, "Common rules").
///
/// Mutation: register in a normal build (the link fails on the runtime's
/// missing symbols), or call the suite's function without `CUNO_TEST`.
#[test]
fn a_normal_build_compiles_no_test() {
    let program = "int main(void) { return add(1, 2) == 3 ? 0 : 1; }\n";
    for compiler in COMPILERS {
        let p = Project::new()
            .file("c.c", &format!("{C_TESTS}{program}"))
            .file("cxx17.cpp", &CXX17_TESTS.replace("cxx17_", "normal17_"))
            .file("cxx20.cpp", CXX20_TESTS);
        let normal = Flags {
            test: false,
            ..TEST
        };
        let Some(exe) = build(
            &p,
            compiler,
            &[
                ("c.c", normal),
                (
                    "cxx17.cpp",
                    Flags {
                        std: Std::Cxx17,
                        ..normal
                    },
                ),
                (
                    "cxx20.cpp",
                    Flags {
                        std: Std::Cxx20,
                        ..normal
                    },
                ),
            ],
            false,
            "normal",
        ) else {
            continue;
        };
        let run = p.run(&exe, &[]);
        assert_eq!(run.code, Some(0), "{}", compiler.name());
    }
}

const EDGE_CPP: &str = r#"#include <cuno/test.hpp>
#include <climits>
static cuno::test::suite tests = [] {
    using namespace cuno::test;
    test("signs", [] { expect(eq(-1, UINT_MAX)); expect(lt(-1, 0u)); });
    test("escapes", [] { info("a\tb\\c\rd"); });
    test("floats", [] { expect(approx(1.5, 2.0, 0.25)); expect(eq(0.5, 0.25)); });
    test("null text", [] { const char *p = nullptr; expect(eq(p, "x")); });
    test("other exception", [] { expect(throws<int>([] { throw 1.0; })); });
};
"#;

const EDGE_C: &str = r#"#include <cuno/test.h>
cuno_test(c_floats) {
    cuno_check_approx(1.5, 2.0, 0.25);
    cuno_check_eq(0.5, 0.25);
}
"#;

/// The values a failure shows, and the result file's escaping, exactly.
///
/// Mutations, each failing this test:
/// - compare C++ integers after converting one to the other's type, so
///   `eq(-1, UINT_MAX)` passes;
/// - drop the escaping of a tab, CR or backslash in `cuno_test_field` (the
///   line splits where cuno reads a field);
/// - show floating-point values, or `approx`'s two sides, any other way;
/// - show a null `const char *` as an empty string;
/// - report an exception of another type in `throws` as anything else.
#[test]
fn failures_show_their_values_and_fields_are_escaped() {
    for compiler in COMPILERS {
        let p = Project::new()
            .file("edge.cpp", EDGE_CPP)
            .file("edge.c", EDGE_C);
        let cxx20 = Flags {
            std: Std::Cxx20,
            ..TEST
        };
        let Some(exe) = build(
            &p,
            compiler,
            &[("edge.cpp", cxx20), ("edge.c", TEST)],
            true,
            "edge",
        ) else {
            continue;
        };
        let expected: [(&str, i32, &str); 6] = [
            (
                "signs",
                1,
                "fail edge.cpp:5 eq failed\\n  left:  -1\\n  right: 4294967295\ndone",
            ),
            ("escapes", 0, "info a\\tb\\\\c\\rd\ndone"),
            (
                "floats",
                1,
                "fail edge.cpp:7 approx failed\\n  left:  1.5\\n  right: 2\n\
                 fail edge.cpp:7 eq failed\\n  left:  0.5\\n  right: 0.25\n\
                 done",
            ),
            (
                "null text",
                1,
                "fail edge.cpp:8 eq failed\\n  left:  NULL\\n  right: \"x\"\ndone",
            ),
            (
                "other exception",
                1,
                "fail edge.cpp:9 throws failed: an exception of another type was thrown\ndone",
            ),
            (
                "c_floats",
                1,
                "fail edge.c:3 approx(1.5, 2.0, 0.25) failed\\n  left:  1.5\\n  right: 2\n\
                 fail edge.c:4 eq(0.5, 0.25) failed\\n  left:  0.5\\n  right: 0.25\n\
                 done",
            ),
        ];
        for (name, code, events) in expected {
            assert_eq!(
                exact(&p, &exe, name),
                (Some(code), events.to_owned()),
                "{}: {name}",
                compiler.name()
            );
        }
    }
}

/// A test whose name is empty or spans lines would break the `--list`
/// protocol, so the executable refuses to start (testing.md, "Contract
/// between cuno and test executables").
///
/// Mutation: drop the name check in `cuno_test_check_registry`; this test
/// fails.
#[test]
fn a_test_name_must_be_one_nonempty_line() {
    for compiler in COMPILERS {
        let p = Project::new().file(
            "names.cpp",
            "#include <cuno/test.hpp>\n\
             static cuno::test::suite s = [] {\n\
             cuno::test::test(\"\", [] {});\n\
             cuno::test::test(\"two\\nlines\", [] {});\n\
             };\n",
        );
        let cxx20 = Flags {
            std: Std::Cxx20,
            ..TEST
        };
        let Some(exe) = build(&p, compiler, &[("names.cpp", cxx20)], true, "names") else {
            continue;
        };
        let run = p.run(&exe, &["--list"]);
        assert_eq!(run.code, Some(1), "{}", compiler.name());
        let says = "has a name that is empty or spans lines";
        assert_eq!(
            run.stderr.matches(says).count(),
            2,
            "{}: {}",
            compiler.name(),
            run.stderr
        );
        assert!(
            run.stderr.contains("names.cpp:3 ") && run.stderr.contains("names.cpp:4 "),
            "{}: {}",
            compiler.name(),
            run.stderr
        );
    }
}

/// Run by hand, a test executable says how it is used, refuses a name it
/// does not have, and without `CUNO_TEST_RESULT` writes the result to
/// stderr.
///
/// Mutations, each failing this test: return 0 for an unknown name; open
/// `CUNO_TEST_RESULT` without checking it is set (the run crashes or writes
/// nothing); accept no arguments.
#[test]
fn a_test_executable_run_by_hand() {
    for compiler in COMPILERS {
        let p = Project::new().file("edge.cpp", EDGE_CPP);
        let cxx20 = Flags {
            std: Std::Cxx20,
            ..TEST
        };
        let Some(exe) = build(&p, compiler, &[("edge.cpp", cxx20)], true, "hand") else {
            continue;
        };
        let who = compiler.name();
        // The C runtime ends lines with CRLF on Windows.
        let unknown = p.run(&exe, &["--exact", "nope"]);
        assert_eq!(
            (unknown.code, unknown.stderr.replace("\r\n", "\n")),
            (Some(2), "error: no test named `nope`\n".to_owned()),
            "{who}"
        );
        let usage = p.run(&exe, &[]);
        assert_eq!(usage.code, Some(2), "{who}");
        assert!(
            usage
                .stderr
                .replace("\r\n", "\n")
                .starts_with("usage: --list | --exact <name>\n"),
            "{who}"
        );
        let run = p.run(&exe, &["--exact", "escapes"]);
        assert_eq!(
            (run.code, run.stderr.replace("\r\n", "\n")),
            (Some(0), "info\ta\\tb\\\\c\\rd\ndone\n".to_owned()),
            "{who}"
        );
    }
}

/// `cuno build` writes the bundled headers to the cuno home and puts them on
/// the include path, so a normal build of a file including `cuno/test.h`
/// succeeds (build.md, "Default include paths").
///
/// Mutation: drop `bundled::expand` from the build, or the bundled
/// directory from the include path; this test fails.
#[test]
fn a_build_finds_the_bundled_headers() {
    for (name, compiler) in [
        ("msvc", Compiler::Msvc),
        ("clang", Compiler::Clang),
        ("clang-cl", Compiler::ClangCl),
    ] {
        if require_driver(compiler, Language::C).is_none() {
            continue;
        }
        let p = Project::new()
            .file(
                "cuno.toml",
                "[package]\nname = \"app\"\nversion = \"0.1.0\"\nlang = \"c\"\n",
            )
            .file(
                "src/main.c",
                "#include <stdio.h>\n#include <cuno/test.h>\n\
                 cuno_test(t) { cuno_check(1); }\n\
                 int main(void) { puts(\"ran\"); return 0; }\n",
            );
        let run = p.cuno(&["-q", "run", "--compiler", name]);
        // The program's own line ending is the C runtime's: CRLF on Windows.
        assert_eq!(
            (
                run.code,
                run.stdout.replace("\r\n", "\n"),
                run.stderr.as_str()
            ),
            (Some(0), "ran\n".to_owned(), ""),
            "{name}"
        );
        let dir = p
            .home()
            .join("include")
            .join(env!("CARGO_PKG_VERSION"))
            .join("cuno");
        for file in ["test.h", "test.hpp", "test_main.c"] {
            assert_eq!(
                std::fs::read(dir.join(file)).unwrap(),
                std::fs::read(headers().join("cuno").join(file)).unwrap(),
                "{file}"
            );
        }
    }
}
