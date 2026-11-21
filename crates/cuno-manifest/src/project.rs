//! Finding the project and resolving the paths written in its settings. See
//! `docs/en/specs/manifest.md`, "Locating the project and resolving paths".

use std::path::{Component, Path, PathBuf};

use crate::ConfigError;

/// A project: the directory holding its `cuno.toml`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    /// The project root.
    pub root: PathBuf,
}

impl Project {
    /// The project's `cuno.toml`.
    pub fn manifest(&self) -> PathBuf {
        self.root.join("cuno.toml")
    }

    /// The project-level `.cuno/config.toml`.
    pub fn config(&self) -> PathBuf {
        self.root.join(".cuno").join("config.toml")
    }
}

/// The nearest of `cwd` and its ancestors that holds a `cuno.toml` file, or
/// `None` outside any project (not an error: `cuno doctor` and `cuno new`
/// run there). `cwd` is passed in by the binary, which alone reads the
/// process.
pub fn find_project(cwd: &Path) -> Option<Project> {
    cwd.ancestors()
        .find(|dir| dir.join("cuno.toml").is_file())
        .map(|root| Project {
            root: root.to_owned(),
        })
}

/// `value` as written in a file or on the command line: kept when absolute,
/// otherwise joined to `base`, the directory its source is relative to (the
/// project root for `cuno.toml` and `.cuno/config.toml`, the cuno home for
/// the user-level config, the current directory for the command line).
///
/// Joined, not canonicalized, so a path through a symlink stays as written;
/// joining an absolute path gives that path. A Windows path rooted without a
/// drive (`/inc`) or with a drive and no root (`C:inc`) is an error, without
/// a location: the caller knows where it was written.
pub fn resolve_path(value: &Path, base: &Path) -> Result<PathBuf, ConfigError> {
    let drive_only = matches!(value.components().next(), Some(Component::Prefix(_)));
    if !value.is_absolute() && (value.has_root() || drive_only) {
        return Err(ConfigError {
            message: format!(
                "`{}` is neither absolute nor relative: where it points depends on the current drive",
                value.display()
            ),
            location: None,
            help: Some(
                "write an absolute path, or a path relative to the file it is in".to_owned(),
            ),
        });
    }
    Ok(base.join(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "").unwrap();
    }

    /// Mutation: look only in `cwd`, or take the farthest ancestor instead of
    /// the nearest; this test fails.
    #[test]
    fn the_nearest_project_above_the_current_directory_is_found() {
        let root = tempfile::tempdir().unwrap();
        let outer = root.path().join("outer");
        let inner = outer.join("a");
        let deep = inner.join("b").join("c");
        std::fs::create_dir_all(&deep).unwrap();
        touch(&outer.join("cuno.toml"));
        assert_eq!(find_project(&deep).unwrap().root, outer);
        assert_eq!(find_project(&outer).unwrap().root, outer);
        touch(&inner.join("cuno.toml"));
        assert_eq!(find_project(&deep).unwrap().root, inner);
    }

    /// Mutation: accept a directory named `cuno.toml`, or report none when
    /// one is there; this test fails.
    #[test]
    fn only_a_cuno_toml_file_makes_a_project() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("p");
        std::fs::create_dir_all(dir.join("cuno.toml")).unwrap();
        assert_eq!(find_project(&dir), None);
    }

    /// Mutation: point the manifest or the project config anywhere else; this
    /// test fails.
    #[test]
    fn the_project_files_are_at_the_root() {
        let project = Project {
            root: PathBuf::from("r"),
        };
        assert_eq!(project.manifest(), Path::new("r").join("cuno.toml"));
        assert_eq!(
            project.config(),
            Path::new("r").join(".cuno").join("config.toml")
        );
    }

    /// Mutation: ignore the base, or join an absolute path below it; this test
    /// fails.
    #[test]
    fn relative_paths_join_their_base_and_absolute_paths_stay() {
        let root = tempfile::tempdir().unwrap();
        let absolute = root.path().join("abs");
        for base in ["project", "home", "cwd"] {
            let base = root.path().join(base);
            assert_eq!(
                resolve_path(Path::new("inc/x"), &base).unwrap(),
                base.join("inc").join("x")
            );
            assert_eq!(resolve_path(&absolute, &base).unwrap(), absolute);
        }
    }

    /// Mutation: join a path rooted without a drive, or one with a drive and
    /// no root, instead of rejecting it; this test fails.
    #[cfg(windows)]
    #[test]
    fn windows_paths_that_depend_on_the_current_drive_are_errors() {
        let base = Path::new(r"C:\p\q");
        for value in [r"\inc", "/inc", "C:inc"] {
            let err = resolve_path(Path::new(value), base).unwrap_err();
            assert!(err.message.contains(value), "{err}");
            assert!(err.help.is_some());
        }
        assert_eq!(
            resolve_path(Path::new(r"D:\x"), base).unwrap(),
            Path::new(r"D:\x")
        );
    }
}
