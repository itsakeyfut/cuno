//! Which of the package's code files `cuno fmt` and `cuno lint` work on:
//! the code minus what `exclude` names, with file links set apart, since
//! one can lead outside the project (`docs/en/specs/dev-tools.md`, "Target
//! files").

use std::path::{Path, PathBuf};

use cuno_core::{NameCase, paths};
use cuno_manifest::Glob;

/// The files a command works on.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Targets {
    /// The files that are not links, in the order given.
    pub files: Vec<PathBuf>,
    /// The files that are links, in the order given: read through, never
    /// written through.
    pub links: Vec<PathBuf>,
}

/// `code`, the package's code files, less those an `exclude` pattern
/// matches, the links set apart. Patterns match the path relative to
/// `root`, as [`relative`] writes it.
pub fn select(root: &Path, code: Vec<PathBuf>, exclude: &[Glob], case: NameCase) -> Targets {
    let mut targets = Targets::default();
    for path in code {
        let shown = relative(root, &path);
        if exclude.iter().any(|glob| glob.matches(&shown, case)) {
            continue;
        }
        let link = std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink());
        if link {
            targets.links.push(path);
        } else {
            targets.files.push(path);
        }
    }
    targets
}

/// `path` from the project `root`, with `/` between its names, as `exclude`
/// patterns are written and messages show it. A `root` spelled with `.` or
/// `..` is matched as written and then normalized.
pub fn relative(root: &Path, path: &Path) -> String {
    let from_root = path
        .strip_prefix(root)
        .or_else(|_| path.strip_prefix(paths::lexical(root)))
        .unwrap_or(path);
    from_root
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glob(pattern: &str) -> Glob {
        Glob::parse(pattern).unwrap()
    }

    /// A pattern matches the path from the root, whatever the case on
    /// Windows, and the root may be spelled with `..`; what it does not
    /// match is kept in order.
    ///
    /// Mutation: match the absolute path, drop the lexical fallback for
    /// the root, or keep an excluded file; this test fails.
    #[test]
    fn exclude_matches_the_path_from_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("p");
        for file in ["src/main.cpp", "src/vendor/v.cpp", "tests/t.cpp"] {
            let path = root.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "").unwrap();
        }
        let code: Vec<PathBuf> = ["src/main.cpp", "src/vendor/v.cpp", "tests/t.cpp"]
            .iter()
            .map(|f| root.join(f))
            .collect();
        let spelled = root.join("src").join("..");
        let targets = select(
            &spelled,
            code.clone(),
            &[glob("SRC/vendor/**")],
            NameCase::Insensitive,
        );
        assert_eq!(targets.files, [code[0].clone(), code[2].clone()]);
        assert!(targets.links.is_empty());
        assert_eq!(relative(&spelled, &code[2]), "tests/t.cpp");
    }

    /// A file link is set apart from the files, not dropped.
    ///
    /// Mutation: keep links among the files, or drop them; this test
    /// fails.
    #[cfg(windows)]
    #[test]
    fn a_file_link_is_set_apart() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        let real = root.join("src/main.cpp");
        std::fs::write(&real, "").unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("shared.cpp");
        std::fs::write(&target, "").unwrap();
        let link = root.join("src/shared.cpp");
        if std::os::windows::fs::symlink_file(&target, &link).is_err() {
            eprintln!("skipped: this account cannot make file links");
            return;
        }
        let targets = select(
            root,
            vec![real.clone(), link.clone()],
            &[],
            NameCase::Insensitive,
        );
        assert_eq!(targets.files, [real]);
        assert_eq!(targets.links, [link]);
    }
}
