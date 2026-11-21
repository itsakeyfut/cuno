//! Loading and validating `cuno.toml` and `config.toml`.
//!
//! See `docs/en/specs/manifest.md`.

mod diagnostic;
mod manifest;
mod project;
mod read;

pub use diagnostic::{ConfigError, ConfigWarning, Location, shown_path};
pub use manifest::{
    Bin, BuildTable, CompilerName, Edition, Framework, Lang, Loaded, Manifest, OptLevel, Os,
    Package, Profile, ProfileTable, Runtime, Subsystem, Test, Warnings, load_manifest,
    parse_manifest,
};
pub use project::{Project, find_project, resolve_path};
