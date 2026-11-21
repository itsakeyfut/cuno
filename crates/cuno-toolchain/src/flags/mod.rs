//! Turning settings into compiler arguments. See `docs/en/specs/build.md`,
//! "Settings and flags".

mod settings;

pub use settings::{
    BuildSettings, Edition, Lang, OptLevel, ProfileSettings, Runtime, Subsystem, Warnings,
};
