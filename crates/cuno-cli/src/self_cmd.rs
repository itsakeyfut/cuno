//! `cuno self update` and `cuno self uninstall`: cuno updating itself to the
//! latest release, and removing itself. How it was installed, the release,
//! the replacement and the removal are `cuno_self`'s; this module only
//! gathers the values, asks and prints. See `docs/en/specs/commands.md`,
//! "`cuno self update`" and "`cuno self uninstall`".

use std::path::{Path, PathBuf};

use cuno_core::{CunoError, Env, Shell};
use cuno_self::SelfError;
use cuno_self::fetch::{Curl, Fetch};
use cuno_self::install::{self, Method};
use cuno_self::release::{self, Version};
use cuno_self::uninstall::{self, RegistryPath, UserPath};
use cuno_self::update::{self, Check};

use crate::Input;

/// The version of the running cuno.
fn current() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("cuno's own version is x.y.z")
}

/// Windows' own `tar.exe`, which reads zips, under `SystemRoot`.
fn system_tar(env: &Env) -> PathBuf {
    let root = env
        .set_value("SystemRoot")
        .map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
    root.join(r"System32\tar.exe")
}

/// How `exe` was installed, from the cuno home and the cargo home in `env`.
fn method_of(exe: &Path, env: &Env) -> Result<Method, Box<dyn CunoError>> {
    let home = cuno_core::cuno_home(env).map_err(|e| Box::new(e) as Box<dyn CunoError>)?;
    let cargo = install::cargo_home(env);
    Ok(install::method(exe, &home, cargo.as_deref(), env.case()))
}

/// `cuno self update`. How cuno was installed is decided first, so that a
/// cuno cargo installed is pointed to cargo even with no curl at hand.
pub fn update(
    exe: &Result<PathBuf, String>,
    env: &Env,
    temp: &Path,
    shell: &mut Shell,
) -> Result<i32, Box<dyn CunoError>> {
    let exe = exe.clone().map_err(|e| boxed(SelfError::Exe(e)))?;
    let method = method_of(&exe, env)?;
    update::refuse(method, &exe).map_err(boxed)?;
    let curl = Curl::find(env).map_err(boxed)?;
    updated(&exe, &curl, current(), temp, &system_tar(env), shell)
}

fn boxed(e: SelfError) -> Box<dyn CunoError> {
    Box::new(e)
}

/// The update of an installed cuno at `exe`, running `current`, from what
/// `fetch` serves: what an earlier update left aside is removed, then the
/// latest release is installed if it is newer, and the outcome printed.
fn updated(
    exe: &Path,
    fetch: &dyn Fetch,
    current: Version,
    temp: &Path,
    tar: &Path,
    shell: &mut Shell,
) -> Result<i32, Box<dyn CunoError>> {
    update::remove_renamed(exe);
    match update::check(Method::Installer, exe, current, fetch, release::RELEASES).map_err(boxed)? {
        Check::Unchanged { latest } if latest == current => {
            shell.status("Unchanged", format!("cuno {current} is the latest release"));
        }
        Check::Unchanged { latest } => {
            shell.status(
                "Unchanged",
                format!("cuno {current} is newer than the latest release, {latest}"),
            );
        }
        Check::Newer { latest } => {
            shell.status("Downloading", format!("cuno {latest}"));
            update::install(exe, latest, fetch, release::RELEASES, temp, tar).map_err(boxed)?;
            shell.status("Updated", format!("cuno {current} -> {latest}"));
        }
    }
    Ok(0)
}

/// `cuno self uninstall`. The cuno home is checked to be cuno's own before
/// anything is asked, and nothing is removed until the answer is yes (or
/// `-y` was given).
pub fn uninstall(
    exe: &Result<PathBuf, String>,
    env: &Env,
    yes: bool,
    input: Input<'_>,
    shell: &mut Shell,
) -> Result<i32, Box<dyn CunoError>> {
    uninstall_with(
        exe,
        env,
        yes,
        input,
        &RegistryPath { env: env.clone() },
        shell,
    )
}

