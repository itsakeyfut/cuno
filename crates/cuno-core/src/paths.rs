//! Whether two paths are the same place, or one lies inside the other: the
//! one answer every crate asks (`docs/en/specs/project-layout.md`, "Source
//! collection and linking"; `docs/en/specs/architecture.md`, "What goes in
//! `cuno-core`"). Only comparing uses where a path leads; the path a caller
//! goes on to use or show stays as written.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf, Prefix};

use crate::NameCase;
use crate::env::fold;

/// `path` with `.` dropped and each `..` taking the part before it away.
pub fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// `path` as the file system names it, whether or not it exists yet: `.`
/// and `..` resolved, and its longest existing part canonicalized, so a
/// link or a short name compares as where it leads; the part that does not
/// exist yet is kept as written. A relative path is only resolved by name,
/// since a library does not read the process's current directory.
pub fn real(path: &Path) -> PathBuf {
    let path = lexical(path);
    if !path.is_absolute() {
        return path;
    }
    let mut existing = path.as_path();
    let mut rest = Vec::new();
    loop {
        if let Ok(canonical) = std::fs::canonicalize(existing) {
            return rest.iter().rev().fold(canonical, |p, part| p.join(part));
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_owned());
                existing = parent;
            }
            _ => return path,
        }
    }
}

/// What every spelling of one place shares: [`real`], with `/` between its
/// parts, a `\\?\` prefix written as the plain one, and names folded when
/// `case` says the file system ignores case.
pub fn key(path: &Path, case: NameCase) -> String {
    parts(&real(path), case).join("/")
}

/// Whether `a` and `b` are the same place.
pub fn same(a: &Path, b: &Path, case: NameCase) -> bool {
    key(a, case) == key(b, case)
}

/// Whether `path` is `dir` or lies under it.
pub fn within(path: &Path, dir: &Path, case: NameCase) -> bool {
    relative(path, dir, case).is_some()
}

/// Where `path` lies under `dir`, as the file system names it: the parts of
/// [`real`]`(path)` after those of `real(dir)`, empty when the two are the
/// same place, or `None` when `path` is not inside `dir`.
pub fn relative(path: &Path, dir: &Path, case: NameCase) -> Option<PathBuf> {
    let (path, dir) = (real(path), real(dir));
    let (inner, outer) = (parts(&path, case), parts(&dir, case));
    if inner.len() < outer.len() || inner[..outer.len()] != outer[..] {
        return None;
    }
    Some(path.components().skip(outer.len()).collect())
}

/// Each component of `path` as compared: a drive or share prefix in its
/// plain form, the root as an empty part, and names folded per `case`.
fn parts(path: &Path, case: NameCase) -> Vec<String> {
    path.components()
        .map(|component| match component {
            Component::Prefix(prefix) => match prefix.kind() {
                Prefix::VerbatimDisk(d) | Prefix::Disk(d) => {
                    fold(&OsString::from(format!("{}:", char::from(d))), case)
                }
                Prefix::VerbatimUNC(server, share) | Prefix::UNC(server, share) => {
                    let mut unc = OsString::from("//");
                    unc.push(server);
                    unc.push("/");
                    unc.push(share);
                    fold(&unc, case)
                }
                _ => fold(prefix.as_os_str(), case),
            },
            Component::RootDir => OsString::new(),
            other => fold(other.as_os_str(), case),
        })
        .map(|part| part.to_string_lossy().into_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh, empty directory for one test.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cuno-paths-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Mutation: compare without folding case; this test fails.
    #[test]
    fn case_is_ignored_only_where_the_file_system_ignores_it() {
        let a = Path::new("/project/Src/Main.cpp");
        let b = Path::new("/project/src/main.cpp");
        assert!(same(a, b, NameCase::Insensitive));
        assert!(!same(a, b, NameCase::Sensitive));
    }

    /// Mutation: compare paths as written, without resolving `.` and `..`;
    /// this test fails.
    #[test]
    fn dot_and_dot_dot_are_resolved_by_name() {
        let case = NameCase::Sensitive;
        assert!(same(
            Path::new("/p/./src/../src/a.c"),
            Path::new("/p/src/a.c"),
            case
        ));
        assert!(within(Path::new("/p/src/../target"), Path::new("/p"), case));
        assert!(!within(Path::new("/p/../q"), Path::new("/p"), case));
    }

    /// Mutation: compare by string prefix instead of by component; this
    /// test fails.
    #[test]
    fn within_takes_whole_names() {
        let case = NameCase::Sensitive;
        let dir = Path::new("/a/b");
        assert!(within(Path::new("/a/b"), dir, case));
        assert!(within(Path::new("/a/b/c"), dir, case));
        assert!(!within(Path::new("/a/bc"), dir, case));
        assert!(!within(Path::new("/a"), dir, case));
        assert_eq!(
            relative(Path::new("/a/b/c/d"), dir, case),
            Some(PathBuf::from("c/d"))
        );
        assert_eq!(relative(Path::new("/a/b"), dir, case), Some(PathBuf::new()));
    }

    /// A part that does not exist yet is kept as written after the part
    /// that does, which is canonicalized.
    ///
    /// Mutation: drop the part that does not exist, or fall back to the
    /// path as written when any of it is missing; this test fails.
    #[test]
    fn a_path_not_there_yet_continues_from_the_part_that_is() {
        let dir = scratch("tail");
        let case = NameCase::host();
        let missing = dir.join("not").join("yet");
        assert!(real(&missing).ends_with("not/yet"));
        assert!(real(&missing).starts_with(std::fs::canonicalize(&dir).unwrap()));
        assert!(!same(&missing, &dir.join("not"), case));
        assert!(within(&missing, &dir, case));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A path written with `\\?\` and the same path without it are one
    /// place, even where nothing of it exists for `canonicalize` to
    /// reconcile (here, a drive that is not there).
    ///
    /// Mutation: keep a `\\?\` drive prefix as written; this test fails.
    #[cfg(windows)]
    #[test]
    fn a_verbatim_prefix_is_the_plain_one() {
        let drive = (b'D'..=b'Z')
            .map(char::from)
            .find(|d| !Path::new(&format!("{d}:\\")).exists())
            .expect("a free drive letter");
        let verbatim = PathBuf::from(format!(r"\\?\{drive}:\nowhere"));
        let plain = PathBuf::from(format!(r"{drive}:\nowhere"));
        assert!(same(&verbatim, &plain, NameCase::Insensitive));
    }

    /// A junction compares as the directory it leads to, and what lies
    /// under it as lying under that directory.
    ///
    /// Mutation: compare without following links (`real` returning
    /// [`lexical`]); this test fails.
    #[cfg(windows)]
    #[test]
    fn a_junction_is_where_it_leads() {
        let dir = scratch("junction");
        let target = dir.join("target");
        let link = dir.join("link");
        std::fs::create_dir(&target).unwrap();
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&target)
            .output()
            .unwrap();
        assert!(made.status.success(), "{made:?}");
        let case = NameCase::Insensitive;
        assert!(same(&link, &target, case));
        assert!(within(&link.join("debug"), &target, case));
        assert!(!within(&link, &dir.join("elsewhere"), case));
        std::fs::remove_dir(&link).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
