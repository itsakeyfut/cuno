//! Reading the settings, the same way for every command that needs them.

use cuno_core::Shell;
use cuno_manifest::{ConfigError, Inputs, Settings};

/// The settings, with `[term] color` applied to `shell` before the warnings
/// they raised are printed, so every command colors as configured
/// (`docs/en/specs/commands.md`, "Available for all commands"). An error
/// comes back before any configuration file is applied, so it is shown as
/// `--color` and `NO_COLOR` alone decide.
pub fn resolve(inputs: Inputs<'_>, shell: &mut Shell) -> Result<Settings, ConfigError> {
    let loaded = cuno_manifest::resolve(inputs)?;
    shell.configure(loaded.value.color);
    for warning in &loaded.warnings {
        shell.warning(warning);
    }
    Ok(loaded.value)
}
