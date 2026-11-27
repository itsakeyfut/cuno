//! Loading and validating `Cuno.toml` and `config.toml`.
//!
//! See `docs/en/specs/manifest.md`.

mod config;
mod diagnostic;
mod glob;
mod manifest;
mod project;
mod read;
mod settings;

pub use config::{Config, LocatedDir, Tool, load_config, parse_config, write_user_compiler};
pub use cuno_toolchain::flags::{
    BuildSettings, Edition, Lang, OptLevel, ProfileSettings, Runtime, Subsystem, Warnings,
};
pub use diagnostic::{ConfigError, ConfigWarning, Location, shown_path};
pub use glob::Glob;
pub use manifest::{
    Bin, BuildTable, CompilerName, Fmt, FmtStyle, Framework, Lint, LintPreset, Loaded, Manifest,
    Os, Package, Profile, ProfileTable, Test, load_manifest, parse_manifest, valid_package_name,
};
pub use project::{MANIFEST, ManifestIn, Project, find_project, manifest_in, resolve_path};
pub use settings::{CliSettings, Inputs, Settings, Term, resolve, resolve_user};
