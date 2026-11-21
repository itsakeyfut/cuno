//! Where the cuno home is (`docs/en/specs/distribution.md`, "cuno home
//! directory").

use std::fmt;
use std::path::{Path, PathBuf};

use crate::{CunoError, Env, ExitClass};

/// The cuno home: `CUNO_HOME` when set and not empty (a relative value joined
/// to `cwd`), else `.cuno` under `USERPROFILE`, else under `HOME`.
///
/// `USERPROFILE` comes first because Windows always sets it, while the `HOME`
/// an MSYS shell sets can be a Unix-style path; elsewhere only `HOME` is set.
pub fn cuno_home(env: &Env, cwd: &Path) -> Result<PathBuf, HomeError> {
    let set = |name| env.get(name).filter(|value| !value.is_empty());
    if let Some(home) = set("CUNO_HOME") {
        return Ok(cwd.join(home));
    }
    set("USERPROFILE")
        .or_else(|| set("HOME"))
        .map(|user| Path::new(user).join(".cuno"))
        .ok_or(HomeError)
}

/// Neither `CUNO_HOME` nor a user home directory is set.
#[derive(Debug, PartialEq, Eq)]
pub struct HomeError;

impl fmt::Display for HomeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "cannot tell where the cuno home is: none of CUNO_HOME, USERPROFILE and HOME is set",
        )
    }
}

impl std::error::Error for HomeError {}

impl CunoError for HomeError {
    fn exit_class(&self) -> ExitClass {
        ExitClass::Setup
    }

    fn help(&self) -> Option<String> {
        Some("set CUNO_HOME to the directory cuno should use".to_owned())
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
    /// `USERPROFILE`, accept an empty `CUNO_HOME`, or leave a relative one
    /// unjoined; this test fails.
    #[test]
    fn the_home_follows_cuno_home_then_the_user_directory() {
        let cwd = Path::new("work");
        let absolute = std::env::temp_dir().join("cuno-home");
        let cases: [(&[(&str, &str)], PathBuf); 5] = [
            (
                &[
                    ("CUNO_HOME", absolute.to_str().unwrap()),
                    ("USERPROFILE", "u"),
                ],
                absolute.clone(),
            ),
            (&[("CUNO_HOME", "rel")], cwd.join("rel")),
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
            assert_eq!(cuno_home(&env(pairs), cwd), Ok(expected), "{pairs:?}");
        }
    }

    /// Mutation: fall back to some directory instead of failing, or give the
    /// error another exit class or no help; this test fails.
    #[test]
    fn no_home_at_all_is_a_setup_problem_saying_what_to_set() {
        let err = cuno_home(&env(&[("HOME", "")]), Path::new("w")).unwrap_err();
        assert_eq!(err.exit_class(), ExitClass::Setup);
        assert!(err.help().unwrap().contains("CUNO_HOME"));
        assert!(err.to_string().contains("USERPROFILE"));
    }
}
