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

/// The settings for compiling a `lang` source of the project at `root` with
/// `manifest` (or, with no project, the defaults) with `kind` in `profile`,
/// for a host `host` building for itself: `[build]`, `[platform.<os>.build]`,
/// `[build.<compiler>]` and the profile, layered as `manifest.md` says; the
/// package's edition for its own language and the other language's default
/// otherwise (`project-layout.md`, "Target files"); `src/` on the include
/// path.
pub fn compile_settings(
    project: Option<(&Path, &Manifest)>,
    kind: CompilerKind,
    profile: Profile,
    lang: Lang,
    host: HostOs,
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
        include_dirs: root.map(|r| r.join("src")).into_iter().collect(),
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
mod tests;
