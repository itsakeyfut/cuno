//! Where cuno puts what it builds, inside the target directory
//! (`docs/en/specs/build.md`, "Output directory").

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use cuno_core::paths;
use cuno_manifest::Profile;
use cuno_toolchain::HostOs;

/// The file a build leaves in an output directory it made, and without
/// which `cuno clean` removes nothing (`docs/en/specs/build.md`, "Output
/// directory").
pub const MARKER: &str = ".cuno-target";

/// Marks `target_dir` as cuno's when it is not there yet or is empty: it
/// is created and [`MARKER`] written in it. A directory that already holds
/// something unmarked is left as it is, since cuno cannot tell it made it.
pub fn claim(target_dir: &Path) -> io::Result<()> {
    if target_dir.join(MARKER).is_file() {
        return Ok(());
    }
    match std::fs::read_dir(target_dir) {
        Ok(mut entries) => {
            if entries.next().is_some() {
                return Ok(());
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => std::fs::create_dir_all(target_dir)?,
        Err(e) => return Err(e),
    }
    std::fs::write(target_dir.join(MARKER), b"")
}

/// Whether `target_dir` carries [`MARKER`].
pub fn is_claimed(target_dir: &Path) -> bool {
    target_dir.join(MARKER).is_file()
}

/// The paths of one profile's build under a target directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    root: PathBuf,
    profile_dir: PathBuf,
    host: HostOs,
    variant: Variant,
}

/// Which build of a profile a layout lays out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Variant {
    /// The program, its objects under `obj/<package>/`.
    Normal,
    /// The test build, compiled with `CUNO_TEST`, its objects under
    /// `test/<package>/` so they never mix with the program's
    /// (`docs/en/specs/build.md`, "Output directory").
    Test,
}

/// The directory a profile's output goes in: `debug` for `dev`, as cuno's
/// users of Cargo expect, and the profile's own name otherwise.
pub fn profile_dir_name(profile: Profile) -> &'static str {
    match profile {
        Profile::Dev => "debug",
        Profile::Release => "release",
    }
}

impl Layout {
    /// The layout of `profile`'s build of the project at `root` in
    /// `target_dir`, for executables run on `host`.
    pub fn new(target_dir: &Path, profile: Profile, root: &Path, host: HostOs) -> Layout {
        Layout {
            root: paths::lexical(root),
            // Normalized, so a `--target-dir` written with `..` gives the
            // executable a path `cuno run` can show (commands.md, "`cuno run`").
            profile_dir: paths::lexical(target_dir).join(profile_dir_name(profile)),
            host,
            variant: Variant::Normal,
        }
    }

    /// The same profile's test build.
    pub fn for_tests(&self) -> Layout {
        Layout {
            variant: Variant::Test,
            ..self.clone()
        }
    }

    /// `<target>/<profile dir>`, which holds this profile's executables and
    /// everything else of its build.
    pub fn profile_dir(&self) -> &Path {
        &self.profile_dir
    }

    /// The object file for `source` of `package`:
    /// `obj/<package>/<source's path in the project>.obj`, so two sources
    /// of the same name in different directories, or `util.c` beside
    /// `util.cpp`, do not collide. Every compiler cuno uses targets the MSVC
    /// ABI, so the extension is `.obj` for all of them.
    ///
    /// `source` is one [`crate::sources::classify`] gave for the same root,
    /// so it lies in the project; one that does not is a bug in cuno, and
    /// panics rather than put an object outside the target directory.
    pub fn object(&self, package: &str, source: &Path) -> PathBuf {
        self.beside_object(package, source, ".obj")
    }

    /// The stamp `cuno check` leaves for `source` of `package` once it is
    /// checked: where [`Layout::object`] would be, ending `.check` instead.
    /// It assumes and panics as [`Layout::object`] does.
    pub fn check_stamp(&self, package: &str, source: &Path) -> PathBuf {
        self.beside_object(package, source, ".check")
    }

