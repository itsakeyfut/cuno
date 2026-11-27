//! Which tests of one executable cuno runs: its registrations told apart by
//! name and place, the place compared by the file system rather than by how
//! the compiler spelled it; in an executable made from `tests/`, only those
//! placed under `tests/`; and only those the filter names. See
//! `docs/en/specs/testing.md`, "Common rules" and "Execution".

use std::fmt;
use std::path::Path;

use cuno_core::NameCase;
use cuno_core::paths::{self, Place};

use crate::record::Registration;

/// Why an executable's list cannot be used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListError {
    /// One name at two places.
    SameName {
        /// The name.
        name: String,
        /// The two places, as shown (`src/a.cpp:3`).
        places: [String; 2],
    },
    /// A `test.hpp` test registered more than once from one place by one
    /// suite: a loop, or a function that declares it called twice.
    RegisteredTwice {
        /// The name.
        name: String,
        /// Its place, as shown.
        place: String,
    },
    /// A macro-API test registered more than once from one place: it is in
    /// a header several files include.
    InHeader {
        /// The name.
        name: String,
        /// Its place, as shown.
        place: String,
    },
}

impl fmt::Display for ListError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ListError::SameName { name, places } => write!(
                f,
                "two tests are named `{name}`: {} and {}",
                places[0], places[1]
            ),
            ListError::RegisteredTwice { name, place } => {
                write!(f, "test `{name}` is registered more than once from {place}")
            }
            ListError::InHeader { name, place } => {
                write!(f, "the test `{name}` at {place} is compiled more than once")
            }
        }
    }
}

impl ListError {
    /// How to fix it, for a `help:` line.
    pub fn help(&self) -> &'static str {
        match self {
            ListError::SameName { .. } => "give one of them another name",
            ListError::RegisteredTwice { .. } => {
                "give each registration its own name, such as with the loop's index"
            }
            ListError::InHeader { .. } => {
                "cuno_test does not support tests in headers; move it into a source file, or use cuno/test.hpp"
            }
        }
    }
}

/// `file` relative to `root` with `/` separators when it lies there, as
/// messages show a place; as spelled otherwise.
pub fn shown(file: &str, root: &Path, case: NameCase) -> String {
    match paths::relative(Path::new(file), root, case) {
        Some(relative) => relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
        None => file.to_owned(),
    }
}

