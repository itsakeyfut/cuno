//! `cuno self uninstall`: removing the cuno home and, for a cuno its
//! installer placed, its `PATH` entry and the running executable
//! (`docs/en/specs/commands.md`, "`cuno self uninstall`").

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use cuno_core::{Env, NameCase, paths};

use crate::SelfError;
use crate::install::Method;

/// The user's `PATH`, as far as uninstalling needs it. The binary uses
/// [`RegistryPath`]; tests never touch the registry.
pub trait UserPath {
    /// Takes every entry naming `dir` out of the user's `PATH`; whether one
    /// was there.
    fn remove(&self, dir: &Path) -> Result<bool, SelfError>;
}

/// How PowerShell compares `PATH` entries, as `install.ps1` does:
/// environment variables expanded, surrounding quotes and spaces removed,
/// `/`, `.` and `..` resolved, no trailing separator. `Remove-PathEntry`
/// returns the value without the entries naming `$dir`, or `$null` when
/// there is none.
const PATH_FUNCTIONS: &str = r#"
function Get-NormalDir([string]$dir) {
    $d = [Environment]::ExpandEnvironmentVariables($dir).Trim().Trim('"').Trim()
    if ($d -eq '') { return $null }
    try { $d = [IO.Path]::GetFullPath($d) } catch { return $null }
    return $d.TrimEnd('\')
}
function Remove-PathEntry([string]$path, [string]$dir) {
    $want = Get-NormalDir $dir
    $entries = @($path -split ';' | Where-Object { $_ -ne '' })
    $kept = @($entries | Where-Object { (Get-NormalDir $_) -ine $want })
    if ($kept.Count -eq $entries.Count) { return $null }
    return ($kept -join ';')
}
"#;

/// Reads and writes `Path` under `HKCU\Environment`, keeping the value's
/// type, then tells running programs of the change.
const PATH_MAIN: &str = r#"
$ErrorActionPreference = 'Stop'
$key = Get-Item -LiteralPath 'HKCU:\Environment'
if (-not ($key.GetValueNames() -contains 'Path')) { 'kept'; exit 0 }
$kind = $key.GetValueKind('Path').ToString()
$path = [string]$key.GetValue('Path', '', 'DoNotExpandEnvironmentNames')
$value = Remove-PathEntry $path $env:CUNO_UNINSTALL_DIR
if ($null -eq $value) { 'kept'; exit 0 }
Set-ItemProperty -LiteralPath 'HKCU:\Environment' -Name Path -Value $value -Type $kind
Add-Type -Namespace Cuno -Name Native -MemberDefinition @'
[DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
public static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint Msg, UIntPtr wParam,
    string lParam, uint fuFlags, uint uTimeout, out UIntPtr lpdwResult);
'@
$result = [UIntPtr]::Zero
[Cuno.Native]::SendMessageTimeout([IntPtr]0xffff, 0x1a, [UIntPtr]::Zero, 'Environment', 2, 5000, [ref]$result) | Out-Null
'removed'
"#;

/// Windows PowerShell, by its location under `SystemRoot`.
pub fn powershell(env: &Env) -> PathBuf {
    let root = env
        .set_value("SystemRoot")
        .map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
    root.join(r"System32\WindowsPowerShell\v1.0\powershell.exe")
}

/// Runs `script` in PowerShell with `env` and `vars` added: directories are
/// passed in variables rather than spliced into the script, so no quoting
/// can go wrong. Its stdout, trimmed.
fn run_powershell(env: &Env, script: &str, vars: &[(&str, &Path)]) -> Result<String, String> {
    let output = Command::new(powershell(env))
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env_clear()
        .envs(env.vars())
        .envs(vars.iter().map(|(k, v)| (*k, v.as_os_str())))
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// The user's `PATH` in the registry, edited through PowerShell as the
/// installer edits it.
pub struct RegistryPath {
    /// The environment PowerShell runs with.
    pub env: Env,
}

impl UserPath for RegistryPath {
    fn remove(&self, dir: &Path) -> Result<bool, SelfError> {
        let script = format!("{PATH_FUNCTIONS}{PATH_MAIN}");
        let answer = run_powershell(&self.env, &script, &[("CUNO_UNINSTALL_DIR", dir)])
            .map_err(|reason| SelfError::PathEdit { reason })?;
        Ok(answer.lines().last() == Some("removed"))
    }
}

/// `path` without a `\?\` prefix: `\?\C:\x` as `C:\x`, `\?\UNC\s\x` as
/// `\s\x`, so the two spellings of a place compare alike.
fn plain(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\?\UNC\") {
        PathBuf::from(format!(r"\{rest}"))
    } else if let Some(rest) = text.strip_prefix(r"\?\") {
        PathBuf::from(rest)
    } else {
        path.to_owned()
    }
}

/// Refuses a cuno home that is not a directory of cuno's own: the root of a
/// drive or a share, the user's home directory, or one holding it, however
/// spelled. A mistaken `CUNO_HOME` would otherwise remove the user's files
/// wholesale.
pub fn guard(home: &Path, env: &Env) -> Result<(), SelfError> {
    let refuse = || SelfError::NotOwnHome {
        home: home.to_owned(),
    };
    let plain_home = plain(home);
    // A root has a prefix and a root and nothing under them, whether a
    // drive's (`C:\`) or a share's (`\server\share`).
    let named = paths::lexical(&plain_home)
        .components()
        .filter(|c| matches!(c, std::path::Component::Normal(_)))
        .count();
    if named == 0 {
        return Err(refuse());
    }
    for name in ["USERPROFILE", "HOME"] {
        if let Some(user) = env.set_value(name)
            && paths::within(&plain(Path::new(user)), &plain_home, env.case())
        {
            return Err(refuse());
        }
    }
    Ok(())
}

/// Whether `path` is a link: a symbolic link, or on Windows any reparse
/// point, junctions among them.
fn is_link(path: &Path) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return false;
    };
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    meta.file_type().is_symlink()
}

