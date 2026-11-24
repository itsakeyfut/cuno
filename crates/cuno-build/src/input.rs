//! The one place a compile's input is assembled from the settings: which
//! layers of `cuno.toml` apply, the standard for the source's language, and
//! the include directories cuno always adds (`docs/en/specs/build.md`,
//! "Default include paths"). `cuno doctor`'s standard library check and the
//! build both take their flags from here, so they cannot disagree.

use std::path::{Path, PathBuf};

use cuno_manifest::{CompilerName, Manifest, Os, Profile};
use cuno_toolchain::HostOs;
use cuno_toolchain::compiler::CompilerKind;
use cuno_toolchain::flags::{BuildSettings, CompileInput, Edition, Lang, Mode, ProfileSettings};

/// Everything one compile is made from, owned, so a [`CompileInput`] can be
/// borrowed from it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompileSettings {
    /// The compiler.
    pub kind: CompilerKind,
    /// The source's language.
    pub lang: Lang,
    /// The standard for the source.
    pub edition: Edition,
    /// The layered build settings.
    pub build: BuildSettings,
    /// The profile's settings.
    pub profile: ProfileSettings,
    /// The include directories cuno adds, before `include-dirs`.
    pub include_dirs: Vec<PathBuf>,
    /// Whether `CUNO_TEST` is defined.
    pub test: bool,
    /// Compile or check.
    pub mode: Mode,
}

impl CompileSettings {
    /// The input the flag translation reads.
    pub fn input(&self) -> CompileInput<'_> {
        CompileInput {
            kind: self.kind,
            lang: self.lang,
            edition: self.edition,
            build: &self.build,
            profile: &self.profile,
            include_dirs: &self.include_dirs,
            test: self.test,
            mode: self.mode,
        }
    }
}

/// The settings compile_commands.json's commands are made with, whatever is
/// built: [`compile_settings`] in the `dev` profile, compiled, with
/// `CUNO_TEST` defined (`docs/en/specs/build.md`, "Output"). One function,
/// as the pipeline writes the file with them and `cuno doctor`'s clang-tidy
/// check analyses with them, which a second copy of the three values would
/// let drift apart (`docs/en/specs/toolchain.md`, "Minimum compiler
/// versions").
pub fn editor_settings(
    project: Option<(&Path, &Manifest)>,
    kind: CompilerKind,
    lang: Lang,
    host: HostOs,
    bundled: &Path,
) -> CompileSettings {
    compile_settings(
        project,
        kind,
        Profile::Dev,
        lang,
        host,
        bundled,
        true,
        Mode::Compile,
    )
}

/// The settings for compiling a `lang` source of the project at `root` with
/// `manifest` (or, with no project, the defaults) with `kind` in `profile`,
/// for a host `host` building for itself: `[build]`, `[platform.<os>.build]`,
/// `[build.<compiler>]` and the profile, layered as `manifest.md` says; the
/// package's edition for its own language and the other language's default
/// otherwise (`project-layout.md`, "Target files"); `src/` and then
/// `bundled`, the directory of cuno's bundled headers, on the include path.
// Each argument answers its own question; a struct would only rename them.
#[allow(clippy::too_many_arguments)]
pub fn compile_settings(
    project: Option<(&Path, &Manifest)>,
    kind: CompilerKind,
    profile: Profile,
    lang: Lang,
    host: HostOs,
    bundled: &Path,
    test: bool,
    mode: Mode,
) -> CompileSettings {
    let defaults;
    let (root, manifest) = match project {
        Some((root, manifest)) => (Some(root), manifest),
        None => {
            defaults = default_manifest();
            (None, &defaults)
        }
    };
    let edition = if lang == manifest.package.lang {
        manifest.package.edition
    } else {
        lang.default_edition()
    };
    CompileSettings {
        kind,
        lang,
        edition,
        build: manifest.build_settings(target_os(host), CompilerName::from(kind), profile),
        profile: manifest.profile_settings(profile),
        include_dirs: root
            .map(|r| r.join("src"))
            .into_iter()
            .chain([bundled.to_owned()])
            .collect(),
        test,
        mode,
    }
}

/// The OS a build for `host` targets, which picks `[platform.<os>.build]`:
/// the host's own, since cross-compiling is not supported yet.
pub fn target_os(host: HostOs) -> Os {
    match host {
        HostOs::Windows => Os::Windows,
        // The only other host cuno builds on before Linux and macOS are
        // told apart (M8).
        HostOs::Other => Os::Linux,
    }
}

