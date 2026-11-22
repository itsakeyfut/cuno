//! The scratch directory a standard library check writes its probe in,
//! shared by `cuno doctor` and the build commands.

use std::path::Path;

use cuno_core::CunoError;
use cuno_toolchain::ToolchainError;

/// Runs the check of `what` for `command` with a scratch directory of this
/// process's own in the temporary directory, removed afterwards.
pub fn in_scratch(
    command: &str,
    what: &'static str,
    check: impl FnOnce(&Path) -> Result<(), ToolchainError>,
) -> Result<(), Box<dyn CunoError>> {
    // `create_dir`, not `create_dir_all`: a temporary directory that does not
    // exist is a problem to report, not a tree to make.
    let scratch = std::env::temp_dir().join(format!("cuno-{command}-{}", std::process::id()));
    std::fs::create_dir(&scratch).map_err(|e| {
        Box::new(ScratchError {
            what,
            dir: scratch.clone(),
            reason: e.to_string(),
        }) as Box<dyn CunoError>
    })?;
    let result = check(&scratch);
    let _ = std::fs::remove_dir_all(&scratch);
    result.map_err(|e| Box::new(e) as Box<dyn CunoError>)
}

/// The probe's scratch directory cannot be made in the temporary
/// directory (`TEMP` names one that does not exist or cannot be written).
#[derive(Debug)]
struct ScratchError {
    what: &'static str,
    dir: std::path::PathBuf,
    reason: String,
}

impl std::fmt::Display for ScratchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "cannot create {} to check {}: {}",
            self.dir.display(),
            self.what,
            self.reason
        )
    }
}

impl std::error::Error for ScratchError {}

impl CunoError for ScratchError {
    fn exit_class(&self) -> cuno_core::ExitClass {
        cuno_core::ExitClass::Setup
    }
    fn help(&self) -> Option<String> {
        Some("set TEMP to a directory that exists and can be written".to_owned())
    }
}
