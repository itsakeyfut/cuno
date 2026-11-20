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
    /// Keyed by the name as compared (see `key`); the value keeps the name
    /// as it was spelled, for passing the environment on to a child process.
    vars: BTreeMap<OsString, (OsString, OsString)>,
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
            .map(|(name, value)| (key(&name, case), (name, value)))
            .collect();
        Env { case, vars }
    }

    /// The value of `name`, if it is set.
    pub fn get(&self, name: &str) -> Option<&OsStr> {
        self.vars
            .get(&key(OsStr::new(name), self.case))
            .map(|(_, value)| value.as_os_str())
    }

    /// Every variable, as (name as spelled, value), for giving a child
    /// process exactly this environment.
    pub fn vars(&self) -> impl Iterator<Item = (&OsStr, &OsStr)> {
        self.vars
            .values()
            .map(|(name, value)| (name.as_os_str(), value.as_os_str()))
    }

    /// This environment with `name` set to `value`, replacing any variable
    /// that has the same name as this environment compares names.
    pub fn with(&self, name: &str, value: OsString) -> Env {
        let mut env = self.clone();
        env.vars
            .insert(key(OsStr::new(name), self.case), (name.into(), value));
        env
    }

    /// This environment without the variables named in `names`, compared as
    /// this environment compares names.
    pub fn without(&self, names: &[&str]) -> Env {
        let mut env = self.clone();
        for name in names {
            env.vars.remove(&key(OsStr::new(name), self.case));
        }
        env
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

    /// Mutation: make `without` compare names case-sensitively, or make `vars`
    /// return the folded key instead of the spelling; this test fails.
    #[test]
    fn without_removes_by_name_and_vars_keeps_the_spelling() {
        let e = env(
            &[("Path", "C:\\bin"), ("cl", "/nologo")],
            NameCase::Insensitive,
        );
        let rest = e.without(&["CL"]);
        assert_eq!(rest.get("cl"), None);
        let vars: Vec<_> = rest.vars().collect();
        assert_eq!(vars, [(OsStr::new("Path"), OsStr::new("C:\\bin"))]);
    }

    /// Mutation: keep the first of two pairs with the same name; this test
    /// fails.
    /// Mutation: insert under the name as spelled instead of as compared;
    /// this test fails (`Path` would survive beside `PATH`).
    #[test]
    fn with_replaces_a_variable_spelled_differently() {
        let env = env(&[("Path", "old")], NameCase::Insensitive).with("path", "new".into());
        assert_eq!(env.get("PATH"), Some(OsStr::new("new")));
        assert_eq!(env.vars().count(), 1);
    }

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