/// Refuses a home, or a `bin` in it, that is a link: removing through it
/// would remove what it leads to, outside the home.
pub fn refuse_links(home: &Path) -> Result<(), SelfError> {
    for path in [home.to_owned(), home.join("bin")] {
        if is_link(&path) {
            return Err(SelfError::Link { path });
        }
    }
    Ok(())
}

/// What uninstalling did.
#[derive(Debug, PartialEq, Eq)]
pub struct Report {
    /// Whether an entry came out of the user's `PATH`.
    pub path_removed: bool,
    /// Whether the running executable is still to be removed, once cuno has
    /// exited, by [`remove_after_exit`].
    pub exe_left: bool,
}

/// Removes the cuno `home`. For a cuno its installer placed, at `exe`, its
/// `PATH` entry goes first and everything but `exe` itself is removed: a
/// running executable cannot be deleted. Anything else that cannot be
/// removed is an error naming it.
pub fn uninstall(
    home: &Path,
    exe: &Path,
    method: Method,
    path: &dyn UserPath,
    case: NameCase,
) -> Result<Report, SelfError> {
    refuse_links(home)?;
    match method {
        Method::Installer => {
            let bin = home.join("bin");
            let path_removed = path.remove(&bin)?;
            remove_all_but(home, exe, case)?;
            Ok(Report {
                path_removed,
                exe_left: true,
            })
        }
        Method::Cargo | Method::Other => {
            if paths::within(exe, home, case) {
                // cuno is running from inside the home it was asked to
                // remove, somewhere its installer does not put it.
                return Err(remove_error(exe, "cuno is running from it"));
            }
            remove(home)?;
            Ok(Report {
                path_removed: false,
                exe_left: false,
            })
        }
    }
}

/// Removes everything under `dir` but `keep` and the directories holding it.
fn remove_all_but(dir: &Path, keep: &Path, case: NameCase) -> Result<(), SelfError> {
    let entries = std::fs::read_dir(dir).map_err(|e| remove_error(dir, &e.to_string()))?;
    for entry in entries {
        let entry = entry.map_err(|e| remove_error(dir, &e.to_string()))?;
        let path = entry.path();
        if paths::same(&path, keep, case) {
            continue;
        }
        if paths::within(keep, &path, case) {
            remove_all_but(&path, keep, case)?;
        } else {
            remove(&path)?;
        }
    }
    Ok(())
}

