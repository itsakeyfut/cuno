//! `cuno fmt` and `cuno lint` (clang-format / clang-tidy).
//!
//! See `docs/en/specs/dev-tools.md`.

pub mod format;

use std::io;
use std::path::Path;

use cuno_manifest::Manifest;

/// Writes the files editors read so they format and analyse as cuno does,
/// at the project `root`: `.clang-format` from `[fmt]`, overwriting what is
/// there, since cuno.toml is their source of truth
/// (`docs/en/specs/dev-tools.md`, "Configuration"). Returns each file that
/// could not be written, with why; the caller reports them.
pub fn write_editor_files(root: &Path, manifest: &Manifest) -> Vec<(&'static str, io::Error)> {
    let mut failed = Vec::new();
    let config = format::clang_format_config(&manifest.fmt, manifest.package.lang);
    if let Err(e) = std::fs::write(root.join(format::FILE), config) {
        failed.push((format::FILE, e));
    }
    failed
}
