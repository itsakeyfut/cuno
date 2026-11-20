//! What the other cuno crates share: error classification, output, the cuno
//! home, and the environment snapshot. Nothing else belongs here without a
//! change to the spec.
//!
//! See `docs/en/specs/architecture.md`, "What goes in `cuno-core`".

mod env;
mod error;
mod shell;

pub use env::Env;
pub use error::{CunoError, ExitClass};
pub use shell::{ColorChoice, Shell};