/// Removes a file, or a directory and everything in it. A link is removed
/// itself, and what it leads to is left alone.
fn remove(path: &Path) -> Result<(), SelfError> {
    let result = if is_link(path) {
        // A directory link (a junction, a directory symlink) is removed as
        // an empty directory, which removes the link and not its target.
        std::fs::remove_dir(path).or_else(|_| std::fs::remove_file(path))
    } else if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
    result.map_err(|e| remove_error(path, &e.to_string()))
}

fn remove_error(path: &Path, reason: &str) -> SelfError {
    SelfError::Remove {
        path: path.to_owned(),
        reason: reason.to_owned(),
    }
}

/// Starts a PowerShell process of its own, with no window, that waits for
/// process `pid` (the running cuno) to exit and then removes `home` with
/// what is left in it, trying again for a few seconds while something still
/// holds a file.
pub fn remove_after_exit(home: &Path, pid: u32, env: &Env) -> Result<(), SelfError> {
    let script = format!(
        "Wait-Process -Id {pid} -ErrorAction SilentlyContinue; \
         for ($i = 0; $i -lt 20; $i++) {{ \
           Remove-Item -LiteralPath $env:CUNO_UNINSTALL_HOME -Recurse -Force -ErrorAction SilentlyContinue; \
           if (-not (Test-Path -LiteralPath $env:CUNO_UNINSTALL_HOME)) {{ break }}; \
           Start-Sleep -Milliseconds 500 \
         }}"
    );
    let mut command = Command::new(powershell(env));
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &script,
        ])
        .env_clear()
        .envs(env.vars())
        .env("CUNO_UNINSTALL_HOME", home)
        // Not cuno's own directory, which may be inside the home: a process
        // working in a directory keeps it from being removed.
        .current_dir(home.parent().unwrap_or(home))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP: no console window,
        // and a Ctrl-C in cuno's console does not reach it.
        command.creation_flags(0x0800_0000 | 0x0000_0200);
    }
    command.spawn().map(|_| ()).map_err(|e| SelfError::Helper {
        reason: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A `PATH` that records what was taken out of it.
    #[derive(Default)]
    struct FakePath {
        removed: RefCell<Vec<PathBuf>>,
        present: bool,
    }

    impl UserPath for FakePath {
        fn remove(&self, dir: &Path) -> Result<bool, SelfError> {
            self.removed.borrow_mut().push(dir.to_owned());
            Ok(self.present)
        }
    }

    /// A cuno home with settings, headers, a running `cuno.exe` and the copy
    /// an update left aside, beside a sibling that is not the home's.
    fn home() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let parent = tempfile::tempdir().unwrap();
        let home = parent.path().join(".cuno");
        std::fs::create_dir_all(home.join(r"include\0.2.0\cuno")).unwrap();
        std::fs::create_dir_all(home.join("bin")).unwrap();
        std::fs::write(home.join("config.toml"), "").unwrap();
        std::fs::write(home.join(r"include\0.2.0\cuno\test.h"), "").unwrap();
        std::fs::write(home.join(r"bin\cuno.exe"), "").unwrap();
        std::fs::write(home.join(r"bin\cuno.exe.old"), "").unwrap();
        std::fs::create_dir(parent.path().join("sibling")).unwrap();
        let exe = home.join(r"bin\cuno.exe");
        (parent, home, exe)
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// For the installer's cuno: the `PATH` entry for `<home>\bin` goes, and
    /// everything but the running `cuno.exe`, the copy left aside included;
    /// nothing beside the home is touched.
    ///
    /// Mutation: keep `cuno.exe.old`, remove the running exe, skip `PATH`,
    /// or remove the home's parent's entries; this test fails.
    #[test]
    fn the_installers_cuno_leaves_only_itself() {
        let (parent, home, exe) = home();
        let path = FakePath {
            present: true,
            ..FakePath::default()
        };
        let report =
            uninstall(&home, &exe, Method::Installer, &path, NameCase::Insensitive).unwrap();
        assert_eq!(
            report,
            Report {
                path_removed: true,
                exe_left: true
            }
        );
        assert_eq!(*path.removed.borrow(), [home.join("bin")]);
        assert_eq!(names(&home), ["bin"]);
        assert_eq!(names(&home.join("bin")), ["cuno.exe"]);
        assert_eq!(names(parent.path()), [".cuno", "sibling"]);
    }

    /// For cargo's cuno, or one elsewhere: the whole home goes, and `PATH`
    /// is not asked; one running from inside the home is refused.
    ///
    /// Mutation: edit `PATH` for them, or keep part of the home; this test
    /// fails.
    #[test]
    fn another_cuno_removes_the_home_alone() {
        for method in [Method::Cargo, Method::Other] {
            let (parent, home, _) = home();
            let path = FakePath::default();
            let elsewhere = parent.path().join(r"sibling\cuno.exe");
            let report =
                uninstall(&home, &elsewhere, method, &path, NameCase::Insensitive).unwrap();
            assert!(!report.path_removed && !report.exe_left);
            assert!(path.removed.borrow().is_empty());
            assert_eq!(names(parent.path()), ["sibling"]);
        }
        let (_parent, home, _) = home();
        let inside = home.join(r"tools\cuno.exe");
        let result = uninstall(
            &home,
            &inside,
            Method::Other,
            &FakePath::default(),
            NameCase::Insensitive,
        );
        assert!(
            matches!(result, Err(SelfError::Remove { .. })),
            "{result:?}"
        );
        assert!(home.join("config.toml").exists());
    }

    /// A home that is a drive's root, the user's home, or above it is
    /// refused; one inside the user's home is not.
    ///
    /// Mutation: drop any of the three checks; this test fails.
    #[test]
    fn a_home_that_is_not_cunos_own_is_refused() {
        let env = Env::from_pairs(
            [
                ("USERPROFILE".into(), r"C:\Users\me".into()),
                ("HOME".into(), r"D:\home\me".into()),
            ],
            NameCase::Insensitive,
        );
        for home in [
            r"C:\",
            r"C:\Users\me",
            r"c:\users",
            r"D:\home\me\..\me",
            r"D:\",
            // A root holding no user's home, refused as a root.
            r"E:\",
        ] {
            assert!(
                matches!(
                    guard(Path::new(home), &env),
                    Err(SelfError::NotOwnHome { .. })
                ),
                "{home}"
            );
        }
        assert_eq!(guard(Path::new(r"C:\Users\me\.cuno"), &env), Ok(()));
        assert_eq!(guard(Path::new(r"E:\cuno"), &env), Ok(()));
    }

    /// `PATH` entries are matched as `install.ps1` matches them, and only
    /// those go; with none, nothing is written. Run in PowerShell, on a
    /// string, without the registry.
    ///
    /// Mutation: compare entries as written, or drop every entry; this
    /// test fails.
    #[test]
    fn path_entries_are_matched_as_the_installer_matches_them() {
        let env = Env::from_process();
        let cases = [
            (r"C:\a;C:\x\home\bin;C:\b", r"C:\a;C:\b"),
            (r#"C:\a;"C:/x/home/bin/";C:\b"#, r"C:\a;C:\b"),
            (r"C:\a;c:\X\HOME\BIN\;C:\x\.\home\bin", r"C:\a"),
            (r"C:\a;C:\x\home\binx", "<none>"),
        ];
        for (path, want) in cases {
            let script = format!(
                "{PATH_FUNCTIONS}$v = Remove-PathEntry $env:CUNO_T_PATH $env:CUNO_T_DIR; if ($null -eq $v) {{ '<none>' }} else {{ $v }}"
            );
            let got = run_powershell(
                &env,
                &script,
                &[
                    ("CUNO_T_PATH", Path::new(path)),
                    ("CUNO_T_DIR", Path::new(r"C:\x\home\bin")),
                ],
            )
            .unwrap();
            assert_eq!(got, want, "{path}");
        }
    }

    /// The process started to remove what is left waits for the given
    /// process to exit, then removes the home.
    ///
    /// Mutation: remove before the process exits, or not at all; this
    /// test fails.
    #[test]
    fn what_is_left_goes_once_cuno_has_exited() {
        let parent = tempfile::tempdir().unwrap();
        let home = parent.path().join(".cuno");
        std::fs::create_dir_all(home.join("bin")).unwrap();
        std::fs::write(home.join(r"bin\cuno.exe"), "").unwrap();
        let env = Env::from_process();
        // A process standing in for cuno, alive for a few seconds.
        let mut cuno = Command::new(powershell(&env))
            .args(["-NoProfile", "-Command", "Start-Sleep -Seconds 4"])
            .spawn()
            .unwrap();
        remove_after_exit(&home, cuno.id(), &env).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(2));
        assert!(home.exists(), "removed while cuno still ran");
        cuno.wait().unwrap();
        let gone = (0..40).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(500));
            !home.exists()
        });
        assert!(gone, "{} is still there", home.display());
    }

    /// `link` made a junction to `target`.
    fn junction(link: &Path, target: &Path) {
        let made = Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .unwrap();
        assert!(made.status.success(), "{made:?}");
    }

    /// A home, or its `bin`, that is a junction is refused whatever the
    /// method, and what it leads to is left as it was.
    ///
    /// Mutation: drop `refuse_links`, or check only the home; this test
    /// fails.
    #[test]
    fn a_linked_home_or_bin_is_refused() {
        for method in [Method::Installer, Method::Other] {
            let t = tempfile::tempdir().unwrap();
            let target = t.path().join("elsewhere");
            std::fs::create_dir_all(target.join("bin")).unwrap();
            std::fs::write(target.join("precious.txt"), "").unwrap();
            let home = t.path().join(".cuno");
            junction(&home, &target);
            let exe = home.join(r"bin\cuno.exe");
            let result = uninstall(
                &home,
                &exe,
                method,
                &FakePath::default(),
                NameCase::Insensitive,
            );
            assert_eq!(result, Err(SelfError::Link { path: home.clone() }));
            assert!(target.join("precious.txt").exists());
        }
        let t = tempfile::tempdir().unwrap();
        let target = t.path().join("elsewhere");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("precious.txt"), "").unwrap();
        let home = t.path().join(".cuno");
        std::fs::create_dir_all(&home).unwrap();
        junction(&home.join("bin"), &target);
        let exe = home.join(r"bin\cuno.exe");
        let result = uninstall(
            &home,
            &exe,
            Method::Installer,
            &FakePath::default(),
            NameCase::Insensitive,
        );
        assert_eq!(
            result,
            Err(SelfError::Link {
                path: home.join("bin")
            })
        );
        assert!(target.join("precious.txt").exists());
    }

    /// A junction inside the home is removed itself; what it leads to is
    /// not.
    ///
    /// Mutation: remove a link's contents as a directory's; this test
    /// fails.
    #[test]
    fn a_link_inside_the_home_is_removed_but_not_followed() {
        for method in [Method::Installer, Method::Other] {
            let (parent, home, exe) = home();
            let target = parent.path().join(r"sibling\kept");
            std::fs::create_dir_all(&target).unwrap();
            std::fs::write(target.join("precious.txt"), "").unwrap();
            junction(&home.join("toolchains"), &target);
            let elsewhere = parent.path().join(r"tools\cuno.exe");
            let exe = if method == Method::Installer {
                exe
            } else {
                elsewhere
            };
            uninstall(
                &home,
                &exe,
                method,
                &FakePath::default(),
                NameCase::Insensitive,
            )
            .unwrap();
            assert!(!home.join("toolchains").exists());
            assert!(target.join("precious.txt").exists(), "{method:?}");
        }
    }

    /// The `PATH` entry goes before any file, so a file that cannot be
    /// removed never leaves an entry pointing at a half-removed home; a
    /// `PATH` that cannot be edited stops it with nothing removed; and with
    /// no entry, none is reported.
    ///
    /// Mutation: remove the files first, swallow the `PATH` error, or
    /// report an entry regardless; this test fails.
    #[test]
    fn path_goes_first_and_its_failure_removes_nothing() {
        /// A `PATH` that answers `answer`, noting whether the home still held
        /// its settings when asked.
        struct Watching<'a> {
            home: &'a Path,
            fail: bool,
            saw_settings: RefCell<Option<bool>>,
        }
        impl UserPath for Watching<'_> {
            fn remove(&self, _: &Path) -> Result<bool, SelfError> {
                *self.saw_settings.borrow_mut() = Some(self.home.join("config.toml").exists());
                if self.fail {
                    return Err(SelfError::PathEdit {
                        reason: "denied".into(),
                    });
                }
                Ok(false)
            }
        }
        let (_parent, home, exe) = home();
        let path = Watching {
            home: &home,
            fail: false,
            saw_settings: RefCell::new(None),
        };
        let report =
            uninstall(&home, &exe, Method::Installer, &path, NameCase::Insensitive).unwrap();
        assert_eq!(*path.saw_settings.borrow(), Some(true));
        assert!(!report.path_removed);

        let (_parent, home, exe) = super::tests::home();
        let failing = Watching {
            home: &home,
            fail: true,
            saw_settings: RefCell::new(None),
        };
        let result = uninstall(
            &home,
            &exe,
            Method::Installer,
            &failing,
            NameCase::Insensitive,
        );
        assert!(
            matches!(result, Err(SelfError::PathEdit { .. })),
            "{result:?}"
        );
        assert!(home.join("config.toml").exists());
    }

    /// The root of a share, and the user's home spelled with `\\?\`, are
    /// refused like a drive's root and the plain spelling; case is compared
    /// as the host compares it.
    ///
    /// Mutation: test only for a drive's root, compare the `\\?\` spelling
    /// as written, or ignore `env.case()`; this test fails.
    #[test]
    fn every_spelling_of_a_root_or_the_users_home_is_refused() {
        let user = tempfile::tempdir().unwrap();
        let env = Env::from_pairs(
            [("USERPROFILE".into(), user.path().as_os_str().to_owned())],
            NameCase::Insensitive,
        );
        let verbatim = format!(r"\\?\{}", user.path().display());
        for home in [
            r"\\server\share",
            r"\\server\share\",
            r"\\?\UNC\server\share",
            r"\\?\C:\",
            verbatim.as_str(),
        ] {
            assert!(
                matches!(
                    guard(Path::new(home), &env),
                    Err(SelfError::NotOwnHome { .. })
                ),
                "{home}"
            );
        }
        assert_eq!(guard(Path::new(r"\\server\share\cuno"), &env), Ok(()));
        // Paths on no drive here, so the file system cannot settle the case.
        let folded = |case| {
            let env = Env::from_pairs([("USERPROFILE".into(), r"Q:\Users\Me".into())], case);
            guard(Path::new(r"q:\users\me"), &env)
        };
        assert!(matches!(
            folded(NameCase::Insensitive),
            Err(SelfError::NotOwnHome { .. })
        ));
        assert_eq!(folded(NameCase::Sensitive), Ok(()));
    }

    /// With no `SystemRoot`, Windows' usual place.
    ///
    /// Mutation: change the default; this test fails.
    #[test]
    fn powershell_defaults_to_the_usual_windows() {
        let env = Env::from_pairs([], NameCase::Insensitive);
        assert_eq!(
            powershell(&env),
            PathBuf::from(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe")
        );
    }

    /// The process that removes what is left works outside the home, so a
    /// cuno started inside it does not keep it; it removes read-only files;
    /// and it tries again while a file is still held.
    ///
    /// Mutation: leave the helper in cuno's directory, drop `-Force`, or try
    /// once; this test fails.
    #[test]
    fn what_is_left_goes_even_when_held_for_a_while() {
        let parent = tempfile::tempdir().unwrap();
        let home = parent.path().join(".cuno");
        std::fs::create_dir_all(home.join("bin")).unwrap();
        let read_only = home.join("settings.toml");
        std::fs::write(&read_only, "").unwrap();
        let mut permissions = std::fs::metadata(&read_only).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&read_only, permissions).unwrap();
        let held = home.join(r"bin\held.txt");
        std::fs::write(&held, "").unwrap();
        let env = Env::from_process();
        // A file held, without sharing, for about 3 seconds after cuno exits.
        let lock = {
            use std::os::windows::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(&held)
                .unwrap()
        };
        let release = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(4));
            drop(lock);
        });
        let mut cuno = Command::new(powershell(&env))
            .args(["-NoProfile", "-Command", "Start-Sleep -Seconds 1"])
            .current_dir(&home)
            .spawn()
            .unwrap();
        // The helper is started from inside the home, as from a cuno whose
        // directory it is.
        let here = std::env::current_dir().unwrap();
        std::env::set_current_dir(&home).unwrap();
        let started = remove_after_exit(&home, cuno.id(), &env);
        std::env::set_current_dir(here).unwrap();
        started.unwrap();
        cuno.wait().unwrap();
        release.join().unwrap();
        let gone = (0..40).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(500));
            !home.exists()
        });
        assert!(gone, "{} is still there", home.display());
    }
}
