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

use support::compilers::{Compiler, Language, require_driver};
use support::{Project, Run};

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
    /// Whether to optimise across files at link time (`lto = true`).
    lto: bool,
}

const TEST: Flags = Flags {
    std: Std::C11,
    test: true,
    exceptions: true,
    lto: false,
};

const CXX20: Flags = Flags {
    std: Std::Cxx20,
    ..TEST
};

/// The compile of `source` to `object` with `compiler`, or `None` when the
/// compiler is missing and that is allowed. Sources are UTF-8, as cuno
/// compiles them (`/utf-8`).
fn compile(compiler: Compiler, flags: Flags, source: &Path, object: &str) -> Option<Command> {
    let mut cmd = require_driver(compiler, flags.std.language())?;
    let cxx = flags.std.language() == Language::Cxx;
    let include = headers();
    match compiler {
        Compiler::Msvc | Compiler::ClangCl => {
            cmd.args(["/nologo", "/c", "/W4", "/WX", "/utf-8"]);
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
            if flags.lto {
                cmd.args(match compiler {
                    Compiler::Msvc => ["/O2", "/GL"],
                    _ => ["/O2", "-flto"],
                });
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
            if cxx {
                // Warnings a user may well turn on, which a header has to
                // pass in their normal builds too.
                cmd.args(["-Wdouble-promotion", "-Wzero-as-null-pointer-constant"]);
            }
            if cxx && !flags.exceptions {
                cmd.arg("-fno-exceptions");
            }
            if flags.test {
                cmd.arg("-DCUNO_TEST");
            }
            if flags.lto {
                cmd.args(["-O2", "-flto"]);
            }
            cmd.arg(source).arg("-o").arg(object);
        }
    }
    Some(cmd)
}

/// Compiles each of `sources` (in the project) with its flags, and the
/// runtime when `runtime`, then links them into `exe`, returning the link's
/// result and the executable's path. Panics on any compiler error or
/// warning; `None` when the compiler is missing.
fn compile_and_link(
    p: &Project,
    compiler: Compiler,
    sources: &[(&str, Flags)],
    runtime: bool,
    exe: &str,
) -> Option<(Run, PathBuf)> {
    let main = headers().join("cuno").join("test_main.c");
    let mut all: Vec<(PathBuf, Flags)> = sources
        .iter()
        .map(|(source, flags)| (p.path().join(source), *flags))
        .collect();
    let lto = sources.iter().any(|(_, flags)| flags.lto);
    if runtime {
        all.push((main, Flags { lto, ..TEST }));
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
        Compiler::Msvc => {
            link.arg("/nologo")
                .args(&objects)
                .arg(format!("/Fe:{exe}.exe"));
            if lto {
                link.args(["/link", "/LTCG"]);
            }
        }
        Compiler::ClangCl => {
            link.arg("/nologo")
                .args(&objects)
                .arg(format!("/Fe:{exe}.exe"));
            if lto {
                link.arg("-fuse-ld=lld");
            }
        }
        Compiler::Clang => {
            link.args(&objects).arg("-o").arg(format!("{exe}.exe"));
            if lto {
                link.args(["-flto", "-fuse-ld=lld"]);
            }
        }
    }
    Some((p.run_command(link), p.path().join(format!("{exe}.exe"))))
}

/// [`compile_and_link`], panicking unless the link succeeds.
fn build(
    p: &Project,
    compiler: Compiler,
    sources: &[(&str, Flags)],
    runtime: bool,
    exe: &str,
) -> Option<PathBuf> {
    let (run, exe) = compile_and_link(p, compiler, sources, runtime, exe)?;
    assert_eq!(
        run.code,
        Some(0),
        "{} link:\n{}{}",
        compiler.name(),
        run.stdout,
        run.stderr
    );
    Some(exe)
}

/// Runs `exe` with `args` as cuno does, with `CUNO_TEST_RESULT` naming
/// `result`, returning its exit code and the records it wrote. Each record's
/// tab-separated fields are joined with spaces, and a file in a `fail` or
/// `test` record is reduced to its name (compilers spell `__FILE__`
/// differently). Also asserts that the file's lines end in LF alone.
fn records(p: &Project, exe: &Path, args: &[&str], result: &Path) -> (Option<i32>, String) {
    let _ = std::fs::remove_file(result);
    let mut cmd = Command::new(exe);
    cmd.args(args);
    let run = p.run_command_with(cmd, &[("CUNO_TEST_RESULT", result.to_str().unwrap())]);
    let bytes = std::fs::read(result).unwrap_or_default();
    assert!(
        !bytes.contains(&b'\r'),
        "{exe:?} {args:?}: a CR in the result file"
    );
    let text = String::from_utf8(bytes).expect("the result file is UTF-8");
    let events: Vec<String> = text
        .lines()
        .map(|line| {
            let mut fields: Vec<&str> = line.split('\t').collect();
            let file = match fields[0] {
                "fail" => 1,
                "test" => 2,
                _ => 0,
            };
            if file != 0 {
                fields[file] = fields[file].rsplit(['/', '\\']).next().unwrap();
            }
            if fields[0] == "fail" {
                // As a failure is shown: `file:line message`.
                return format!("fail {}:{} {}", fields[1], fields[2], fields[3]);
            }
            fields.join(" ")
        })
        .collect();
    (run.code, events.join("\n"))
}

/// Runs one test of `exe` as cuno does: [`records`] of `--exact name`.
fn exact(p: &Project, exe: &Path, name: &str) -> (Option<i32>, String) {
    records(p, exe, &["--exact", name], &p.path().join("result.txt"))
}

/// The `test` records `exe --list` writes, one per line.
fn list(p: &Project, exe: &Path) -> String {
    let (code, text) = records(p, exe, &["--list"], &p.path().join("list.txt"));
    assert_eq!(code, Some(0), "{exe:?} --list");
    text
}

/// The names `exe --list` reports, one per line.
fn names(p: &Project, exe: &Path) -> String {
    list(p, exe)
        .lines()
        .map(|record| {
            // `test <name> <file> <line> <api>`, where only the name may
            // hold spaces.
            let fields: Vec<&str> = record.rsplitn(4, ' ').collect();
            format!("{}\n", fields[3].strip_prefix("test ").unwrap())
        })
        .collect()
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

/// Every test, in C, C++17 and C++20 in one executable (a package
/// may mix the two languages, testing.md, "Common rules"), is listed
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
            names(&p, &exe),
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
            lto: false,
        };
        let Some(exe) = build(&p, compiler, &[("t.cpp", flags)], true, "t") else {
            continue;
        };
        assert!(
            !names(&p, &exe).contains("exceptions"),
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

/// `--list` writes a record for every registration, with the place it
/// names and the entry it came through, and leaves telling tests apart to
/// cuno (testing.md, "Common rules"): a C++ test in a header two files
/// include is listed twice from one place, and two tests of one name are
/// listed from their own places. `--exact` runs the first registration.
///
/// Mutation: list each name once, drop a field from the `test` record, or
/// mark a `test.hpp` registration `macro`; this test fails.
#[test]
fn every_registration_is_listed_with_its_place() {
    let same = "#include <cuno/test.hpp>\n\
                static cuno::test::suite s = [] { cuno::test::test(\"same\", [] {}); };\n";
    for compiler in COMPILERS {
        let p = Project::new()
            .file("shared.hpp", SHARED_HPP)
            .file("a.cpp", &format!("#include \"shared.hpp\"\n{same}"))
            .file("b.cpp", &format!("#include \"shared.hpp\"\n\n{same}"));
        let Some(exe) = build(
            &p,
            compiler,
            &[("a.cpp", CXX20), ("b.cpp", CXX20)],
            true,
            "shared",
        ) else {
            continue;
        };
        let who = compiler.name();
        let mut listed: Vec<&str> = Vec::new();
        let text = list(&p, &exe);
        listed.extend(text.lines());
        listed.sort_unstable();
        assert_eq!(
            listed,
            [
                "test same a.cpp 3 modern",
                "test same b.cpp 4 modern",
                "test twice doubles shared.hpp 4 modern",
                "test twice doubles shared.hpp 4 modern",
            ],
            "{who}"
        );
        assert_eq!(
            exact(&p, &exe, "twice doubles"),
            (Some(0), "done".to_owned()),
            "{who}"
        );
    }
}

/// The macro API's registrations are marked `macro`, so that cuno can
/// refuse one compiled more than once (a test in a header). On MSVC the
/// linker refuses it first: each C test's registration has a name of its
/// own there, kept through link-time optimisation (testing.md, "Common
/// rules").
///
/// Mutation: mark a macro registration `modern` (Clang and clang-cl), or
/// make the MSVC registration pointer static again (the header's test links
/// on MSVC); this test fails.
#[test]
fn a_macro_test_in_a_header_is_listed_or_refused() {
    for compiler in COMPILERS {
        let p = Project::new()
            .file(
                "shared.h",
                "#include <cuno/test.h>\ncuno_test(in_header) {}\n",
            )
            .file("a.c", "#include \"shared.h\"\n")
            .file("b.c", "#include \"shared.h\"\n");
        let Some((run, exe)) =
            compile_and_link(&p, compiler, &[("a.c", TEST), ("b.c", TEST)], true, "h")
        else {
            continue;
        };
        let who = compiler.name();
        if compiler == Compiler::Msvc {
            assert_ne!(run.code, Some(0), "{who}");
            assert!(
                run.stdout.contains("LNK2005")
                    && run.stdout.contains("cuno_test_initialiser_in_header"),
                "{who}: {}",
                run.stdout
            );
            continue;
        }
        assert_eq!(run.code, Some(0), "{who}: {}{}", run.stdout, run.stderr);
        assert_eq!(
            list(&p, &exe),
            "test in_header shared.h 2 macro\ntest in_header shared.h 2 macro",
            "{who}"
        );
    }
}

/// Every C and C++ test registers through link-time optimisation, which
/// drops what nothing refers to (`lto = true`, build.md). A C package is
/// built on its own: a C++ object in the same executable keeps MSVC's
/// initialiser section alive and would hide the loss.
///
/// Mutation: make the MSVC registration pointer static again (and drop its
/// `/include`); MSVC then lists no C test and this test fails.
#[test]
fn registration_survives_link_time_optimisation() {
    for compiler in COMPILERS {
        let p = Project::new()
            .file(
                "c.c",
                "#include <cuno/test.h>\ncuno_test(in_c) { cuno_check(1); }\n",
            )
            .file(
                "cxx.cpp",
                "#include <cuno/test.hpp>\n\
                 static cuno::test::suite s = [] { cuno::test::test(\"in cxx\", [] {}); };\n",
            );
        let c = Flags { lto: true, ..TEST };
        let cxx = Flags { lto: true, ..CXX20 };
        let Some(c_exe) = build(&p, compiler, &[("c.c", c)], true, "c") else {
            continue;
        };
        let Some(cxx_exe) = build(&p, compiler, &[("cxx.cpp", cxx)], true, "cxx") else {
            continue;
        };
        let who = compiler.name();
        assert_eq!(names(&p, &c_exe), "in_c\n", "{who}");
        assert_eq!(
            exact(&p, &c_exe, "in_c"),
            (Some(0), "done".to_owned()),
            "{who}"
        );
        assert_eq!(names(&p, &cxx_exe), "in cxx\n", "{who}");
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
        // The list, too, never goes to stdout, which is the user's.
        let listed = p.run(&exe, &["--list"]);
        assert_eq!(listed.code, Some(0), "{who}");
        assert_eq!(listed.stdout, "", "{who}");
        assert!(
            listed.stderr.contains("test\tescapes\t"),
            "{who}: {}",
            listed.stderr
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

const OPS_C: &str = r#"#include <math.h>
#include <cuno/test.h>
enum color { RED, GREEN };
cuno_test(c_ops) {
    char c = 'x';
    enum color e = GREEN;
    cuno_check_ne(1, 1);
    cuno_check_le(2, 1);
    cuno_check_gt(1, 2);
    cuno_check_lt(1.5f, 1.0f);
    cuno_check_eq(0.5L, 0.25L);
    cuno_check_eq(-5LL, 5ULL);
    cuno_check_eq(c, 'y');
    cuno_check_eq(e, RED);
    cuno_check_approx(1.0, 1.5, 0.5);
}
cuno_test(c_nan) {
    cuno_check_ne(NAN, NAN);
    cuno_check_eq(NAN, 1.0);
    cuno_check_le(NAN, 1.0);
    cuno_check_ge(NAN, 1.0);
}
cuno_test(c_require_ne) { cuno_require_ne(1, 1); cuno_check(0); }
cuno_test(c_require_approx) { cuno_require_approx(1.0, 2.0, 0.5); cuno_check(0); }
cuno_test(c_skip_after_failure) { cuno_check(0); cuno_skip("later"); }
"#;

const OPS_CPP: &str = r#"#include <cuno/test.hpp>
static cuno::test::suite tests = [] {
    using namespace cuno::test;
    test("cxx ops", [] {
        char c = 'a';
        expect(ne(1, 1));
        expect(le(2, 1));
        expect(gt(1, 2));
        expect(ge(1, 2));
        expect(eq(c, 'b'));
        expect(eq(c, 'a'));
        expect(approx(1.0, 1.5, 0.5));
        expect(ne(1.0, 1.0));
        expect(lt(2.0, 1.0));
        expect(le(2.0, 1.0));
        expect(gt(1.0, 2.0));
        expect(ge(1.0, 2.0));
    });
    test("cxx require ne", [] { require(ne(1, 1)); expect(false); });
    test("cxx skip after failure", [] { expect(false); skip("later"); });
};
"#;

/// Every comparison, in both entries, reports its own failure; a NaN is
/// equal to nothing in C; `approx` holds at its tolerance; C++ compares
/// characters and C compares enums; a `require` of any kind ends the test;
/// and a skip after a failure still fails (testing.md, "Contract between
/// cuno and test executables").
///
/// Mutations, each failing this test:
/// - point any `cuno_check_*` or `cuno_require_*` macro at another
///   operator, or `ne`, `le`, `gt`, `ge` in `test.hpp`;
/// - return SAME for an unordered pair in `cuno_test_order`;
/// - compare `approx` with `<` rather than `<=`;
/// - send a character through `std::cmp_equal` (C++ does not compile);
/// - match `x` itself rather than `1 ? (x) : (x)` in `CUNO_TEST_VALUE`
///   (MSVC does not compile the enum);
/// - map `float` or `long double` to another kind in the `_Generic` table;
/// - let `cuno_require_approx` go on, or `cuno_test_skip` exit 0 after a
///   failure.
#[test]
fn comparisons_report_exactly() {
    for compiler in COMPILERS {
        let p = Project::new().file("ops.c", OPS_C).file("ops.cpp", OPS_CPP);
        let Some(exe) = build(
            &p,
            compiler,
            &[("ops.c", TEST), ("ops.cpp", CXX20)],
            true,
            "ops",
        ) else {
            continue;
        };
        let who = compiler.name();
        let fail = |line: u32, what: &str, left: &str, right: &str| {
            format!("fail ops.c:{line} {what} failed\\n  left:  {left}\\n  right: {right}\n")
        };
        let c_ops = [
            fail(7, "ne(1, 1)", "1", "1"),
            fail(8, "le(2, 1)", "2", "1"),
            fail(9, "gt(1, 2)", "1", "2"),
            fail(10, "lt(1.5f, 1.0f)", "1.5", "1"),
            fail(11, "eq(0.5L, 0.25L)", "0.5", "0.25"),
            fail(12, "eq(-5LL, 5ULL)", "-5", "5"),
            fail(13, "eq(c, 'y')", "120", "121"),
            fail(14, "eq(e, RED)", "1", "0"),
        ]
        .concat();
        assert_eq!(
            exact(&p, &exe, "c_ops"),
            (Some(1), format!("{c_ops}done")),
            "{who}"
        );
        let (code, nan) = exact(&p, &exe, "c_nan");
        let failed: Vec<&str> = nan
            .lines()
            .filter(|l| l.starts_with("fail"))
            // The operator: NAN is a macro, so what follows is the C
            // library's spelling of it.
            .map(|l| &l.split(' ').nth(2).unwrap()[..3])
            .collect();
        assert_eq!(
            (code, failed),
            (Some(1), vec!["eq(", "le(", "ge("]),
            "{who}: {nan}"
        );
        assert_eq!(
            exact(&p, &exe, "c_require_ne"),
            (Some(1), format!("{}done", fail(23, "ne(1, 1)", "1", "1"))),
            "{who}"
        );
        assert_eq!(
            exact(&p, &exe, "c_require_approx"),
            (
                Some(1),
                format!("{}done", fail(24, "approx(1.0, 2.0, 0.5)", "1", "2"))
            ),
            "{who}"
        );
        assert_eq!(
            exact(&p, &exe, "c_skip_after_failure"),
            (
                Some(1),
                "fail ops.c:25 check(0) failed\nskip later".to_owned()
            ),
            "{who}"
        );

        let cxx = |line: u32, what: &str, left: &str, right: &str| {
            format!("fail ops.cpp:{line} {what} failed\\n  left:  {left}\\n  right: {right}\n")
        };
        let cxx_ops = [
            cxx(6, "ne", "1", "1"),
            cxx(7, "le", "2", "1"),
            cxx(8, "gt", "1", "2"),
            cxx(9, "ge", "1", "2"),
            cxx(10, "eq", "97", "98"),
            cxx(13, "ne", "1", "1"),
            cxx(14, "lt", "2", "1"),
            cxx(15, "le", "2", "1"),
            cxx(16, "gt", "1", "2"),
            cxx(17, "ge", "1", "2"),
        ]
        .concat();
        assert_eq!(
            exact(&p, &exe, "cxx ops"),
            (Some(1), format!("{cxx_ops}done")),
            "{who}"
        );
        assert_eq!(
            exact(&p, &exe, "cxx require ne"),
            (Some(1), format!("{}done", cxx(19, "ne", "1", "1"))),
            "{who}"
        );
        assert_eq!(
            exact(&p, &exe, "cxx skip after failure"),
            (
                Some(1),
                "fail ops.cpp:20 expect failed\nskip later".to_owned()
            ),
            "{who}"
        );
    }
}

/// A test named in characters outside the Windows code page can be listed
/// and chosen, and its result written under such a path (testing.md,
/// "Contract between cuno and test executables").
///
/// Mutation: read the arguments through `main` rather than `wmain`, or the
/// path through `getenv`/`fopen`; this test fails.
#[test]
fn names_and_paths_need_not_fit_the_code_page() {
    for compiler in COMPILERS {
        let p = Project::new().file(
            "u.cpp",
            "#include <cuno/test.hpp>\n\
             static cuno::test::suite s = [] { cuno::test::test(\"足し算 é\", [] {}); };\n",
        );
        let Some(exe) = build(&p, compiler, &[("u.cpp", CXX20)], true, "u") else {
            continue;
        };
        let who = compiler.name();
        assert_eq!(names(&p, &exe), "足し算 é\n", "{who}");
        let dir = p.path().join("dé");
        std::fs::create_dir(&dir).unwrap();
        assert_eq!(
            records(&p, &exe, &["--exact", "足し算 é"], &dir.join("r.txt")),
            (Some(0), "done".to_owned()),
            "{who}"
        );
    }
}

/// A test may be named like the runtime's own functions: the names the
/// macros make never are one.
///
/// Mutation: name the registration function `cuno_test_register_<name>`
/// again; `cuno_test(macro)` then does not compile.
#[test]
fn a_test_may_be_named_after_the_runtime() {
    for compiler in COMPILERS {
        let p = Project::new().file(
            "n.c",
            "#include <cuno/test.h>\n\
             cuno_test(macro) {}\ncuno_test(register) {}\ncuno_test(info) {}\n",
        );
        let Some(exe) = build(&p, compiler, &[("n.c", TEST)], true, "n") else {
            continue;
        };
        assert_eq!(
            names(&p, &exe),
            "macro\nregister\ninfo\n",
            "{}",
            compiler.name()
        );
    }
}
