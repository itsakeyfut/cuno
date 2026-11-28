//! `cuno self update`: cuno updating itself to the latest release. How it
//! was installed, the release and the replacement are `cuno_self`'s; this
//! module only gathers the values and prints. See
//! `docs/en/specs/commands.md`, "`cuno self update`".

use std::path::{Path, PathBuf};

use cuno_core::{CunoError, Env, Shell};
use cuno_self::SelfError;
use cuno_self::fetch::Curl;
use cuno_self::install::{self, Method};
use cuno_self::release::{self, Version};
use cuno_self::update::{self, Check};

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

/// `cuno self update`.
pub fn update(
    exe: &Result<PathBuf, String>,
    env: &Env,
    temp: &Path,
    shell: &mut Shell,
) -> Result<i32, Box<dyn CunoError>> {
    let exe = exe
        .clone()
        .map_err(|e| Box::new(SelfError::Exe(e)) as Box<dyn CunoError>)?;
    let method = method_of(&exe, env)?;
    if method == Method::Installer {
        update::remove_renamed(&exe);
    }
    let curl = Curl::new(env);
    let current = current();
    let boxed = |e: SelfError| Box::new(e) as Box<dyn CunoError>;
    match update::check(method, &exe, current, &curl, release::RELEASES).map_err(boxed)? {
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
            update::install(
                &exe,
                latest,
                &curl,
                release::RELEASES,
                temp,
                &system_tar(env),
            )
            .map_err(boxed)?;
            shell.status("Updated", format!("cuno {current} -> {latest}"));
        }
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
}
