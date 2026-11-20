//! MSVC: finding Visual Studio and its toolsets. See
//! `docs/en/specs/toolchain.md`, "Visual Studio / Build Tools".

mod detect;

pub use detect::{MsvcToolset, VsInstallation, find_installations};