/// The manifest of a project with nothing written: what a command run
/// outside a project builds with.
fn default_manifest() -> Manifest {
    cuno_manifest::parse_manifest(
        "[package]\nname = \"default\"\nversion = \"0.0.0\"\n",
        Path::new("cuno.toml"),
        Path::new("."),
    )
    .expect("a manifest with only [package] name and version is valid")
    .value
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use cuno_toolchain::flags::compile_flags;

    /// A bundled include directory for the tests' settings. Only compared,
    /// never looked at on disk.
    pub(crate) const BUNDLED: &str = "C:/home/include/0.0.0";

    fn manifest(toml: &str) -> Manifest {
        cuno_manifest::parse_manifest(toml, Path::new("cuno.toml"), Path::new("C:/p"))
            .unwrap()
            .value
    }

    const LAYERED: &str = r#"
[package]
name = "app"
version = "0.1.0"
edition = "c++17"

[build]
defines = ["SHARED"]

[platform.windows.build]
defines = ["ON_WINDOWS"]

[platform.linux.build]
defines = ["ON_LINUX"]

[build.clang-cl]
flags = ["/DCLANG_CL_ONLY"]

[build.msvc]
flags = ["/DMSVC_ONLY"]

[profile.release.build]
defines = ["RELEASE_ONLY"]
"#;

    fn settings(m: &Manifest, kind: CompilerKind, profile: Profile, lang: Lang) -> CompileSettings {
        compile_settings(
            Some((Path::new("C:/p"), m)),
            kind,
            profile,
            lang,
            HostOs::Windows,
            Path::new(BUNDLED),
            false,
            Mode::Compile,
        )
    }

    /// Mutation: drop any layer (`[build]`, the host's `[platform]`, the
    /// compiler's own table, the profile's), take another OS's or another
    /// compiler's table, or leave `src/` or the bundled headers off the
    /// include path, or put them in the other order; this test fails.
    #[test]
    fn every_layer_for_this_host_compiler_and_profile_applies() {
        let m = manifest(LAYERED);
        let dev = settings(&m, CompilerKind::ClangCl, Profile::Dev, Lang::Cxx);
        assert_eq!(dev.build.defines, ["SHARED", "ON_WINDOWS"]);
        assert_eq!(dev.build.flags, ["/DCLANG_CL_ONLY"]);
        assert_eq!(
            dev.include_dirs,
            [PathBuf::from("C:/p/src"), PathBuf::from(BUNDLED)]
        );
        assert_eq!(dev.profile, m.profile_settings(Profile::Dev));

        let release = settings(&m, CompilerKind::Msvc, Profile::Release, Lang::Cxx);
        assert_eq!(
            release.build.defines,
            ["SHARED", "ON_WINDOWS", "RELEASE_ONLY"]
        );
        assert_eq!(release.build.flags, ["/DMSVC_ONLY"]);
        assert_eq!(release.profile, m.profile_settings(Profile::Release));
    }

    /// A source in the package's language gets the package's edition; one in
    /// the other language gets that language's default and none of the C++-only
    /// flags (`project-layout.md`, "Target files").
    ///
    /// Mutation: give every source the package's edition, or its language;
    /// this test fails.
    #[test]
    fn each_language_gets_its_own_standard() {
        let m = manifest(LAYERED);
        let cxx = settings(&m, CompilerKind::Msvc, Profile::Dev, Lang::Cxx);
        assert_eq!((cxx.lang, cxx.edition), (Lang::Cxx, Edition::Cxx17));
        let c = settings(&m, CompilerKind::Msvc, Profile::Dev, Lang::C);
        assert_eq!((c.lang, c.edition), (Lang::C, Edition::C17));
        let flags: Vec<_> = compile_flags(&c.input())
            .into_iter()
            .map(|f| f.into_string().unwrap())
            .collect();
        assert!(flags.contains(&"/std:c17".to_owned()), "{flags:?}");
        assert!(!flags.contains(&"/EHsc".to_owned()), "{flags:?}");
    }

    /// Outside a project there is nothing to layer and no `src/`: the defaults,
    /// C++20, and only the bundled headers on the include path. `test` and the
    /// mode reach the command line.
    ///
    /// Mutation: add another include directory, or another standard, outside a
    /// project, or drop `test` or the mode in `input()`; this test fails.
    #[test]
    fn outside_a_project_the_defaults_apply() {
        let s = compile_settings(
            None,
            CompilerKind::Clang,
            Profile::Dev,
            Lang::Cxx,
            HostOs::Windows,
            Path::new(BUNDLED),
            true,
            Mode::Check,
        );
        assert_eq!((s.lang, s.edition), (Lang::Cxx, Edition::Cxx20));
        assert_eq!(s.include_dirs, [PathBuf::from(BUNDLED)]);
        assert!(s.build.defines.is_empty());
        assert_eq!((s.test, s.mode), (true, Mode::Check));
        // ... and reach the command line through `input()`.
        let args: Vec<_> =
            cuno_toolchain::flags::compile_args(&s.input(), Path::new("a.cpp"), None)
                .into_iter()
                .map(|a| a.into_string().unwrap())
                .collect();
        assert!(args.contains(&"-DCUNO_TEST".to_owned()), "{args:?}");
        assert!(args.contains(&"-fsyntax-only".to_owned()), "{args:?}");
    }

    /// Each host builds for its own OS, which picks its `[platform]` layer.
    ///
    /// Mutation: map either host to another OS; this test fails.
    #[test]
    fn a_host_builds_for_its_own_os() {
        assert_eq!(target_os(HostOs::Windows), Os::Windows);
        let m = manifest(LAYERED);
        let other = compile_settings(
            Some((Path::new("C:/p"), &m)),
            CompilerKind::Clang,
            Profile::Dev,
            Lang::Cxx,
            HostOs::Other,
            Path::new(BUNDLED),
            false,
            Mode::Compile,
        );
        assert_eq!(other.build.defines, ["SHARED", "ON_LINUX"]);
    }
}
