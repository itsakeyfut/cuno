//! Finding the project and resolving the paths written in its settings. See
//! `docs/en/specs/manifest.md`, "Locating the project and resolving paths".

use std::path::{Component, Path, PathBuf};

use cuno_core::NameCase;

use crate::ConfigError;

/// The name of a package's manifest (`docs/en/specs/manifest.md`, "File
/// layout"): every reader and writer of it joins this.
pub const MANIFEST: &str = "Cuno.toml";

/// What a directory holds as a manifest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManifestIn {
    /// Its manifest.
    Found(PathBuf),
    /// A file named as the manifest in another case (`cuno.toml`,
    /// `CUNO.TOML`) and no `Cuno.toml`, on a file system that tells them
    /// apart: an error, not a directory without a manifest.
    WrongCase(PathBuf),
    /// No manifest.
    None,
}

impl ManifestIn {
    /// The name of the file it found, as on disk: what a message about the
    /// directory's own package names.
    pub fn file_name(&self) -> Option<String> {
        match self {
            ManifestIn::Found(path) | ManifestIn::WrongCase(path) => {
                path.file_name().map(|n| n.to_string_lossy().into_owned())
            }
            ManifestIn::None => None,
        }
    }
}

/// The manifest `dir` holds, its file system telling names apart as `case`
/// says (`docs/en/specs/manifest.md`, "File layout"). Where names differ by
/// case, the directory is listed and its names compared exactly, so a name
/// in another case is told apart from the manifest's even when the file
/// system at hand would open either; a directory that cannot be listed is
/// asked for `Cuno.toml` itself, as it was before.
pub fn manifest_in(dir: &Path, case: NameCase) -> ManifestIn {
    let exact = dir.join(MANIFEST);
    let opened = || {
        if exact.is_file() {
            ManifestIn::Found(exact.clone())
        } else {
            ManifestIn::None
        }
    };
    if case == NameCase::Insensitive {
        return opened();
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return opened();
    };
    let names: Vec<String> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file() || t.is_symlink()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    if names.iter().any(|n| n == MANIFEST) && exact.is_file() {
        ManifestIn::Found(exact)
    } else if let Some(other) = names
        .iter()
        .find(|n| n.as_str() != MANIFEST && n.eq_ignore_ascii_case(MANIFEST))
    {
        ManifestIn::WrongCase(dir.join(other))
    } else {
        ManifestIn::None
    }
}

/// The error for a manifest named in another case, `file` in `dir`: the
/// search for the project stops there, rather than going on to one above
/// it (`docs/en/specs/manifest.md`, "File layout").
pub fn wrong_case(file: &Path) -> ConfigError {
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dir = file.parent().unwrap_or(file);
    ConfigError {
        message: format!(
            "found {name} in {}; the manifest is named {MANIFEST}",
            dir.display()
        ),
        location: None,
        help: Some(format!("rename it to {MANIFEST}")),
    }
}

/// A project: the directory holding its `Cuno.toml`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    /// The project root.
    pub root: PathBuf,
}

impl Project {
    /// The project's `Cuno.toml`.
    pub fn manifest(&self) -> PathBuf {
        self.root.join(MANIFEST)
    }

    /// The project-level `.cuno/config.toml`.
    pub fn config(&self) -> PathBuf {
        self.root.join(".cuno").join("config.toml")
    }
}

/// The nearest of `cwd` and its ancestors that holds a `Cuno.toml` file, or
/// `None` outside any project (not an error: `cuno doctor` and `cuno new`
/// run there). A manifest named in another case on the way stops the search with an
/// error rather than letting a project above it be used (`case`: how the
/// host's file system tells names apart). `cwd` is passed in by the binary,
/// which alone reads the process.
pub fn find_project(cwd: &Path, case: NameCase) -> Result<Option<Project>, ConfigError> {
    for dir in cwd.ancestors() {
        match manifest_in(dir, case) {
            ManifestIn::Found(_) => {
                return Ok(Some(Project {
                    root: dir.to_owned(),
                }));
            }
            ManifestIn::WrongCase(file) => return Err(wrong_case(&file)),
            ManifestIn::None => {}
        }
    }
    Ok(None)
}