/// The names to run, in the order first registered: every test of
/// `registrations` (or, for an executable made from `tests/`, those placed
/// under `root`'s `tests/`) whose name contains `filter`.
pub fn tests_to_run(
    registrations: &[Registration],
    integration: bool,
    filter: Option<&str>,
    root: &Path,
    case: NameCase,
) -> Result<Vec<String>, ListError> {
    let place = |r: &Registration| (Place::of(Path::new(&r.file), case), r.line);
    let at = |r: &Registration| format!("{}:{}", shown(&r.file, root, case), r.line);
    let tests_dir = Place::of(&root.join("tests"), case);
    let mut names: Vec<String> = Vec::new();
    for (i, r) in registrations.iter().enumerate() {
        let earlier = &registrations[..i];
        let Some(first) = earlier.iter().find(|e| e.name == r.name) else {
            names.push(r.name.clone());
            continue;
        };
        let (a, b) = (place(first), place(r));
        if !(a.0.same(&b.0) && a.1 == b.1) {
            return Err(ListError::SameName {
                name: r.name.clone(),
                places: [at(first), at(r)],
            });
        }
        // One place twice is one test for `test.hpp` when the copies come
        // from different suites, which is how a test in a header is meant
        // to be written; twice from one suite, the later one would never
        // run (`testing.md`, "Common rules").
        if r.suite != 0
            && earlier
                .iter()
                .any(|e| e.name == r.name && e.suite == r.suite)
        {
            return Err(ListError::RegisteredTwice {
                name: r.name.clone(),
                place: at(r),
            });
        }
        // The macro API does not support a test in a header, and says so.
        if first.macro_api || r.macro_api {
            return Err(ListError::InHeader {
                name: r.name.clone(),
                place: at(r),
            });
        }
    }
    Ok(names
        .into_iter()
        .filter(|name| {
            let r = registrations
                .iter()
                .find(|r| &r.name == name)
                .expect("a name comes from a registration");
            !integration || Place::of(Path::new(&r.file), case).within(&tests_dir)
        })
        .filter(|name| filter.is_none_or(|f| name.contains(f)))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A registration; a `test.hpp` one comes from a suite of its own, as a
    /// header's copies do, and a macro one from none.
    fn registration(name: &str, file: &str, line: u32, macro_api: bool) -> Registration {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
        let suite = if macro_api {
            0
        } else {
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        };
        in_suite(name, file, line, macro_api, suite)
    }

    fn in_suite(name: &str, file: &str, line: u32, macro_api: bool, suite: u32) -> Registration {
        Registration {
            name: name.into(),
            file: file.into(),
            line,
            macro_api,
            suite,
        }
    }

    /// One suite registering one name from one place twice is a test
    /// registered again, refused with the place; the same two from
    /// different suites are a header's copies, one test, even after a
    /// third copy repeats in its own suite.
    ///
    /// Mutation: compare without the suite, compare only with the first
    /// registration of the name, or merge as before; this test fails.
    #[test]
    fn one_suite_registering_one_place_twice_is_an_error() {
        let root = Path::new(ROOT);
        let twice = [
            in_suite("same", "C:/p/src/loop.cpp", 3, false, 1),
            in_suite("same", "C:/p/src/loop.cpp", 3, false, 1),
        ];
        let err = tests_to_run(&twice, false, None, root, CASE).unwrap_err();
        assert_eq!(
            err.to_string(),
            "test `same` is registered more than once from src/loop.cpp:3"
        );
        assert_eq!(
            err.help(),
            "give each registration its own name, such as with the loop's index"
        );

        let copies = [
            in_suite("shared", "C:/p/src/h.hpp", 3, false, 1),
            in_suite("shared", "C:/p/src/h.hpp", 3, false, 2),
        ];
        assert_eq!(
            tests_to_run(&copies, false, None, root, CASE).unwrap(),
            ["shared"]
        );
        let again_in_the_second = [
            in_suite("shared", "C:/p/src/h.hpp", 3, false, 1),
            in_suite("shared", "C:/p/src/h.hpp", 3, false, 2),
            in_suite("shared", "C:/p/src/h.hpp", 3, false, 2),
        ];
        assert!(matches!(
            tests_to_run(&again_in_the_second, false, None, root, CASE),
            Err(ListError::RegisteredTwice { .. })
        ));
    }

    const ROOT: &str = "C:/p";
    const CASE: NameCase = NameCase::Insensitive;

    /// A test registered by several files from one place is one test,
    /// however its file is spelled; one name at two places is an error
    /// naming both (`testing.md`, "Common rules").
    ///
    /// Mutation: compare files as spelled, keep a duplicate, or ignore a
    /// second place; this test fails.
    #[test]
    fn a_test_is_its_name_and_place() {
        let list = [
            registration("shared", "C:/p/src/util.h", 4, false),
            registration("shared", "C:/p/tests/../src/UTIL.h", 4, false),
            registration("own", "C:/p/src/a.cpp", 9, false),
        ];
        assert_eq!(
            tests_to_run(&list, false, None, Path::new(ROOT), CASE).unwrap(),
            ["shared", "own"]
        );
        let clash = [
            registration("same", "C:/p/src/a.cpp", 2, false),
            registration("same", "C:/p/src/b.cpp", 3, false),
        ];
        let err = tests_to_run(&clash, false, None, Path::new(ROOT), CASE).unwrap_err();
        assert_eq!(
            err.to_string(),
            "two tests are named `same`: src/a.cpp:2 and src/b.cpp:3"
        );
        assert_eq!(err.help(), "give one of them another name");
    }

    /// A macro-API test registered twice from one place is in a header,
    /// which that API does not support.
    ///
    /// Mutation: merge macro registrations as `test.hpp`'s are; this test
    /// fails.
    #[test]
    fn a_macro_test_in_a_header_is_an_error() {
        let list = [
            registration("in_header", "C:/p/src/h.h", 2, true),
            registration("in_header", "C:/p/src/h.h", 2, true),
        ];
        let err = tests_to_run(&list, false, None, Path::new(ROOT), CASE).unwrap_err();
        assert_eq!(
            err.to_string(),
            "the test `in_header` at src/h.h:2 is compiled more than once"
        );
        assert!(
            err.help()
                .starts_with("cuno_test does not support tests in headers")
        );
    }

    /// An executable made from `tests/` runs only the tests placed under
    /// `tests/`, so the in-source tests it also holds run once, from the
    /// in-source tests' executable (`testing.md`, "Execution").
    ///
    /// Mutation: run every registration from a `tests/` executable, or
    /// compare the place as spelled; this test fails.
    #[test]
    fn a_tests_executable_runs_its_own_tests() {
        let list = [
            registration("adds", "C:/p/Tests/adds.cpp", 3, false),
            registration("in source", "C:/p/src/calc.cpp", 7, false),
            registration("helper", "C:/p/tests/common/h.cpp", 1, false),
        ];
        assert_eq!(
            tests_to_run(&list, true, None, Path::new(ROOT), CASE).unwrap(),
            ["adds", "helper"]
        );
        assert_eq!(
            tests_to_run(&list, false, None, Path::new(ROOT), CASE).unwrap(),
            ["adds", "in source", "helper"]
        );
    }

    /// The filter keeps the names containing it.
    ///
    /// Mutation: ignore the filter, or match whole names only; this test
    /// fails.
    #[test]
    fn the_filter_keeps_names_containing_it() {
        let list = [
            registration("add works", "C:/p/src/a.cpp", 1, false),
            registration("parse", "C:/p/src/a.cpp", 2, false),
        ];
        assert_eq!(
            tests_to_run(&list, false, Some("add"), Path::new(ROOT), CASE).unwrap(),
            ["add works"]
        );
    }

    /// A place is a file and a line: the same name in the same file at
    /// another line, or at the same line of another file, is two tests; a
    /// pair of which only one came through the macro API is still refused
    /// as compiled twice, with the whole of its help.
    ///
    /// Mutation: compare files only or lines only, require both
    /// registrations to be macro ones, or cut the help short; this test
    /// fails.
    #[test]
    fn a_place_is_a_file_and_a_line() {
        let root = Path::new(ROOT);
        let lines = [
            registration("x", "C:/p/src/a.cpp", 1, false),
            registration("x", "C:/p/src/a.cpp", 2, false),
        ];
        assert!(matches!(
            tests_to_run(&lines, false, None, root, CASE),
            Err(ListError::SameName { .. })
        ));
        let files = [
            registration("x", "C:/p/src/a.cpp", 1, false),
            registration("x", "C:/p/src/b.cpp", 1, false),
        ];
        assert!(matches!(
            tests_to_run(&files, false, None, root, CASE),
            Err(ListError::SameName { .. })
        ));
        let mixed = [
            registration("x", "C:/p/src/h.h", 1, false),
            registration("x", "C:/p/src/h.h", 1, true),
        ];
        let err = tests_to_run(&mixed, false, None, root, CASE).unwrap_err();
        assert_eq!(
            err.help(),
            "cuno_test does not support tests in headers; move it into a source file, or use cuno/test.hpp"
        );
    }

    /// A place outside the root is shown as the compiler spelled it.
    ///
    /// Mutation: show nothing, or a path relative to somewhere else; this
    /// test fails.
    #[test]
    fn a_place_outside_the_root_is_shown_as_spelled() {
        assert_eq!(
            shown("D:/other/x.cpp", Path::new(ROOT), CASE),
            "D:/other/x.cpp"
        );
        assert_eq!(shown("C:/p/src/x.cpp", Path::new(ROOT), CASE), "src/x.cpp");
    }
}