    /// `source`'s path under `obj/<package>`, with `suffix` added to its
    /// name. Panics on a `source` outside the project, as
    /// [`Layout::object`] says.
    ///
    /// Its directory is compared as where it leads, and the file by where
    /// its name sits, so a source that is itself a link to a file outside
    /// the project still gets an object here, as does one in a `src/` that
    /// is a link leading out of the project (`docs/en/specs/project-layout.md`,
    /// "Source collection and linking").
    fn beside_object(&self, package: &str, source: &Path, suffix: &str) -> PathBuf {
        let source = paths::lexical(source);
        let case = self.host.name_case();
        // `src/` is the project's even when it is a link leading out of it.
        let src = self.root.join("src");
        let under = source.parent().and_then(|dir| {
            paths::relative(dir, &self.root, case)
                .or_else(|| paths::relative(dir, &src, case).map(|r| Path::new("src").join(r)))
        });
        let (Some(under), Some(file)) = (under, source.file_name()) else {
            panic!(
                "an object was asked for {}, outside the project {}",
                source.display(),
                self.root.display()
            );
        };
        let mut name = OsString::from(file);
        name.push(suffix);
        let dir = match self.variant {
            Variant::Normal => "obj",
            Variant::Test => "test",
        };
        self.profile_dir
            .join(dir)
            .join(package)
            .join(under)
            .join(name)
    }

    /// The executable `bin`.
    pub fn exe(&self, bin: &str) -> PathBuf {
        let suffix = match self.host {
            HostOs::Windows => ".exe",
            HostOs::Other => "",
        };
        self.profile_dir.join(format!("{bin}{suffix}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(profile: Profile) -> Layout {
        Layout::new(
            Path::new("C:/p/target"),
            profile,
            Path::new("C:/p"),
            HostOs::Windows,
        )
    }

    /// Mutation: name `dev`'s directory `dev`, put objects beside the
    /// executables, flatten an object's path, or drop the source's own
    /// extension from its object's name; this test fails.
    #[test]
    fn every_output_has_its_own_place_in_the_profile_dir() {
        let dev = layout(Profile::Dev);
        assert_eq!(dev.profile_dir(), Path::new("C:/p/target/debug"));
        assert_eq!(
            layout(Profile::Release).profile_dir(),
            Path::new("C:/p/target/release")
        );
        let objects = [
            dev.object("app", Path::new("C:/p/src/a/util.cpp")),
            dev.object("app", Path::new("C:/p/src/b/util.cpp")),
            dev.object("app", Path::new("C:/p/src/a/util.c")),
        ];
        assert_eq!(
            objects[0],
            Path::new("C:/p/target/debug/obj/app/src/a/util.cpp.obj")
        );
        assert_ne!(objects[0], objects[1]);
        assert_ne!(objects[0], objects[2]);
        assert_eq!(dev.exe("app"), Path::new("C:/p/target/debug/app.exe"));
        for path in objects.iter().chain([&dev.exe("app")]) {
            assert!(path.starts_with("C:/p/target"), "{}", path.display());
        }
    }

    /// A source spelled with another case than the root, or with `..`, gets
    /// its object inside the target directory; one outside the project is
    /// refused rather than placed beside itself.
    ///
    /// Mutation: keep the root as given, compare it with case on Windows,
    /// or fall back to the source's own path; this test fails.
    #[test]
    fn objects_stay_in_the_target_directory() {
        let dev = Layout::new(
            Path::new("C:/p/target"),
            Profile::Dev,
            Path::new("C:/p/x/.."),
            HostOs::Windows,
        );
        assert_eq!(
            dev.object("app", Path::new("c:/P/src/a.cpp")),
            Path::new("C:/p/target/debug/obj/app/src/a.cpp.obj")
        );
        assert_eq!(
            dev.object("app", Path::new("C:/p/src/sub/../a.cpp")),
            Path::new("C:/p/target/debug/obj/app/src/a.cpp.obj")
        );
        let outside = std::panic::catch_unwind(|| dev.object("app", Path::new("D:/q/a.cpp")));
        assert!(outside.is_err());
    }

    /// A target directory written with `..` is resolved, so the executable
    /// `cuno run` names has no `..` in its path.
    ///
    /// Mutation: keep the target directory as given; this test fails.
    #[test]
    fn a_target_directory_with_dot_dot_is_resolved() {
        let dev = Layout::new(
            Path::new("C:/p/src/../out"),
            Profile::Dev,
            Path::new("C:/p"),
            HostOs::Windows,
        );
        assert_eq!(dev.exe("app"), Path::new("C:/p/out/debug/app.exe"));
    }

    /// A missing or empty output directory is claimed; one that already
    /// holds something unmarked is not, and one claimed stays claimed.
    ///
    /// Mutation: claim a directory that holds something, or claim nothing
    /// that did not exist; this test fails.
    #[test]
    fn only_a_new_or_empty_output_directory_is_claimed() {
        let root = tempfile::tempdir().unwrap();
        let new = root.path().join("new/target");
        claim(&new).unwrap();
        assert!(is_claimed(&new));
        claim(&new).unwrap();
        assert!(is_claimed(&new));

        let empty = root.path().join("empty");
        std::fs::create_dir(&empty).unwrap();
        claim(&empty).unwrap();
        assert!(is_claimed(&empty));

        let src = root.path().join("src");
        std::fs::create_dir(&src).unwrap();
        std::fs::write(src.join("main.cpp"), "").unwrap();
        claim(&src).unwrap();
        assert!(!is_claimed(&src));
    }

    /// Mutation: give executables `.exe` on every host; this test fails.
    #[test]
    fn only_windows_executables_end_in_exe() {
        let other = Layout::new(
            Path::new("/p/target"),
            Profile::Dev,
            Path::new("/p"),
            HostOs::Other,
        );
        assert_eq!(other.exe("app"), Path::new("/p/target/debug/app"));
    }

    /// A source spelled through a junction to the project's `src/` gets its
    /// object where the same source spelled under the root does.
    ///
    /// Mutation: compare the source's directory with the root as written,
    /// without following links; this test fails.
    #[cfg(windows)]
    #[test]
    fn a_source_through_a_junction_has_its_object_in_the_project() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("p");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/a.cpp"), "").unwrap();
        let alias = dir.path().join("alias");
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&alias)
            .arg(root.join("src"))
            .output()
            .unwrap();
        assert!(made.status.success(), "{made:?}");
        let dev = Layout::new(&root.join("target"), Profile::Dev, &root, HostOs::Windows);
        assert_eq!(
            dev.object("app", &alias.join("a.cpp")),
            root.join("target/debug/obj/app/src/a.cpp.obj")
        );
    }

