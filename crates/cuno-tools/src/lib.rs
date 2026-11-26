//! `cuno fmt` and `cuno lint` (clang-format / clang-tidy).
//!
//! See `docs/en/specs/dev-tools.md`.

pub mod format;
pub mod targets;
pub mod tidy;
pub mod write;

use std::fmt;
use std::io;
use std::path::Path;

use cuno_manifest::Manifest;

/// Why an editor file was left as it was.
#[derive(Debug)]
pub enum EditorFileWarning {
    /// A file cuno did not write is there: it is the user's.
    Kept(&'static str),
    /// It could not be written, or is not a file (a link, a directory).
    Failed(&'static str, io::Error),
}

impl fmt::Display for EditorFileWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EditorFileWarning::Kept(file) => write!(
                f,
                "kept the hand-written `{file}`; remove it to have cuno write it from cuno.toml"
            ),
            EditorFileWarning::Failed(file, e) => write!(f, "could not write {file}: {e}"),
        }
    }
}

/// Writes the files editors read so they format and analyse as cuno does,
/// at the project `root`: `.clang-format` from `[fmt]` and `.clang-tidy`
/// from `[lint]`. Only a file cuno
/// wrote, which starts with its marker, is overwritten, and only when its
/// content changes; a file the user wrote, a link or a directory is left
/// and warned about (`docs/en/specs/dev-tools.md`, "Configuration").
pub fn write_editor_files(root: &Path, manifest: &Manifest) -> Vec<EditorFileWarning> {
    let lang = manifest.package.lang;
    let format = format::clang_format_config(&manifest.fmt, lang);
    let tidy = tidy::clang_tidy_file(&manifest.lint);
    [
        write_generated(root, format::FILE, format::MARKER, &format),
        write_generated(root, tidy::FILE, tidy::MARKER, &tidy),
    ]
    .into_iter()
    .filter_map(Result::err)
    .collect()
}

/// Writes `content` to `root/file` unless something other than a file cuno
/// generated (first line `marker`) is there, or it already holds `content`.
fn write_generated(
    root: &Path,
    file: &'static str,
    marker: &str,
    content: &str,
) -> Result<(), EditorFileWarning> {
    let path = root.join(file);
    // Never followed: a link would have cuno write outside the project.
    match write::generated(&path, marker) {
        Ok(()) => {}
        Err(write::Refusal::HandWritten) => return Err(EditorFileWarning::Kept(file)),
        Err(write::Refusal::Unreadable(e)) => return Err(EditorFileWarning::Failed(file, e)),
        Err(other) => {
            return Err(EditorFileWarning::Failed(
                file,
                io::Error::other(other.to_string()),
            ));
        }
    }
    if std::fs::read(&path).is_ok_and(|held| held == content.as_bytes()) {
        return Ok(());
    }
    std::fs::write(&path, content).map_err(|e| EditorFileWarning::Failed(file, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MARKER: &str = "# generated";

    /// A missing file is written, an unchanged one left alone, a generated
    /// one rewritten, and a hand-written one or a directory kept and
    /// warned about.
    ///
    /// Mutation: overwrite a file without the marker, follow or overwrite
    /// a non-file, or rewrite an unchanged file; this test fails.
    #[test]
    fn only_a_generated_file_is_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let path = root.join(".x");
        let new = format!("{MARKER}\nA: 1\n");
        write_generated(root, ".x", MARKER, &new).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), new);

        // Unchanged: not written again, so its time stays an hour back.
        let past = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(past)
            .unwrap();
        write_generated(root, ".x", MARKER, &new).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), past);

        let newer = format!("{MARKER}\nA: 2\n");
        write_generated(root, ".x", MARKER, &newer).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), newer);

        std::fs::write(&path, "BasedOnStyle: Google\n").unwrap();
        let kept = write_generated(root, ".x", MARKER, &newer).unwrap_err();
        assert!(matches!(kept, EditorFileWarning::Kept(".x")), "{kept:?}");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "BasedOnStyle: Google\n"
        );

        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        let failed = write_generated(root, ".x", MARKER, &newer).unwrap_err();
        assert!(
            matches!(failed, EditorFileWarning::Failed(".x", _)),
            "{failed:?}"
        );
    }

    /// A junction where the file should be is not written through.
    ///
    /// Mutation: follow links; this test fails.
    #[cfg(windows)]
    #[test]
    fn a_link_is_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(dir.path().join(".x"))
            .arg(outside.path())
            .output()
            .unwrap();
        assert!(made.status.success(), "{made:?}");
        let failed = write_generated(dir.path(), ".x", MARKER, "x").unwrap_err();
        assert!(failed.to_string().contains("link"), "{failed}");
        std::fs::remove_dir(dir.path().join(".x")).unwrap();
    }
}
