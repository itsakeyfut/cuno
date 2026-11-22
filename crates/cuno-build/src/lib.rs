//! Building and executing the build graph, change detection, and
//! `compile_commands.json`.
//!
//! See `docs/en/specs/build.md`.

mod error;
pub mod exec;
pub mod graph;
pub mod input;
pub mod layout;
pub mod sources;

pub use error::BuildError;
