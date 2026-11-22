//! Turning settings into compiler arguments. See `docs/en/specs/build.md`,
//! "Settings and flags".

mod compile;
mod link;
mod settings;

pub use compile::{
    CompileInput, Mode, TARGET_SWITCH, check_edition, compile_args, compile_flags, dependency_args,
    for_clang_tools,
};
pub use link::{LinkInput, link_args};

pub use settings::{
    BuildSettings, Edition, Lang, OptLevel, ProfileSettings, Runtime, Subsystem, Warnings,
};