/// `value` as written in a file or on the command line: kept when absolute,
/// otherwise joined to `base`, the directory its source is relative to (the
/// project root for `Cuno.toml` and `.cuno/config.toml`, the cuno home for
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
        touch(&outer.join("Cuno.toml"));
        assert_eq!(
            find_project(&deep, NameCase::host()).unwrap().unwrap().root,
            outer
        );
        assert_eq!(
            find_project(&outer, NameCase::host())
                .unwrap()
                .unwrap()
                .root,
            outer
        );
        touch(&inner.join("Cuno.toml"));
        assert_eq!(
            find_project(&deep, NameCase::host()).unwrap().unwrap().root,
            inner
        );
    }

    /// Where names differ by case, a lowercase `cuno.toml` stops the search
    /// with an error naming it, even with a project above; where they do not
    /// (Windows), it is the manifest.
    ///
    /// Mutation: skip a lowercase one and go on up, compare names without
    /// case where they differ, or refuse it where they do not; this test
    /// fails.
    #[test]
    fn a_lowercase_manifest_stops_the_search_where_case_matters() {
        let root = tempfile::tempdir().unwrap();
        let outer = root.path().join("outer");
        let inner = outer.join("inner");
        std::fs::create_dir_all(&inner).unwrap();
        touch(&outer.join(MANIFEST));
        touch(&inner.join("cuno.toml"));
        let err = find_project(&inner, NameCase::Sensitive).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!(
                "found cuno.toml in {}; the manifest is named Cuno.toml",
                inner.display()
            )
        );
        assert_eq!(err.help.as_deref(), Some("rename it to Cuno.toml"));
        assert_eq!(
            manifest_in(&outer, NameCase::Sensitive),
            ManifestIn::Found(outer.join(MANIFEST))
        );
        if cfg!(windows) {
            assert_eq!(
                find_project(&inner, NameCase::Insensitive)
                    .unwrap()
                    .unwrap()
                    .root,
                inner
            );
        }
    }

    /// Mutation: accept a directory named `Cuno.toml`, or report none when
    /// one is there; this test fails.
    #[test]
    fn only_a_cuno_toml_file_makes_a_project() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("p");
        std::fs::create_dir_all(dir.join("Cuno.toml")).unwrap();
        assert_eq!(find_project(&dir, NameCase::host()).unwrap(), None);
    }

    /// Mutation: point the manifest or the project config anywhere else; this
    /// test fails.
    #[test]
    fn the_project_files_are_at_the_root() {
        let project = Project {
            root: PathBuf::from("r"),
        };
        assert_eq!(project.manifest(), Path::new("r").join("Cuno.toml"));
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

    /// Any other case of the name stops the search, naming the file as it
    /// is; a directory named `Cuno.toml` is no manifest.
    ///
    /// Mutation: recognise only `cuno.toml`, name the file `cuno.toml`
    /// whatever its case, or count a directory; this test fails.
    #[test]
    fn any_other_case_stops_the_search_and_a_directory_is_no_manifest() {
        let root = tempfile::tempdir().unwrap();
        let outer = root.path().join("outer");
        let inner = outer.join("inner");
        std::fs::create_dir_all(&inner).unwrap();
        touch(&outer.join(MANIFEST));
        touch(&inner.join("CUNO.TOML"));
        let err = find_project(&inner, NameCase::Sensitive).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!(
                "found CUNO.TOML in {}; the manifest is named Cuno.toml",
                inner.display()
            )
        );

        let dir = root.path().join("d");
        std::fs::create_dir_all(dir.join(MANIFEST)).unwrap();
        assert_eq!(manifest_in(&dir, NameCase::Sensitive), ManifestIn::None);
    }
}
