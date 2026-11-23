//! Whether two paths are the same place, or one lies inside the other: the
//! one answer every crate asks (`docs/en/specs/project-layout.md`, "Source
//! collection and linking"; `docs/en/specs/architecture.md`, "What goes in
//! `cuno-core`"). Only comparing uses where a path leads; the path a caller
//! goes on to use or show stays as written.

use std::ffi::{OsStr, OsString};
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

/// Where a path leads, ready to be compared many times: work out the
/// [`Place`] of each path once, then ask it as often as needed.
#[derive(Clone, Debug)]
pub struct Place {
    /// The path as the file system names it: see [`Place::of`].
    real: PathBuf,
    /// Each part of `real` as compared.
    parts: Vec<String>,
}

impl Place {
    /// Where `path` leads, whether or not it exists yet: `.` and `..`
    /// resolved, and its longest existing part canonicalized, so a link or a
    /// short name is where it leads; the part that does not exist yet is
    /// kept as written. The existing part is compared as the file system
    /// spells it, and only the rest is folded when `case` ignores case,
    /// since no folding table cuno could hold is the file system's own. A
    /// relative path is only resolved by name, since a library does not
    /// read the process's current directory.
    pub fn of(path: &Path, case: NameCase) -> Place {
        let (real, exact) = resolve(path);
        let parts = real
            .components()
            .enumerate()
            .map(|(i, component)| part(component, if i < exact { None } else { Some(case) }))
            .collect();
        Place { real, parts }
    }

    /// Whether `self` and `other` are the same place.
    pub fn same(&self, other: &Place) -> bool {
        self.parts == other.parts
    }

    /// Whether `self` is `dir` or lies under it.
    pub fn within(&self, dir: &Place) -> bool {
        self.relative(dir).is_some()
    }

    /// Where `self` lies under `dir`, as the file system names it: empty
    /// when the two are the same place, `None` when `self` is not inside.
    pub fn relative(&self, dir: &Place) -> Option<PathBuf> {
        self.parts
            .starts_with(&dir.parts)
            .then(|| self.real.components().skip(dir.parts.len()).collect())
    }

    /// The directory `self` is in.
    pub fn parent(&self) -> Option<Place> {
        let real = self.real.parent()?.to_owned();
        let parts = self.parts[..self.parts.len() - 1].to_vec();
        Some(Place { real, parts })
    }
}

/// Whether `a` and `b` are the same place: see [`Place`].
pub fn same(a: &Path, b: &Path, case: NameCase) -> bool {
    Place::of(a, case).same(&Place::of(b, case))
}

/// Whether `path` is `dir` or lies under it: see [`Place`].
pub fn within(path: &Path, dir: &Path, case: NameCase) -> bool {
    Place::of(path, case).within(&Place::of(dir, case))
}

/// Where `path` lies under `dir`: see [`Place::relative`].
pub fn relative(path: &Path, dir: &Path, case: NameCase) -> Option<PathBuf> {
    Place::of(path, case).relative(&Place::of(dir, case))
}

/// `path` resolved as [`Place::of`] says, and how many of its leading
/// components the file system spelled.
fn resolve(path: &Path) -> (PathBuf, usize) {
    let path = lexical(path);
    if !path.is_absolute() {
        return (path, 0);
    }
    let mut existing = path.as_path();
    let mut rest = Vec::new();
    loop {
        if let Ok(canonical) = std::fs::canonicalize(existing) {
            let exact = canonical.components().count();
            let real = rest.iter().rev().fold(canonical, |p, part| p.join(part));
            return (real, exact);
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_owned());
                existing = parent;
            }
            _ => return (path, 0),
        }
    }
}

/// One component as compared: a drive or share prefix in its plain form
/// (a drive letter in either case), the root as an empty part, and a name
/// folded per `case`, or kept when `case` is `None` because the file
/// system spelled it.
fn part(component: Component<'_>, case: Option<NameCase>) -> String {
    let folded = |name: &OsStr| match case {
        Some(case) => fold(name, case),
        None => name.to_owned(),
    };
    let part = match component {
        Component::Prefix(prefix) => match prefix.kind() {
            Prefix::VerbatimDisk(d) | Prefix::Disk(d) => {
                OsString::from(format!("{}:", char::from(d.to_ascii_uppercase())))
            }
            Prefix::VerbatimUNC(server, share) | Prefix::UNC(server, share) => {
                let mut unc = OsString::from("//");
                unc.push(server);
                unc.push("/");
                unc.push(share);
                fold(&unc, NameCase::Insensitive)
            }
            _ => folded(prefix.as_os_str()),
        },
        Component::RootDir => OsString::new(),
        other => folded(other.as_os_str()),
    };
    part.to_string_lossy().into_owned()
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

    /// Mutation: compare paths as written, without resolving `.` and `..`,
    /// or resolve a relative path against the current directory; this test
    /// fails.
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
        // Relative, and resolved by name only: `src` exists in the current
        // directory of a test run, and must not become absolute.
        let relative_path = Path::new("no-such-dir/y/../z");
        assert!(same(relative_path, Path::new("no-such-dir/z"), case));
        assert!(within(relative_path, Path::new("no-such-dir/."), case));
        assert!(!within(
            Path::new("no-such-dir/y/out"),
            Path::new("no-such-dir/z"),
            case
        ));
        assert_eq!(resolve(Path::new("src/../src")).0, PathBuf::from("src"));
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
        let (real, _) = resolve(&missing);
        assert!(real.ends_with("not/yet"));
        assert!(real.starts_with(std::fs::canonicalize(&dir).unwrap()));
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

    /// Names the file system holds are compared as it spells them: `σ` and
    /// `ς` are two files on NTFS, though simple upper-casing makes both
    /// `Σ`. Only a part not there yet is folded.
    ///
    /// Mutation: fold the existing part too, or fold nothing; this test
    /// fails.
    #[test]
    fn existing_names_are_compared_as_the_file_system_spells_them() {
        let dir = scratch("spelled");
        let case = NameCase::Insensitive;
        for name in ["tool_σ.cpp", "tool_ς.cpp"] {
            std::fs::write(dir.join(name), "").unwrap();
        }
        assert!(!same(
            &dir.join("tool_σ.cpp"),
            &dir.join("tool_ς.cpp"),
            case
        ));
        assert!(same(&dir.join("New/Out"), &dir.join("new/out"), case));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A share written with `\\?\UNC\` and without it is one share, its
    /// server and share names in either case; so is a drive.
    ///
    /// Mutation: keep a `\\?\` prefix as written, or compare the server,
    /// share or drive letter with case; this test fails.
    #[cfg(windows)]
    #[test]
    fn a_share_is_one_share_however_it_is_written() {
        let prefix = |p: &'static str| part(Path::new(p).components().next().unwrap(), None);
        assert_eq!(
            prefix(r"\\?\UNC\Server\Share\x"),
            prefix(r"\\server\share\x")
        );
        assert_eq!(prefix(r"\\?\c:\x"), prefix(r"C:\x"));
    }

    /// Mutation: give `parent` the place itself; this test fails.
    #[test]
    fn a_place_knows_its_directory() {
        let case = NameCase::Sensitive;
        let file = Place::of(Path::new("/p/src/a.c"), case);
        let src = Place::of(Path::new("/p/src"), case);
        assert!(file.parent().unwrap().same(&src));
        assert!(!file.same(&src));
    }
}
