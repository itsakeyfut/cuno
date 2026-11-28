//! `cuno self update`: whether the latest release is newer, and putting it
//! in place of the running executable (`docs/en/specs/commands.md`, "`cuno
//! self update`").

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use cuno_core::scratch;
use sha2::{Digest, Sha256};

use crate::SelfError;
use crate::fetch::Fetch;
use crate::install::Method;
use crate::release::{self, Version};

/// What the latest release is to the running cuno.
#[derive(Debug, PartialEq, Eq)]
pub enum Check {
    /// It is the running version, or older: nothing to do. cuno never goes
    /// back to an older version.
    Unchanged {
        /// The latest release.
        latest: Version,
    },
    /// It is newer, and is what to install.
    Newer {
        /// The latest release.
        latest: Version,
    },
}

/// Compares the running version with the latest release. A cuno its
/// installer did not put in place is refused before anything is fetched.
pub fn check(
    method: Method,
    exe: &Path,
    current: Version,
    fetch: &dyn Fetch,
    releases: &str,
) -> Result<Check, SelfError> {
    match method {
        Method::Installer => {}
        Method::Cargo => return Err(SelfError::Cargo),
        Method::Other => {
            return Err(SelfError::NotInstaller {
                exe: exe.to_owned(),
            });
        }
    }
    let latest = release::latest(fetch, releases)?;
    Ok(if latest > current {
        Check::Newer { latest }
    } else {
        Check::Unchanged { latest }
    })
}

/// Where the executable a replacement renamed aside is left: deleted by the
/// next `cuno self update` or `cuno self uninstall`, since a running one
/// cannot be.
pub fn renamed_aside(exe: &Path) -> PathBuf {
    let mut name = exe.file_name().unwrap_or_default().to_owned();
    name.push(".old");
    exe.with_file_name(name)
}

/// Deletes what an earlier update renamed aside, if it is there and no
/// longer running; one still running is left for next time.
pub fn remove_renamed(exe: &Path) {
    let _ = std::fs::remove_file(renamed_aside(exe));
}

/// Downloads the release of `version` into a directory of this run's own in
/// `temp`, checks the zip against its SHA-256, unpacks `cuno.exe` with
/// `tar` and puts it at `exe`. Nothing at `exe` changes unless every step
/// before the last succeeded.
pub fn install(
    exe: &Path,
    version: Version,
    fetch: &dyn Fetch,
    releases: &str,
    temp: &Path,
    tar: &Path,
) -> Result<(), SelfError> {
    let work = scratch::in_temp(temp, "self-update", "download cuno")
        .map_err(|e| SelfError::Temp(e.to_string()))?;
    let zip = release::zip_name(&version.to_string());
    let base = release::download_base(releases, version);
    let zip_path = work.path().join(&zip);
    let sum_path = work.path().join(format!("{zip}.sha256"));
    fetch.download(&format!("{base}/{zip}"), &zip_path)?;
    fetch.download(&format!("{base}/{zip}.sha256"), &sum_path)?;
    let published = std::fs::read_to_string(&sum_path).map_err(|e| io(&sum_path, e))?;
    let want = published.split_whitespace().next().unwrap_or("");
    if !sha256(&zip_path)?.eq_ignore_ascii_case(want) {
        return Err(SelfError::Checksum { zip });
    }
    let new = unpack(tar, &zip_path, &zip, work.path())?;
    replace(exe, &new)
}