    /// A source in a `src/` that is a junction leading out of the project
    /// gets its object under `obj/<package>/src`, as before the comparison
    /// followed links.
    ///
    /// Mutation: place by where the directory leads only, without taking
    /// `src/` as the project's; this test fails.
    #[cfg(windows)]
    #[test]
    fn a_src_linked_from_outside_has_its_objects_in_the_project() {
        let dir = tempfile::tempdir().unwrap();
        let (root, shared) = (dir.path().join("p"), dir.path().join("shared"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&shared).unwrap();
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(root.join("src"))
            .arg(&shared)
            .output()
            .unwrap();
        assert!(made.status.success(), "{made:?}");
        let dev = Layout::new(&root.join("target"), Profile::Dev, &root, HostOs::Windows);
        assert_eq!(
            dev.object("app", &root.join("src").join("a.cpp")),
            root.join("target/debug/obj/app/src/a.cpp.obj")
        );
    }

    /// A test build's objects go under `test/<package>/`, by the source's
    /// place in the project as the program's go under `obj/<package>/`
    /// (`docs/en/specs/build.md`, "Output directory").
    ///
    /// Mutation: put test objects under `obj/`, drop the package, or move
    /// the program's objects too; this test fails.
    #[test]
    fn a_test_build_has_its_own_objects() {
        let dev = layout(Profile::Dev);
        assert_eq!(
            dev.for_tests().object("app", Path::new("C:/p/src/a.cpp")),
            Path::new("C:/p/target/debug/test/app/src/a.cpp.obj")
        );
        assert_eq!(
            dev.object("app", Path::new("C:/p/src/a.cpp")),
            Path::new("C:/p/target/debug/obj/app/src/a.cpp.obj")
        );
        assert_eq!(dev.for_tests().exe("app"), dev.exe("app"));
    }
}
