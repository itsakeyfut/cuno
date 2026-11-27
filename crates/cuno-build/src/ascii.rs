//! What a compiler whose tools open paths in the ANSI code page cannot
//! build: the paths that would reach them not in ASCII however they are
//! written. Refused before anything compiles (`docs/en/specs/toolchain.md`,
//! "MinGW (GCC)").

use std::path::{Path, PathBuf};

use cuno_core::Env;
use cuno_toolchain::compiler::CompilerKind;

use crate::error::{BuildError, NotAscii};
use crate::layout::Layout;

/// Whether `path` is all ASCII.
fn is_ascii(path: &Path) -> bool {
    path.to_str().is_some_and(str::is_ascii)
}

/// The first path `kind`'s tools would be given that is not ASCII, written
/// from the project root where it is inside it (as the build writes it):
/// the target directory, a `lib-dirs` directory, and with `lto` the
/// temporary directory, `TMP` or else `TEMP` in `env`, which GCC's
/// link-time optimisation writes to. Ok for a compiler whose tools open
/// any path.
pub fn check(
    kind: CompilerKind,
    layout: &Layout,
    target_dir: &Path,
    lib_dirs: &[PathBuf],
    lto: bool,
    env: &Env,
) -> Result<(), BuildError> {
    if !kind.needs_ascii_paths() {
        return Ok(());
    }
    let refuse = |what, path: &Path| BuildError::NotAscii {
        what,
        path: path.to_owned(),
    };
    if !is_ascii(&layout.from_root(target_dir)) {
        return Err(refuse(NotAscii::Target, target_dir));
    }
    if let Some(dir) = lib_dirs.iter().find(|d| !is_ascii(&layout.from_root(d))) {
        return Err(refuse(NotAscii::LibDir, dir));
    }
    if lto
        && let Some(temp) = env.get("TMP").or_else(|| env.get("TEMP"))
        && !is_ascii(Path::new(temp))
    {
        return Err(refuse(NotAscii::Temp, Path::new(temp)));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cuno_core::{CunoError, ExitClass, NameCase};
    use cuno_manifest::Profile;
    use cuno_toolchain::HostOs;

    fn layout(root: &str) -> Layout {
        Layout::new(
            &Path::new(root).join("target"),
            Profile::Dev,
            Path::new(root),
            HostOs::Windows,
        )
    }

    fn env(pairs: &[(&str, &str)]) -> Env {
        Env::from_pairs(
            pairs.iter().map(|(k, v)| ((*k).into(), (*v).into())),
            NameCase::Insensitive,
        )
    }

    /// A project whose own path is not ASCII builds with GCC: what is
    /// inside it is written from the root. A target directory outside it, a
    /// library directory, or with LTO the temporary directory, that is not
    /// ASCII is refused with the path, exit 3; `TEMP` counts when `TMP` is
    /// not set. Other compilers are never refused.
    ///
    /// Mutation: skip any of the three, check the absolute path of what is
    /// inside the project, ignore `TEMP`, refuse the temporary directory
    /// without LTO, or refuse another compiler; this test fails.
    #[test]
    fn what_cannot_be_written_in_ascii_is_refused() {
        let gcc = CompilerKind::Gcc;
        let japanese = layout("C:/日本語/p");
        let ascii = env(&[("TMP", "C:/tmp")]);
        let inside = Path::new("C:/日本語/p/target");
        let libs = [PathBuf::from("C:/日本語/p/libs")];
        assert!(check(gcc, &japanese, inside, &libs, true, &ascii).is_ok());

        let what = |r: Result<(), BuildError>| match r {
            Err(BuildError::NotAscii { what, path }) => Some((what, path)),
            _ => None,
        };
        assert_eq!(
            what(check(
                gcc,
                &japanese,
                Path::new("D:/出力"),
                &[],
                false,
                &ascii
            )),
            Some((NotAscii::Target, PathBuf::from("D:/出力")))
        );
        assert_eq!(
            what(check(
                gcc,
                &japanese,
                inside,
                &[PathBuf::from("C:/日本語/p/ライブラリ")],
                false,
                &ascii
            )),
            Some((NotAscii::LibDir, PathBuf::from("C:/日本語/p/ライブラリ")))
        );
        let japanese_temp = env(&[("TEMP", "C:/Users/山田/AppData/Local/Temp")]);
        assert_eq!(
            what(check(gcc, &japanese, inside, &[], true, &japanese_temp)),
            Some((
                NotAscii::Temp,
                PathBuf::from("C:/Users/山田/AppData/Local/Temp")
            ))
        );
        assert!(check(gcc, &japanese, inside, &[], false, &japanese_temp).is_ok());
        let err = check(gcc, &japanese, Path::new("D:/出力"), &[], false, &ascii).unwrap_err();
        assert_eq!(err.exit_class(), ExitClass::Setup);
        for kind in [
            CompilerKind::Msvc,
            CompilerKind::Clang,
            CompilerKind::ClangCl,
        ] {
            assert!(
                check(
                    kind,
                    &japanese,
                    Path::new("D:/出力"),
                    &[],
                    true,
                    &japanese_temp
                )
                .is_ok()
            );
        }
    }
}
