//! The environment snapshot the binary passes to libraries, which never read
//! the process environment themselves (`docs/en/specs/architecture.md`,
//! "Dependencies between crates").

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};

/// Environment variables, looked up by name. On Windows names are
/// case-insensitive, as the operating system treats them, so `Path` and
/// `PATH` are the same variable.
#[derive(Clone, Debug, Default)]
pub struct Env {
    /// Keyed by the name as compared (upper-cased on Windows).
    vars: BTreeMap<OsString, OsString>,
}

impl Env {
    /// The environment of the running process. Only the binary calls this.
    pub fn from_process() -> Env {
        Env::from_pairs(std::env::vars_os())
    }

    /// An environment holding exactly `pairs`; a later pair replaces an
    /// earlier one with the same name.
    pub fn from_pairs(pairs: impl IntoIterator<Item = (OsString, OsString)>) -> Env {
        let vars = pairs
            .into_iter()
            .map(|(name, value)| (key(&name), value))
            .collect();
        Env { vars }
    }

    /// The value of `name`, if it is set.
    pub fn get(&self, name: &str) -> Option<&OsStr> {
        self.vars
            .get(&key(OsStr::new(name)))
            .map(OsString::as_os_str)
    }
}

/// The name as compared: upper-cased on Windows, where variable names are
/// case-insensitive; unchanged elsewhere.
fn key(name: &OsStr) -> OsString {
    if cfg!(windows) {
        // Names that are not valid Unicode keep their own spelling; Windows
        // itself only folds the case of letters it can read.
        match name.to_str() {
            Some(name) => name.to_uppercase().into(),
            None => name.to_owned(),
        }
    } else {
        name.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> Env {
        Env::from_pairs(pairs.iter().map(|(n, v)| (n.into(), v.into())))
    }

    /// Mutation: make `key` return the name unchanged on Windows; this test
    /// fails there.
    #[test]
    fn names_are_compared_as_the_platform_does() {
        let e = env(&[("Path", "C:\\bin")]);
        assert_eq!(e.get("Path"), Some(OsStr::new("C:\\bin")));
        if cfg!(windows) {
            assert_eq!(e.get("PATH"), Some(OsStr::new("C:\\bin")));
            assert_eq!(e.get("path"), Some(OsStr::new("C:\\bin")));
        } else {
            assert_eq!(e.get("PATH"), None);
        }
    }

    /// Mutation: keep the first of two pairs with the same name; this test
    /// fails.
    #[test]
    fn a_later_pair_replaces_an_earlier_one() {
        let e = env(&[("CUNO_X", "first"), ("CUNO_X", "second")]);
        assert_eq!(e.get("CUNO_X"), Some(OsStr::new("second")));
    }

    #[test]
    fn an_unset_variable_is_none() {
        assert_eq!(env(&[]).get("CUNO_X"), None);
    }
}
