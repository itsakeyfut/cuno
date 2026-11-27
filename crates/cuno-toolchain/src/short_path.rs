//! A path's short (8.3) name, for tools that open paths in the ANSI code
//! page and so cannot open one that is not ASCII (`docs/en/specs/
//! toolchain.md`, "MinGW (GCC)"). The one place cuno calls Windows itself.

use std::path::{Path, PathBuf};

/// `path`'s short name, when the volume keeps one and it is ASCII; `None`
/// otherwise, and off Windows.
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn ascii_short_path(path: &Path) -> Option<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: `wide` is NUL-terminated; with no buffer, the call only
    // returns the length it needs, counting the NUL.
    let needed = unsafe { GetShortPathNameW(wide.as_ptr(), std::ptr::null_mut(), 0) };
    if needed == 0 {
        return None;
    }
    let mut buffer = vec![0u16; needed as usize];
    // SAFETY: `buffer` holds `needed` units, the length the call asked for.
    let written = unsafe { GetShortPathNameW(wide.as_ptr(), buffer.as_mut_ptr(), needed) };
    if written == 0 || written >= needed {
        return None;
    }
    buffer.truncate(written as usize);
    let short = PathBuf::from(OsString::from_wide(&buffer));
    short.to_str().is_some_and(str::is_ascii).then_some(short)
}

/// Whether `path` can be written in the ANSI code page exactly, so that a
/// tool reading its arguments and environment in it sees the same path
/// (CP932 holds Japanese, CP1252 does not).
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn fits_ansi_code_page(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Globalization::{CP_ACP, WC_NO_BEST_FIT_CHARS, WideCharToMultiByte};

    let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    let Ok(len) = i32::try_from(wide.len()) else {
        return false;
    };
    if len == 0 {
        return true;
    }
    let mut defaulted = 0;
    // SAFETY: `wide` holds `len` units; with no output buffer the call only
    // measures, and reports through `defaulted` whether any unit had no
    // character in the code page.
    let written = unsafe {
        WideCharToMultiByte(
            CP_ACP,
            WC_NO_BEST_FIT_CHARS,
            wide.as_ptr(),
            len,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
            &mut defaulted,
        )
    };
    written > 0 && defaulted == 0
}

/// Off Windows there is no ANSI code page to fit.
#[cfg(not(windows))]
pub fn fits_ansi_code_page(_path: &Path) -> bool {
    true
}

/// Off Windows there are no short names.
#[cfg(not(windows))]
pub fn ascii_short_path(_path: &Path) -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ASCII always fits the ANSI code page; a character no Windows code
    /// page holds (U+20B9F, outside Shift_JIS's set) does not.
    ///
    /// Mutation: answer true, or false, always; this test fails.
    #[cfg(windows)]
    #[test]
    fn ascii_fits_the_code_page_and_a_rare_ideograph_does_not() {
        assert!(fits_ansi_code_page(Path::new("C:/Users/runner/Temp")));
        assert!(!fits_ansi_code_page(Path::new("C:/Users/\u{20B9F}")));
    }

    /// A directory named in Japanese has an ASCII short name where the
    /// volume keeps them (the system volume usually does), and a path that
    /// does not exist has none.
    ///
    /// Mutation: return the long path, or one for a missing path; this
    /// test fails.
    #[cfg(windows)]
    #[test]
    fn a_japanese_directory_has_an_ascii_short_name() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("一時");
        std::fs::create_dir(&dir).unwrap();
        assert_eq!(ascii_short_path(&root.path().join("無い")), None);
        // Some volumes keep no short names: then there is nothing to test.
        let Some(short) = ascii_short_path(&dir) else {
            return;
        };
        assert!(short.to_str().unwrap().is_ascii(), "{}", short.display());
        assert_ne!(short, dir);
        assert!(short.is_dir());
    }
}
