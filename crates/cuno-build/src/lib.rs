//! Building and executing the build graph, change detection, and
//! `compile_commands.json`.
//!
//! See `docs/en/specs/build.md`.

mod ascii;
pub mod bundled;
mod compdb;
mod deps;
mod error;
pub mod exec;
mod graph;
mod input;
pub mod layout;
pub mod pipeline;
pub mod sources;
mod state;
pub mod toolchain;

pub use error::BuildError;