/// The SHA-256 of the file at `path`, in lowercase hex.
fn sha256(path: &Path) -> Result<String, SelfError> {
    let mut file = std::fs::File::open(path).map_err(|e| io(path, e))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let n = file.read(&mut buffer).map_err(|e| io(path, e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// `cuno.exe` taken out of the zip at `zip_path` into `dir`, with the `tar`
/// Windows includes, which reads zips (a GNU tar found first on `PATH`
/// does not).
fn unpack(tar: &Path, zip_path: &Path, zip: &str, dir: &Path) -> Result<PathBuf, SelfError> {
    let unpack_error = |reason: String| SelfError::Unpack {
        zip: zip.to_owned(),
        reason,
    };
    let output = Command::new(tar)
        .arg("-xf")
        .arg(zip_path)
        .arg("-C")
        .arg(dir)
        .arg("cuno.exe")
        .output()
        .map_err(|e| unpack_error(format!("cannot run {}: {e}", tar.display())))?;
    let exe = dir.join("cuno.exe");
    if !output.status.success() || !exe.is_file() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let reason = stderr
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("it holds no cuno.exe");
        return Err(unpack_error(reason.trim().to_owned()));
    }
    Ok(exe)
}

/// Puts `new` at `exe`. A running executable cannot be deleted or
/// overwritten on Windows, but can be renamed, so `exe` is renamed aside
/// first, and renamed back if the copy fails.
pub fn replace(exe: &Path, new: &Path) -> Result<(), SelfError> {
    let aside = renamed_aside(exe);
    let replace_error = |e: std::io::Error| SelfError::Replace {
        exe: exe.to_owned(),
        reason: e.to_string(),
    };
    // One a running cuno still holds cannot be deleted, and a rename onto
    // it would fail with a less clear reason.
    if aside.exists() {
        std::fs::remove_file(&aside).map_err(|e| SelfError::Replace {
            exe: exe.to_owned(),
            reason: format!(
                "{} cannot be removed, perhaps because an earlier cuno is still running: {e}",
                aside.display()
            ),
        })?;
    }
    std::fs::rename(exe, &aside).map_err(replace_error)?;
    if let Err(e) = std::fs::copy(new, exe) {
        let _ = std::fs::rename(&aside, exe);
        return Err(replace_error(e));
    }
    Ok(())
}

fn io(path: &Path, e: std::io::Error) -> SelfError {
    SelfError::Io {
        path: path.to_owned(),
        reason: e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    const RELEASES: &str = "https://r";

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    /// A release server made of files: the latest tag, and each URL's bytes.
    /// It records every URL asked for.
    #[derive(Default)]
    struct Files {
        latest: &'static str,
        served: BTreeMap<String, Vec<u8>>,
        asked: RefCell<Vec<String>>,
    }

    impl Fetch for Files {
        fn redirect(&self, url: &str) -> Result<Option<String>, SelfError> {
            self.asked.borrow_mut().push(url.to_owned());
            Ok(Some(format!("{RELEASES}/tag/v{}", self.latest)))
        }
        fn download(&self, url: &str, to: &Path) -> Result<(), SelfError> {
            self.asked.borrow_mut().push(url.to_owned());
            let bytes = self.served.get(url).ok_or_else(|| SelfError::Download {
                url: url.to_owned(),
                reason: "curl: (22) The requested URL returned error: 404".to_owned(),
            })?;
            std::fs::write(to, bytes).unwrap();
            Ok(())
        }
    }

    /// Windows' tar, which reads zips.
    fn tar() -> PathBuf {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
        Path::new(&root).join(r"System32\tar.exe")
    }

    /// A zip holding a `cuno.exe` of `content`, made with Windows' tar.
    fn zip_holding(content: &[u8]) -> Vec<u8> {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("cuno.exe"), content).unwrap();
        let zip = dir.path().join("out.zip");
        let status = Command::new(tar())
            .arg("-a")
            .arg("-cf")
            .arg(&zip)
            .arg("-C")
            .arg(dir.path())
            .arg("cuno.exe")
            .status()
            .unwrap();
        assert!(status.success());
        std::fs::read(zip).unwrap()
    }

    /// A server whose latest release is `version`, serving its zip and a
    /// `.sha256`, which `corrupt` makes wrong.
    fn server(version: &'static str, content: &[u8], corrupt: bool) -> Files {
        let zip = zip_holding(content);
        let digest: String = Sha256::digest(&zip)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let digest = if corrupt { "0".repeat(64) } else { digest };
        let name = release::zip_name(version);
        let base = format!("{RELEASES}/download/v{version}");
        let mut served = BTreeMap::new();
        served.insert(
            format!("{base}/{name}.sha256"),
            format!("{digest}  {name}\n").into_bytes(),
        );
        served.insert(format!("{base}/{name}"), zip);
        Files {
            latest: version,
            served,
            ..Files::default()
        }
    }

    /// An installed `bin\cuno.exe` holding `old`, as a plain file.
    fn installed(old: &[u8]) -> (tempfile::TempDir, PathBuf) {
        let home = tempfile::tempdir().unwrap();
        let bin = home.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let exe = bin.join("cuno.exe");
        std::fs::write(&exe, old).unwrap();
        (home, exe)
    }

    /// Up to date, or newer than the latest, is unchanged and downloads
    /// nothing; an older cuno learns of the newer release. A cuno its
    /// installer did not place is refused before anything is asked of the
    /// network.
    ///
    /// Mutation: invert or loosen the comparison (so a newer build would go
    /// back), or look up the release before checking the method; this test
    /// fails.
    #[test]
    fn only_an_older_installed_cuno_is_updated() {
        let exe = Path::new(r"C:\h\bin\cuno.exe");
        let files = Files {
            latest: "0.2.0",
            ..Files::default()
        };
        let check = |current| check(Method::Installer, exe, v(current), &files, RELEASES);
        assert_eq!(check("0.2.0"), Ok(Check::Unchanged { latest: v("0.2.0") }));
        assert_eq!(check("0.3.0"), Ok(Check::Unchanged { latest: v("0.2.0") }));
        assert_eq!(check("0.1.9"), Ok(Check::Newer { latest: v("0.2.0") }));
        assert_eq!(*files.asked.borrow(), vec![format!("{RELEASES}/latest"); 3]);

        let quiet = Files::default();
        assert_eq!(
            super::check(Method::Cargo, exe, v("0.1.0"), &quiet, RELEASES),
            Err(SelfError::Cargo)
        );
        assert_eq!(
            super::check(Method::Other, exe, v("0.1.0"), &quiet, RELEASES),
            Err(SelfError::NotInstaller {
                exe: exe.to_owned()
            })
        );
        assert!(quiet.asked.borrow().is_empty());
    }

    /// The release's `cuno.exe` replaces the installed one, which is left
    /// renamed aside; nothing is left in the temporary directory.
    ///
    /// Mutation: skip the copy, or leave the work directory behind; this
    /// test fails.
    #[test]
    fn a_newer_release_replaces_the_executable() {
        let (_home, exe) = installed(b"old");
        let temp = tempfile::tempdir().unwrap();
        let files = server("0.2.0", b"new", false);
        install(&exe, v("0.2.0"), &files, RELEASES, temp.path(), &tar()).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"new");
        assert_eq!(std::fs::read(renamed_aside(&exe)).unwrap(), b"old");
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
    }

    /// A zip whose SHA-256 is not the published one changes nothing.
    ///
    /// Mutation: skip the checksum comparison, or replace before comparing;
    /// this test fails.
    #[test]
    fn a_wrong_checksum_changes_nothing() {
        let (_home, exe) = installed(b"old");
        let temp = tempfile::tempdir().unwrap();
        let files = server("0.2.0", b"new", true);
        let result = install(&exe, v("0.2.0"), &files, RELEASES, temp.path(), &tar());
        assert_eq!(
            result,
            Err(SelfError::Checksum {
                zip: "cuno-0.2.0-x86_64-pc-windows-msvc.zip".to_owned()
            })
        );
        assert_eq!(std::fs::read(&exe).unwrap(), b"old");
        assert!(!renamed_aside(&exe).exists());
    }

    /// A release missing a file is a download error, and changes nothing.
    ///
    /// Mutation: carry on past a failed download; this test fails.
    #[test]
    fn a_failed_download_changes_nothing() {
        let (_home, exe) = installed(b"old");
        let temp = tempfile::tempdir().unwrap();
        let mut files = server("0.2.0", b"new", false);
        files.served.retain(|url, _| !url.ends_with(".sha256"));
        let result = install(&exe, v("0.2.0"), &files, RELEASES, temp.path(), &tar());
        assert!(
            matches!(result, Err(SelfError::Download { .. })),
            "{result:?}"
        );
        assert_eq!(std::fs::read(&exe).unwrap(), b"old");
    }

    /// Waits, so that a copy of this test binary runs while another test
    /// replaces it. Run only by that test.
    #[test]
    #[ignore = "run by a_running_executable_is_replaced"]
    fn wait_while_being_replaced() {
        std::thread::sleep(std::time::Duration::from_secs(20));
    }

    /// A running executable is renamed aside and the new one put in its
    /// place: the design rests on Windows allowing this, so it is proven on
    /// a running process, a copy of this test binary.
    ///
    /// Mutation: delete or overwrite `exe` instead of renaming it first;
    /// this test fails.
    #[test]
    fn a_running_executable_is_replaced() {
        let (_home, exe) = installed(b"");
        std::fs::copy(std::env::current_exe().unwrap(), &exe).unwrap();
        let mut running = Command::new(&exe)
            .args([
                "--ignored",
                "--exact",
                "update::tests::wait_while_being_replaced",
            ])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        // Long enough for the process to have the image mapped.
        std::thread::sleep(std::time::Duration::from_millis(500));
        let new = exe.with_file_name("new.exe");
        std::fs::write(&new, b"new").unwrap();
        let result = replace(&exe, &new);
        running.kill().unwrap();
        running.wait().unwrap();
        result.unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"new");
        assert!(renamed_aside(&exe).exists());
    }

    /// What an earlier update left aside is removed first; a copy that fails
    /// puts the executable back.
    ///
    /// Mutation: keep a stale `.old` (so the rename fails), or drop the
    /// rename back; this test fails.
    #[test]
    fn a_stale_copy_is_removed_and_a_failed_copy_undone() {
        let (_home, exe) = installed(b"current");
        std::fs::write(renamed_aside(&exe), b"stale").unwrap();
        let new = exe.with_file_name("new.exe");
        std::fs::write(&new, b"new").unwrap();
        replace(&exe, &new).unwrap();
        assert_eq!(std::fs::read(renamed_aside(&exe)).unwrap(), b"current");

        let missing = exe.with_file_name("missing.exe");
        let result = replace(&exe, &missing);
        assert!(
            matches!(result, Err(SelfError::Replace { .. })),
            "{result:?}"
        );
        assert_eq!(std::fs::read(&exe).unwrap(), b"new");
    }
}
