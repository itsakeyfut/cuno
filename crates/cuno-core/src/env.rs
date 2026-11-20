//! The environment snapshot the binary passes to libraries, which never read
//! the process environment themselves (`docs/en/specs/architecture.md`,
//! "Dependencies between crates").

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};

/// How variable names are compared, which depends on the operating system the
/// variables came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameCase {
    /// `Path` and `PATH` are one variable (Windows).
    Insensitive,
    /// `Path` and `PATH` are two variables (other systems).
    Sensitive,
}

/// Environment variables, looked up by name.
#[derive(Clone, Debug)]
pub struct Env {
    case: NameCase,
    /// Keyed by the name as compared (see `key`).
    vars: BTreeMap<OsString, OsString>,
}

impl Default for Env {
    /// An empty environment that compares names as this platform does.
    fn default() -> Env {
        Env::from_pairs([], NameCase::host())
    }
}

impl NameCase {
    /// How the operating system cuno is running on compares names. Used only
    /// for the process's own environment ([`Env::from_process`]): the host is
    /// otherwise received as a value (`docs/en/specs/architecture.md`), and
    /// every other `Env` is built with an explicit `NameCase`, so both
    /// behaviours are tested on any system.
    pub fn host() -> NameCase {
        if cfg!(windows) {
            NameCase::Insensitive
        } else {
            NameCase::Sensitive
        }
    }
}

impl Env {
    /// The environment of the running process. Only the binary calls this.
    pub fn from_process() -> Env {
        Env::from_pairs(std::env::vars_os(), NameCase::host())
    }

    /// An environment holding exactly `pairs`, with names compared as `case`
    /// says; a later pair replaces an earlier one with the same name.
    pub fn from_pairs(
        pairs: impl IntoIterator<Item = (OsString, OsString)>,
        case: NameCase,
    ) -> Env {
        let vars = pairs
            .into_iter()
            .map(|(name, value)| (key(&name, case), value))
            .collect();
        Env { case, vars }
    }

    /// The value of `name`, if it is set.
    pub fn get(&self, name: &str) -> Option<&OsStr> {
        self.vars
            .get(&key(OsStr::new(name), self.case))
            .map(OsString::as_os_str)
    }
}

/// The name as compared. Case-insensitive names are upper-cased one character
/// at a time, keeping a character whose upper case is more than one character
/// (such as `ß`): Windows folds names character by character, so `ß` must not
/// become `SS`. Names that are not valid Unicode are kept as they are.
fn key(name: &OsStr, case: NameCase) -> OsString {
    match (case, name.to_str()) {
        (NameCase::Insensitive, Some(name)) => name
            .chars()
            .map(|c| {
                let mut upper = c.to_uppercase();
                match (upper.next(), upper.next()) {
                    (Some(u), None) => u,
                    _ => c,
                }
            })
            .collect::<String>()
            .into(),
        _ => name.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)], case: NameCase) -> Env {
        Env::from_pairs(pairs.iter().map(|(n, v)| (n.into(), v.into())), case)
    }

    /// Mutation: make `key` ignore `case`, in either direction; this test
    /// fails.
    #[test]
    fn names_are_compared_as_the_case_says() {
        let windows = env(&[("Path", "C:\\bin")], NameCase::Insensitive);
        assert_eq!(windows.get("PATH"), Some(OsStr::new("C:\\bin")));
        assert_eq!(windows.get("path"), Some(OsStr::new("C:\\bin")));
        let other = env(&[("Path", "/bin")], NameCase::Sensitive);
        assert_eq!(other.get("Path"), Some(OsStr::new("/bin")));
        assert_eq!(other.get("PATH"), None);
    }

    /// Mutation: upper-case with `str::to_uppercase` (which turns `ß` into
    /// `SS`); this test fails.
    #[test]
    fn folding_is_character_by_character() {
        let e = env(&[("STRAßE", "1")], NameCase::Insensitive);
        assert_eq!(e.get("straße"), Some(OsStr::new("1")));
        assert_eq!(e.get("STRASSE"), None);
    }

    /// Mutation: keep the first of two pairs with the same name; this test
    /// fails.
    #[test]
    fn a_later_pair_replaces_an_earlier_one() {
        let e = env(
            &[("CUNO_X", "first"), ("CUNO_X", "second")],
            NameCase::Sensitive,
        );
        assert_eq!(e.get("CUNO_X"), Some(OsStr::new("second")));
    }

    /// Mutation: make `from_process` return an empty environment; this test
    /// fails (`PATH` is set in every process the tests run in).
    #[test]
    fn from_process_reads_the_process_environment() {
        assert!(Env::from_process().get("PATH").is_some());
    }

    /// Mutation: make `NameCase::host` return the other variant; this test
    /// fails.
    #[test]
    fn the_host_case_matches_the_platform() {
        let expected = if cfg!(windows) {
            NameCase::Insensitive
        } else {
            NameCase::Sensitive
        };
        assert_eq!(NameCase::host(), expected);
    }
}
