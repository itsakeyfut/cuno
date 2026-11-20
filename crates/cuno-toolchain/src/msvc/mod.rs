//! MSVC: finding Visual Studio and its toolsets, and the environment a
//! compile needs. See `docs/en/specs/toolchain.md`, "Visual Studio / Build
//! Tools" and "Environment resolution on Windows".

mod detect;
mod environment;

pub use detect::{MsvcToolset, VsInstallation, find_installations};
pub use environment::{
    MsvcEnvironment, Registry, SystemRegistry, WindowsSdk, find_windows_sdk, resolve,
};
