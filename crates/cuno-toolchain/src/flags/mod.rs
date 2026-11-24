//! Turning settings into compiler arguments. See `docs/en/specs/build.md`,
//! "Settings and flags".

mod clang_tools;
mod compile;
mod link;
mod settings;

pub use clang_tools::{ToolCommand, ToolContext, for_clang_tools, include_dirs};
pub use compile::{
    CompileInput, Mode, TARGET_SWITCH, check_edition, color_args, compile_args, compile_flags,
    dependency_args,
};
pub use link::{LinkInput, library_file, link_args};

pub use settings::{
    BuildSettings, Edition, Lang, OptLevel, ProfileSettings, Runtime, Subsystem, Warnings,
};
