//! Where the cuno home is (`docs/en/specs/distribution.md`, "cuno home
//! directory").

use std::fmt;
use std::path::{Path, PathBuf};

use crate::{CunoError, Env, ExitClass};

/// The cuno home: `CUNO_HOME` when set and not empty, which must be
/// absolute, else `.cuno` under `USERPROFILE`, else under `HOME`.
///
/// A relative `CUNO_HOME` would move with the directory cuno runs in, and on
/// Windows `/x` and `C:x` depend on the current drive, so neither is taken.
/// `USERPROFILE` comes first because Windows always sets it, while the `HOME`
/// an MSYS shell sets can be a Unix-style path; elsewhere only `HOME` is set.
pub fn cuno_home(env: &Env) -> Result<PathBuf, HomeError> {
    let set = |name| env.get(name).filter(|value| !value.is_empty());
    if let Some(home) = set("CUNO_HOME") {
        let home = PathBuf::from(home);
        return if home.is_absolute() {
            Ok(home)
        } else {
            Err(HomeError::NotAbsolute(home))
        };
    }
    set("USERPROFILE")
        .or_else(|| set("HOME"))
        .map(|user| Path::new(user).join(".cuno"))
        .ok_or(HomeError::NotFound)
}

/// Why there is no cuno home.
#[derive(Debug, PartialEq, Eq)]
pub enum HomeError {
    /// Neither `CUNO_HOME` nor a user home directory is set.
    NotFound,
    /// `CUNO_HOME` is not an absolute path.
    NotAbsolute(PathBuf),
}

impl fmt::Display for HomeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HomeError::NotFound => f.write_str(
                "cannot tell where the cuno home is: none of CUNO_HOME, USERPROFILE and HOME is set",
            ),
            HomeError::NotAbsolute(path) => write!(
                f,
                "CUNO_HOME is `{}`, which is not an absolute path",
                path.display()
            ),
        }
    }
}

impl std::error::Error for HomeError {}

impl CunoError for HomeError {
    fn exit_class(&self) -> ExitClass {
        ExitClass::Setup
    }

    fn help(&self) -> Option<String> {
        Some(match self {
            HomeError::NotFound => "set CUNO_HOME to the directory cuno should use".to_owned(),
            HomeError::NotAbsolute(_) => {
                "set CUNO_HOME to an absolute path, including the drive on Windows".to_owned()
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NameCase;

    fn env(pairs: &[(&str, &str)]) -> Env {
        Env::from_pairs(
            pairs.iter().map(|(k, v)| ((*k).into(), (*v).into())),
            NameCase::Insensitive,
        )
    }

    /// Mutation: read the process environment, prefer `HOME` to
    /// `USERPROFILE`, or take an empty `CUNO_HOME`; this test fails.
    #[test]
    fn the_home_follows_cuno_home_then_the_user_directory() {
        let absolute = std::env::temp_dir().join("cuno-home");
        let absolute = absolute.to_str().unwrap();
        let cases: [(&[(&str, &str)], PathBuf); 4] = [
            (
                &[("CUNO_HOME", absolute), ("USERPROFILE", "u")],
                PathBuf::from(absolute),
            ),
            (
                &[("CUNO_HOME", ""), ("HOME", "h")],
                Path::new("h").join(".cuno"),
            ),
            (
                &[("USERPROFILE", "u"), ("HOME", "h")],
                Path::new("u").join(".cuno"),
            ),
            (&[("HOME", "h")], Path::new("h").join(".cuno")),
        ];
        for (pairs, expected) in cases {
            assert_eq!(cuno_home(&env(pairs)), Ok(expected), "{pairs:?}");
        }
    }

    /// Mutation: take a relative or drive-dependent `CUNO_HOME`; this test
    /// fails.
    #[test]
    fn cuno_home_must_be_absolute() {
        let mut values = vec!["rel", "./x"];
        if cfg!(windows) {
            values.extend(["/x", r"\x", "C:x"]);
        }
        for value in values {
            let err = cuno_home(&env(&[("CUNO_HOME", value), ("HOME", "h")])).unwrap_err();
            assert_eq!(err, HomeError::NotAbsolute(value.into()), "{value}");
            assert_eq!(
                err.to_string(),
                format!("CUNO_HOME is `{value}`, which is not an absolute path")
            );
            assert!(err.help().unwrap().contains("absolute path"));
        }
    }

    /// Mutation: fall back to some directory instead of failing, or give the
    /// error another exit class or no help; this test fails.
    #[test]
    fn no_home_at_all_is_a_setup_problem_saying_what_to_set() {
        let err = cuno_home(&env(&[("HOME", "")])).unwrap_err();
        assert_eq!(err, HomeError::NotFound);
        assert_eq!(err.exit_class(), ExitClass::Setup);
        assert!(err.help().unwrap().contains("CUNO_HOME"));
        assert!(err.to_string().contains("USERPROFILE"));
    }
}
