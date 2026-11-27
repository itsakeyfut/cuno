//! What a compiler whose tools open paths in the ANSI code page cannot
//! build: the paths that would reach them not in ASCII however they are
//! written. Refused before anything compiles (`docs/en/specs/toolchain.md`,
//! "MinGW (GCC)").

use std::path::{Path, PathBuf};

use cuno_core::Env;
use cuno_toolchain::compiler::CompilerKind;

use crate::error::{BuildError, NotAscii};
use crate::layout::Layout;
use crate::sources::Bin;
use cuno_toolchain::short_path::{fits_ansi_code_page, temp_dir};

/// Whether `path` is all ASCII.
fn is_ascii(path: &Path) -> bool {
    path.to_str().is_some_and(str::is_ascii)
}

/// What a build that links gives the linker besides the objects.
pub struct Linking<'a> {
    /// `lib-dirs`.
    pub lib_dirs: &'a [PathBuf],
    /// `lto`, whose link writes to the temporary directory in a way that
    /// fails for any path that is not ASCII.
    pub lto: bool,
    /// The program's executables, named after their entry files under
    /// `src/bin/` or `[[bin]] name`.
    pub bins: &'a [Bin],
}

/// The first path `kind`'s tools would be given that is not ASCII, written
/// from the project root where it is inside it (as the build writes it):
/// the target directory, and for a build that links (`linking`), a
/// `lib-dirs` directory, an executable's name, and the temporary directory
/// its linker writes to: as Windows finds it, `TMP`, else `TEMP`, else
/// `USERPROFILE` in `env`, the build's, which already holds the directory's
/// short name where it has an ASCII one. Ok for a compiler whose tools open
/// any path.
pub fn check(
    kind: CompilerKind,
    layout: &Layout,
    target_dir: &Path,
    linking: Option<Linking<'_>>,
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
    let Some(linking) = linking else {
        return Ok(());
    };
    if let Some(dir) = linking
        .lib_dirs
        .iter()
        .find(|d| !is_ascii(&layout.from_root(d)))
    {
        return Err(refuse(NotAscii::LibDir, dir));
    }
    if let Some(bin) = linking.bins.iter().find(|b| !b.name.is_ascii()) {
        return Err(refuse(NotAscii::Bin, &bin.entry.path));
    }
    // The build's environment already names it by its ASCII short name
    // where it has one; without one, a link works only when the path fits
    // the ANSI code page, and LTO's not at all.
    if let Some(temp) = temp_dir(env)
        && !is_ascii(&temp)
        && (linking.lto || !fits_ansi_code_page(&temp))
    {
        return Err(refuse(NotAscii::Temp, &temp));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::Source;
    use cuno_core::{CunoError, ExitClass, NameCase};
    use cuno_manifest::Profile;
    use cuno_toolchain::HostOs;
    use cuno_toolchain::flags::{Lang, Subsystem};

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

    fn bin(name: &str) -> Bin {
        Bin {
            name: name.to_owned(),
            entry: Source {
                path: PathBuf::from(format!("C:/p/src/bin/{name}.c")),
                lang: Lang::C,
            },
            subsystem: Subsystem::Console,
        }
    }

    fn what(r: Result<(), BuildError>) -> Option<(NotAscii, PathBuf)> {
        match r {
            Err(BuildError::NotAscii { what, path }) => Some((what, path)),
            _ => None,
        }
    }

    /// A project whose own path is not ASCII builds with GCC: what is
    /// inside it is written from the root. A target directory that is not
    /// ASCII written so, and for a build that links a library directory,
    /// an executable's name, or the temporary directory (`TMP`, else
    /// `TEMP`, else `USERPROFILE`) left without an ASCII short name where
    /// the link would fail on it (with LTO, or a path the ANSI code page
    /// cannot hold), are refused with the path, exit 3.
    /// A check links nothing, so only the target directory counts there.
    /// Other compilers are never refused.
    ///
    /// Mutation: skip any of the checks, check the absolute path of what is
    /// inside the project, ignore `TEMP` or `USERPROFILE`, refuse the
    /// temporary directory for a check or for a plain link the code page
    /// holds, let LTO or a path it cannot hold through, or refuse another
    /// compiler; this test fails.
    #[test]
    fn what_cannot_be_written_in_ascii_is_refused() {
        let gcc = CompilerKind::Gcc;
        let japanese = layout("C:/日本語/p");
        let ascii = env(&[("TMP", "C:/tmp")]);
        let inside = Path::new("C:/日本語/p/target");
        let libs = [PathBuf::from("C:/日本語/p/libs")];
        let bins = [bin("app")];
        let linking = |lib_dirs, lto, bins| {
            Some(Linking {
                lib_dirs,
                lto,
                bins,
            })
        };

        assert!(check(gcc, &japanese, inside, linking(&libs, false, &bins), &ascii).is_ok());

        assert_eq!(
            what(check(gcc, &japanese, Path::new("D:/出力"), None, &ascii)),
            Some((NotAscii::Target, PathBuf::from("D:/出力")))
        );
        assert_eq!(
            what(check(
                gcc,
                &japanese,
                Path::new("C:/日本語/p/出力"),
                None,
                &ascii
            )),
            Some((NotAscii::Target, PathBuf::from("C:/日本語/p/出力"))),
            "inside the project, but not ASCII from its root"
        );
        let other_libs = [PathBuf::from("C:/日本語/p/ライブラリ")];
        assert_eq!(
            what(check(
                gcc,
                &japanese,
                inside,
                linking(&other_libs, false, &[]),
                &ascii
            )),
            Some((NotAscii::LibDir, other_libs[0].clone()))
        );
        let japanese_bin = [bin("app"), bin("漢字")];
        assert_eq!(
            what(check(
                gcc,
                &japanese,
                inside,
                linking(&[], false, &japanese_bin),
                &ascii
            )),
            Some((NotAscii::Bin, PathBuf::from("C:/p/src/bin/漢字.c")))
        );
        // A temporary directory left without an ASCII short name: LTO's
        // link fails on it whatever the code page, a plain link only when
        // the code page cannot hold it (U+20B9F no Windows code page holds).
        let unheld = "C:/Users/\u{20B9F}/Temp";
        for temp in [
            env(&[("TEMP", "C:/Users/山田/Temp")]),
            env(&[("USERPROFILE", "C:/Users/山田/Temp")]),
        ] {
            assert_eq!(
                what(check(
                    gcc,
                    &japanese,
                    inside,
                    linking(&[], true, &[]),
                    &temp
                )),
                Some((NotAscii::Temp, PathBuf::from("C:/Users/山田/Temp")))
            );
            // A check links nothing.
            assert!(check(gcc, &japanese, inside, None, &temp).is_ok());
        }
        assert_eq!(
            what(check(
                gcc,
                &japanese,
                inside,
                linking(&[], false, &[]),
                &env(&[("TMP", unheld)])
            )),
            Some((NotAscii::Temp, PathBuf::from(unheld)))
        );
        let held = env(&[("TMP", "C:/Users/山田/Temp")]);
        let plain = check(gcc, &japanese, inside, linking(&[], false, &[]), &held);
        assert_eq!(
            plain.is_ok(),
            fits_ansi_code_page(Path::new("C:/Users/山田/Temp")),
            "a plain link is refused exactly when the code page cannot hold the path"
        );
        let err = check(gcc, &japanese, Path::new("D:/出力"), None, &ascii).unwrap_err();
        assert_eq!(err.exit_class(), ExitClass::Setup);
        let japanese_temp = env(&[("TMP", "C:/Users/山田/Temp")]);
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
                    linking(&other_libs, false, &japanese_bin),
                    &japanese_temp
                )
                .is_ok()
            );
        }
    }
}
