//! Building and executing the build graph, change detection, and
//! `compile_commands.json`.
//!
//! See `docs/en/specs/build.md`.

pub mod bundled;
pub mod compdb;
pub mod deps;
mod error;
pub mod exec;
pub mod graph;
pub mod input;
pub mod layout;
pub mod pipeline;
pub mod sources;
pub mod state;
pub mod toolchain;

pub use error::BuildError;
