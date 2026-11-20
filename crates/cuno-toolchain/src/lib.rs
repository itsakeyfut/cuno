//! Compiler abstraction, detection, per-compiler implementations, and flag
//! translation.
//!
//! See `docs/en/specs/toolchain.md`.

mod error;
pub mod msvc;
mod run;
mod version;

pub use error::ToolchainError;
pub use run::{Output, Runner, SystemRunner};
pub use version::Version;
