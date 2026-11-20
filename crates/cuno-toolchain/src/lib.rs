//! Compiler abstraction, detection, per-compiler implementations, and flag
//! translation.
//!
//! See `docs/en/specs/toolchain.md`.

mod error;
mod host;
pub mod llvm;
pub mod msvc;
mod run;
pub mod select;
mod version;

pub use error::ToolchainError;
pub use host::HostOs;
pub use run::{Output, Runner, SystemRunner};
pub use version::Version;