/// [`uninstall()`] with the user's `PATH` given, so tests never touch the
/// registry.
fn uninstall_with(
    exe: &Result<PathBuf, String>,
    env: &Env,
    yes: bool,
    input: Input<'_>,
    path: &dyn UserPath,
    shell: &mut Shell,
) -> Result<i32, Box<dyn CunoError>> {
    let exe = exe.clone().map_err(|e| boxed(SelfError::Exe(e)))?;
    let home = cuno_core::cuno_home(env).map_err(|e| Box::new(e) as Box<dyn CunoError>)?;
    let method = method_of(&exe, env)?;
    if !home.exists() {
        shell.warning(format!(
            "nothing to remove: {} does not exist",
            home.display()
        ));
        return Ok(0);
    }
    uninstall::guard(&home, env).map_err(boxed)?;
    let bin = home.join("bin");
    if !yes {
        if !input.is_terminal {
            return Err(boxed(SelfError::NoAnswer));
        }
        let mut question = format!("This removes:\n  {}\n", home.display());
        if method == Method::Installer {
            question += &format!("  {} from PATH\n", bin.display());
        }
        question += "Continue? [y/N] ";
        shell.raw(question.as_bytes());
        let mut answer = String::new();
        // An unreadable answer is no answer: nothing is removed.
        let _ = input.stdin.read_line(&mut answer);
        let answer = answer.trim();
        if !(answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes")) {
            return Ok(0);
        }
    }
    let report = uninstall::uninstall(&home, &exe, method, path, env.case()).map_err(boxed)?;
    if report.path_removed {
        shell.status("Removed", format!("{} from PATH", bin.display()));
    }
    if report.exe_left {
        uninstall::remove_after_exit(&home, std::process::id(), env).map_err(boxed)?;
    }
    shell.status("Removed", home.display());
    match method {
        Method::Installer => {}
        Method::Cargo => shell.help(
            "cuno itself was installed by cargo install; run cargo uninstall cuno to remove it",
        ),
        Method::Other => shell.help(format!(
            "cuno itself is at {}; remove it yourself",
            exe.display()
        )),
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cuno_core::NameCase;

    fn env(pairs: &[(&str, &str)]) -> Env {
        Env::from_pairs(
            pairs.iter().map(|(k, v)| ((*k).into(), (*v).into())),
            NameCase::Insensitive,
        )
    }

    /// The cuno home and the cargo home come from the environment cuno was
    /// given, `CUNO_HOME` and `CARGO_HOME` included.
    ///
    /// Mutation: ignore `CUNO_HOME` or `CARGO_HOME`; this test fails.
    #[test]
    fn the_homes_come_from_the_environment() {
        let given = env(&[
            ("CUNO_HOME", r"D:\c"),
            ("CARGO_HOME", r"D:\r"),
            ("USERPROFILE", r"C:\u"),
        ]);
        let of = |exe: &str| method_of(Path::new(exe), &given).ok();
        assert_eq!(of(r"D:\c\bin\cuno.exe"), Some(Method::Installer));
        assert_eq!(of(r"D:\r\bin\cuno.exe"), Some(Method::Cargo));
        assert_eq!(of(r"C:\u\.cuno\bin\cuno.exe"), Some(Method::Other));
    }

    /// `SystemRoot` decides which tar, so a GNU tar first on `PATH` is not
    /// the one run.
    ///
    /// Mutation: run `tar` from `PATH`; this test fails.
    #[test]
    fn tar_is_windows_own() {
        assert_eq!(
            system_tar(&env(&[("SystemRoot", r"C:\WINDOWS")])),
            PathBuf::from(r"C:\WINDOWS\System32\tar.exe")
        );
    }

    /// The test's shell, and what was written to it.
    #[derive(Clone, Default)]
    struct Captured(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for Captured {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Captured {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    /// A release server whose latest is `latest`, serving a zip of a
    /// `cuno.exe` holding `b"new"` and its SHA-256.
    struct Release {
        latest: &'static str,
        zip: Vec<u8>,
    }

    impl Fetch for Release {
        fn redirect(&self, _: &str) -> Result<Option<String>, SelfError> {
            Ok(Some(format!("{}/tag/v{}", release::RELEASES, self.latest)))
        }
        fn download(&self, url: &str, to: &Path) -> Result<(), SelfError> {
            let bytes = if url.ends_with(".sha256") {
                use sha2::{Digest, Sha256};
                let hex: String = Sha256::digest(&self.zip)
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect();
                format!("{hex}  x\n").into_bytes()
            } else {
                self.zip.clone()
            };
            std::fs::write(to, bytes).unwrap();
            Ok(())
        }
    }

    fn tar() -> PathBuf {
        system_tar(&Env::from_process())
    }

    fn release(latest: &'static str) -> Release {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("cuno.exe"), b"new").unwrap();
        let zip = dir.path().join("out.zip");
        let status = std::process::Command::new(tar())
            .args(["-a", "-cf"])
            .arg(&zip)
            .arg("-C")
            .arg(dir.path())
            .arg("cuno.exe")
            .status()
            .unwrap();
        assert!(status.success());
        Release {
            latest,
            zip: std::fs::read(zip).unwrap(),
        }
    }

    /// Runs the update of an installed `cuno.exe` holding `b"old"`, with a
    /// stale `cuno.exe.old` beside it, running `current`: the result, what
    /// was printed, the executable's bytes, and whether the stale copy is
    /// gone.
    fn run(latest: &'static str, current: &str) -> (i32, String, Vec<u8>) {
        let home = tempfile::tempdir().unwrap();
        let exe = home.path().join("cuno.exe");
        std::fs::write(&exe, b"old").unwrap();
        std::fs::write(home.path().join("cuno.exe.old"), b"stale").unwrap();
        let temp = tempfile::tempdir().unwrap();
        let out = Captured::default();
        let mut shell = Shell::new(Box::new(out.clone()), None, &env(&[]), false);
        let code = updated(
            &exe,
            &release(latest),
            Version::parse(current).unwrap(),
            temp.path(),
            &tar(),
            &mut shell,
        )
        .unwrap();
        let aside = std::fs::read(home.path().join("cuno.exe.old")).unwrap_or_default();
        assert_ne!(aside, b"stale", "the stale copy was not removed");
        (code, out.text(), std::fs::read(&exe).unwrap())
    }

    /// Up to date, newer than the latest, and older: each says so in the
    /// spec's words and exits 0, and only the older one is replaced; any
    /// earlier copy left aside is removed first.
    ///
    /// Mutation: reword a status line, invert the comparison, or skip
    /// removing the copy left aside; this test fails.
    #[test]
    fn each_outcome_is_said_as_the_spec_says() {
        assert_eq!(
            run("0.2.0", "0.2.0"),
            (
                0,
                "   Unchanged cuno 0.2.0 is the latest release\n".to_owned(),
                b"old".to_vec()
            )
        );
        assert_eq!(
            run("0.2.0", "0.3.0"),
            (
                0,
                "   Unchanged cuno 0.3.0 is newer than the latest release, 0.2.0\n".to_owned(),
                b"old".to_vec()
            )
        );
        assert_eq!(
            run("0.2.0", "0.1.0"),
            (
                0,
                " Downloading cuno 0.2.0\n     Updated cuno 0.1.0 -> 0.2.0\n".to_owned(),
                b"new".to_vec()
            )
        );
    }

    /// The version compared is cuno's own.
    ///
    /// Mutation: compare any other version; this test fails.
    #[test]
    fn the_running_version_is_cunos_own() {
        assert_eq!(current().to_string(), env!("CARGO_PKG_VERSION"));
    }

    /// No path for the running executable is an error, not a guess.
    ///
    /// Mutation: fall back to some path; this test fails.
    #[test]
    fn no_executable_path_is_an_error() {
        let out = Captured::default();
        let mut shell = Shell::new(Box::new(out.clone()), None, &env(&[]), false);
        let temp = tempfile::tempdir().unwrap();
        let error = update(
            &Err("denied".to_owned()),
            &env(&[]),
            temp.path(),
            &mut shell,
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "cannot find where cuno itself is: denied"
        );
    }

    /// With no `SystemRoot`, Windows' usual place.
    ///
    /// Mutation: change the default; this test fails.
    #[test]
    fn tar_defaults_to_the_usual_windows() {
        assert_eq!(
            system_tar(&env(&[])),
            PathBuf::from(r"C:\Windows\System32\tar.exe")
        );
    }

    /// The case the environment compares names with is the case paths are
    /// compared with.
    ///
    /// Mutation: pass another `NameCase`; this test fails.
    #[test]
    fn paths_are_compared_as_the_host_compares_names() {
        let given = env(&[("CUNO_HOME", r"D:\c")]);
        assert_eq!(
            method_of(Path::new(r"d:\C\BIN\cuno.exe"), &given).ok(),
            Some(Method::Installer)
        );
    }

    /// A `PATH` that says it held the entry, and records the ask.
    #[derive(Default)]
    struct FakePath(std::cell::RefCell<Vec<PathBuf>>);

    impl UserPath for FakePath {
        fn remove(&self, dir: &Path) -> Result<bool, SelfError> {
            self.0.borrow_mut().push(dir.to_owned());
            Ok(true)
        }
    }

    /// A cuno home holding `config.toml`, with a cuno placed by hand
    /// outside it, so nothing is left running and no `PATH` is edited.
    struct Setup {
        dir: tempfile::TempDir,
        env: Env,
        exe: PathBuf,
    }

    impl Setup {
        fn new() -> Setup {
            let dir = tempfile::tempdir().unwrap();
            let home = dir.path().join("home");
            std::fs::create_dir_all(&home).unwrap();
            std::fs::write(home.join("config.toml"), "").unwrap();
            let user = dir.path().join("user");
            let env = Env::from_pairs(
                [
                    ("CUNO_HOME".into(), home.into_os_string()),
                    ("USERPROFILE".into(), user.into_os_string()),
                    (
                        "CARGO_HOME".into(),
                        dir.path().join("cargo").into_os_string(),
                    ),
                ],
                NameCase::Insensitive,
            );
            let exe = dir.path().join(r"tools\cuno.exe");
            Setup { dir, env, exe }
        }

        fn home(&self) -> PathBuf {
            self.dir.path().join("home")
        }

        /// Runs the uninstall with `answer` on a standard input that is a
        /// terminal or not: the result's exit code, and what was printed.
        fn run(&self, yes: bool, answer: &str, terminal: bool) -> (i32, String) {
            let out = Captured::default();
            let mut shell = Shell::new(Box::new(out.clone()), None, &self.env, false);
            let mut stdin = std::io::Cursor::new(answer.as_bytes().to_vec());
            let input = Input {
                stdin: &mut stdin,
                is_terminal: terminal,
            };
            let code = match uninstall_with(
                &Ok(self.exe.clone()),
                &self.env,
                yes,
                input,
                &FakePath::default(),
                &mut shell,
            ) {
                Ok(code) => code,
                Err(e) => {
                    shell.error(&e);
                    if let Some(help) = e.help() {
                        shell.help(help);
                    }
                    e.exit_class().code()
                }
            };
            (code, out.text())
        }
    }

    /// Without `-y`, nothing is removed until the answer is yes; with no
    /// terminal to answer it is a usage error, asked of nobody.
    ///
    /// Mutation: take end of input or `n` as yes, remove before asking, or
    /// wait on a standard input that is not a terminal; this test fails.
    #[test]
    fn nothing_is_removed_without_a_yes() {
        let s = Setup::new();
        let (code, out) = s.run(false, "", false);
        assert_eq!(code, 2, "{out}");
        assert_eq!(
            out,
            "error: cuno self uninstall asks before deleting, and standard input is not a terminal\nhelp: pass -y to remove without asking\n"
        );
        assert!(s.home().exists());

        for answer in ["", "n\n", "no\n", "yess\n"] {
            let (code, out) = s.run(false, answer, true);
            assert_eq!(code, 0, "{answer:?}: {out}");
            assert_eq!(
                out,
                format!("This removes:\n  {}\nContinue? [y/N] ", s.home().display())
            );
            assert!(s.home().exists(), "{answer:?}");
        }

        let (code, out) = s.run(false, "YES\n", true);
        assert_eq!(code, 0, "{out}");
        assert!(!s.home().exists());
        assert!(
            out.ends_with(&format!(
                "     Removed {}\nhelp: cuno itself is at {}; remove it yourself\n",
                s.home().display(),
                s.exe.display()
            )),
            "{out}"
        );
    }

    /// A cuno cargo installed loses its home and is pointed to cargo for
    /// the rest; no `PATH` is edited for it.
    ///
    /// Mutation: give it the installer's treatment, or drop the help; this
    /// test fails.
    #[test]
    fn a_cargo_installed_cuno_is_pointed_to_cargo_uninstall() {
        let s = Setup::new();
        let cargo = Setup {
            exe: s.dir.path().join("cargo").join("bin").join("cuno.exe"),
            ..s
        };
        let (code, out) = cargo.run(true, "", false);
        assert_eq!(code, 0, "{out}");
        let help = "help: cuno itself was installed by cargo install; run cargo uninstall cuno to remove it";
        assert_eq!(
            out,
            format!("     Removed {}\n{help}\n", cargo.home().display())
        );
        assert!(!cargo.home().exists());
    }

    /// `-y` removes without asking.
    ///
    /// Mutation: ask even with `-y`; this test fails.
    #[test]
    fn yes_removes_without_asking() {
        let s = Setup::new();
        let (code, out) = s.run(true, "", false);
        assert_eq!(code, 0, "{out}");
        assert!(!out.contains("Continue?"), "{out}");
        assert!(!s.home().exists());
    }

    /// A cuno home that is the user's own home is refused before anything
    /// is asked or removed; no home at all is a warning.
    ///
    /// Mutation: skip the guard, or ask before it; this test fails.
    #[test]
    fn a_home_not_cunos_own_is_refused_and_a_missing_one_warned_of() {
        let s = Setup::new();
        let user = s.dir.path().join("user");
        std::fs::create_dir_all(&user).unwrap();
        let env = s.env.with("CUNO_HOME", user.clone().into_os_string());
        let refused = Setup {
            env,
            ..Setup::new()
        };
        let (code, out) = refused.run(false, "y\n", true);
        assert_eq!(code, 3, "{out}");
        assert!(out.starts_with("error: refusing to remove "), "{out}");
        assert!(!out.contains("Continue?"), "{out}");

        std::fs::remove_dir_all(s.home()).unwrap();
        let (code, out) = s.run(true, "", false);
        assert_eq!(code, 0);
        assert_eq!(
            out,
            format!(
                "warning: nothing to remove: {} does not exist\n",
                s.home().display()
            )
        );
    }
}
