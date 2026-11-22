//! Where cuno puts what it builds, inside the target directory
//! (`docs/en/specs/build.md`, "Output directory").

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use cuno_manifest::Profile;
use cuno_toolchain::HostOs;

/// The paths of one profile's build under a target directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    root: PathBuf,
    profile_dir: PathBuf,
    host: HostOs,
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
            root: root.to_owned(),
            profile_dir: target_dir.join(profile_dir_name(profile)),
            host,
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
    pub fn object(&self, package: &str, source: &Path) -> PathBuf {
        let relative = source.strip_prefix(&self.root).unwrap_or(source);
        let mut object = self.profile_dir.join("obj").join(package);
        for component in relative.components() {
            object.push(component);
        }
        let mut name = object.file_name().map(OsString::from).unwrap_or_default();
        name.push(".obj");
        object.set_file_name(name);
        object
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
}
