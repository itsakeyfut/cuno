//! The headers and the test runtime bundled with cuno, embedded in the binary
//! and written to the cuno home, where every build includes them
//! (`docs/en/specs/distribution.md`, "cuno home"; `docs/en/specs/build.md`,
//! "Default include paths").

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use cuno_core::{CunoError, ExitClass};

/// Every bundled file, by its path under the include directory.
const FILES: [(&str, &str); 3] = [
    ("cuno/test.h", include_str!("../headers/cuno/test.h")),
    ("cuno/test.hpp", include_str!("../headers/cuno/test.hpp")),
    (
        "cuno/test_main.c",
        include_str!("../headers/cuno/test_main.c"),
    ),
];

/// The include directory of this version of cuno under `home`. Each version
/// has its own, so updating cuno does not change what an existing project
/// builds with.
pub fn include_dir(home: &Path) -> PathBuf {
    home.join("include").join(env!("CARGO_PKG_VERSION"))
}

/// Writes every bundled file under [`include_dir`] that is missing or
/// differs from this binary's, and returns the directory. A file that is
/// already right is not written, so its time stamp, which a build compares,
/// moves only when the content does.
pub fn expand(home: &Path) -> Result<PathBuf, ExpandError> {
    let dir = include_dir(home);
    for (name, content) in FILES {
        let path = dir.join(name);
        let current = std::fs::read(&path).ok();
        if current.as_deref() == Some(content.as_bytes()) {
            continue;
        }
        let write = || -> io::Result<()> {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, content)
        };
        write().map_err(|source| ExpandError { path, source })?;
    }
    Ok(dir)
}

/// A bundled file that could not be written.
#[derive(Debug)]
pub struct ExpandError {
    path: PathBuf,
    source: io::Error,
}

impl fmt::Display for ExpandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "cannot write `{}` in the cuno home: {}",
            self.path.display(),
            self.source
        )
    }
}

impl std::error::Error for ExpandError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

impl CunoError for ExpandError {
    /// The cuno home is part of the setup, as `CUNO_HOME` is.
    fn exit_class(&self) -> ExitClass {
        ExitClass::Setup
    }

    fn help(&self) -> Option<String> {
        Some("make the cuno home writable, or set CUNO_HOME to a directory that is".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mutation: leave a file out of `expand`'s loop, or skip writing one
    /// that differs; this test fails.
    #[test]
    fn expanding_writes_every_file_and_repairs_a_changed_one() {
        let home = tempfile::tempdir().unwrap();
        let dir = expand(home.path()).unwrap();
        assert_eq!(dir, include_dir(home.path()));
        for (name, content) in FILES {
            assert_eq!(std::fs::read_to_string(dir.join(name)).unwrap(), content);
        }
        std::fs::write(dir.join("cuno/test.h"), "changed").unwrap();
        expand(home.path()).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("cuno/test.h")).unwrap(),
            FILES[0].1
        );
    }

    /// Mutation: write every file on every run; the time stamp moves and
    /// this test fails.
    #[test]
    fn a_file_already_right_is_left_alone() {
        let home = tempfile::tempdir().unwrap();
        let dir = expand(home.path()).unwrap();
        let file = dir.join("cuno/test.h");
        let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        std::fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(old)
            .unwrap();
        expand(home.path()).unwrap();
        assert_eq!(std::fs::metadata(&file).unwrap().modified().unwrap(), old);
    }

    /// Mutation: map the error to another exit class; this test fails.
    #[test]
    fn an_unwritable_home_is_a_setup_problem() {
        let home = tempfile::tempdir().unwrap();
        // A file where the include directory has to be.
        std::fs::write(home.path().join("include"), "").unwrap();
        let err = expand(home.path()).unwrap_err();
        assert_eq!(err.exit_class(), ExitClass::Setup);
        assert!(err.to_string().starts_with("cannot write `"), "{err}");
    }
}
