//! Loading and validating `cuno.toml` and `config.toml`.
//!
//! See `docs/en/specs/manifest.md`.

mod config;
mod diagnostic;
mod manifest;
mod project;
mod read;
mod settings;

pub use config::{Config, Tool, load_config, parse_config};
pub use cuno_toolchain::flags::{
    BuildSettings, Edition, Lang, OptLevel, ProfileSettings, Runtime, Subsystem, Warnings,
};
pub use diagnostic::{ConfigError, ConfigWarning, Location, shown_path};
pub use manifest::{
    Bin, BuildTable, CompilerName, Framework, Loaded, Manifest, Os, Package, Profile, ProfileTable,
    Test, load_manifest, parse_manifest,
};
pub use project::{Project, find_project, resolve_path};
pub use settings::{CliSettings, Inputs, Settings, resolve};
