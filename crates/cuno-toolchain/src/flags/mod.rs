//! Turning settings into compiler arguments. See `docs/en/specs/build.md`,
//! "Settings and flags".

mod compile;
mod settings;

pub use compile::{CompileInput, Mode, compile_args, compile_flags};

pub use settings::{
    BuildSettings, Edition, Lang, OptLevel, ProfileSettings, Runtime, Subsystem, Warnings,
};
