//! Compiler abstraction, detection, per-compiler implementations, and flag
//! translation.
//!
//! See `docs/en/specs/toolchain.md`.

pub mod compiler;
mod error;
pub mod flags;
pub mod gcc;
mod host;
pub mod llvm;
pub mod msvc;
pub mod prepare;
pub mod probe;
mod run;
pub mod select;
mod version;

pub use error::ToolchainError;
pub use host::HostOs;
pub use run::{Output, Runner, SystemRunner, TOOL_TIMEOUT};
pub use version::Version;
