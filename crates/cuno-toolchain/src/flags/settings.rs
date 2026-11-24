//! The settings a compile and a link are made from, resolved from
//! `cuno.toml` by `cuno-manifest` (`docs/en/specs/manifest.md`). They live
//! here because turning them into flags is this crate's job
//! (`docs/en/specs/architecture.md`).

use std::path::PathBuf;

macro_rules! choices {
    ($(#[$doc:meta])* $name:ident { $($(#[$vdoc:meta])* $variant:ident = $text:literal),+ $(,)? }) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
        pub enum $name { $($(#[$vdoc])* $variant),+ }

        impl $name {
            /// Every value, as written in `cuno.toml`.
            pub const ALL: &[($name, &str)] = &[$(($name::$variant, $text)),+];

            /// The value as written in `cuno.toml`.
            pub fn as_str(self) -> &'static str {
                match self { $($name::$variant => $text),+ }
            }

            /// The value `text` names in `cuno.toml`.
            pub fn parse(text: &str) -> Option<$name> {
                Self::ALL.iter().find(|(_, t)| *t == text).map(|(v, _)| *v)
            }
        }
    };
}

choices!(
    /// `[package] lang`.
    Lang { /// C.
    C = "c", /// C++.
    Cxx = "c++" }
);
choices!(
    /// `[package] edition`.
    Edition { /// C++17.
    Cxx17 = "c++17", /// C++20.
    Cxx20 = "c++20", /// C++23.
    Cxx23 = "c++23", /// C11.
    C11 = "c11", /// C17.
    C17 = "c17", /// C23.
    C23 = "c23" }
);
choices!(
    /// `warnings`.
    Warnings { /// The compiler's own.
    Default = "default", /// The recommended set.
    All = "all", /// Strict conformance too.
    Pedantic = "pedantic" }
);
choices!(
    /// `runtime`.
    Runtime { /// The C runtime as a DLL.
    Dynamic = "dynamic", /// The C runtime in the executable.
    Static = "static" }
);
choices!(
    /// `[[bin]] subsystem`.
    Subsystem { /// A console program.
    Console = "console", /// A program without a console window.
    Windows = "windows" }
);
/// `opt-level`: `0` to `3`, or `"s"` for size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptLevel {
    /// `0`.
    O0,
    /// `1`.
    O1,
    /// `2`.
    O2,
    /// `3`.
    O3,
    /// `"s"`.
    Size,
}

impl Lang {
    /// The entry file a package of this language has when `cuno.toml` names
    /// none, relative to the package root as `cuno.toml` writes paths
    /// (`docs/en/specs/project-layout.md`).
    pub fn default_entry(self) -> &'static str {
        match self {
            Lang::C => "src/main.c",
            Lang::Cxx => "src/main.cpp",
        }
    }

    /// The edition a package of this language gets by default.
    pub fn default_edition(self) -> Edition {
        match self {
            Lang::C => Edition::C17,
            Lang::Cxx => Edition::Cxx20,
        }
    }
}

impl Edition {
    /// The language this edition is a standard of.
    pub fn lang(self) -> Lang {
        match self {
            Edition::C11 | Edition::C17 | Edition::C23 => Lang::C,
            _ => Lang::Cxx,
        }
    }
}

/// The build settings for one build, with every layer applied and the
/// defaults of `manifest.md` filled in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildSettings {
    /// `warnings`.
    pub warnings: Warnings,
    /// `warnings-as-errors`.
    pub warnings_as_errors: bool,
    /// `defines`.
    pub defines: Vec<String>,
    /// `include-dirs`.
    pub include_dirs: Vec<PathBuf>,
    /// `links`.
    pub links: Vec<String>,
    /// `lib-dirs`.
    pub lib_dirs: Vec<PathBuf>,
    /// `link-flags`.
    pub link_flags: Vec<String>,
    /// `[build.<compiler>] flags`.
    pub flags: Vec<String>,
    /// `runtime`.
    pub runtime: Runtime,
    /// `exceptions`.
    pub exceptions: bool,
    /// `rtti`.
    pub rtti: bool,
}

/// A profile's settings with the defaults of `manifest.md` filled in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProfileSettings {
    /// `opt-level`.
    pub opt_level: OptLevel,
    /// `debug-info`.
    pub debug_info: bool,
    /// `debug-assertions`.
    pub debug_assertions: bool,
    /// `lto`.
    pub lto: bool,
}
