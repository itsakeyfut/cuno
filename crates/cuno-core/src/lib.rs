//! What the other cuno crates share: error classification, output, the cuno
//! home, the environment snapshot, path identity, starting a built program,
//! and the scratch directory. Nothing else belongs here without a change to
//! the spec.
//!
//! See `docs/en/specs/architecture.md`, "What goes in `cuno-core`".

mod env;
mod error;
mod home;
pub mod paths;
pub mod program;
pub mod scratch;
mod shell;

pub use env::{Env, NameCase};
pub use error::{CunoError, ExitClass};
pub use home::{HomeError, cuno_home};
pub use shell::{ColorChoice, Shell, Verbosity, colors};
