//! Loading and validating `cuno.toml` and `config.toml`.
//!
//! See `docs/en/specs/manifest.md`.

mod diagnostic;
mod project;

pub use diagnostic::{ConfigError, ConfigWarning, Location, shown_path};
pub use project::{Project, find_project, resolve_path};
