//! How the running cuno was installed, told by where it is
//! (`docs/en/specs/distribution.md`, "Self-update"): no record is kept that
//! could disagree with where cuno actually is.

use std::path::{Path, PathBuf};

use cuno_core::{Env, NameCase, paths};

/// How the running cuno was installed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// By cuno's own installer: it is `<cuno home>\bin\cuno.exe`, and cuno
    /// updates and removes it.
    Installer,
    /// By `cargo install`: it is in `<CARGO_HOME>\bin`, and cargo updates
    /// and removes it.
    Cargo,
    /// Anywhere else, such as a zip unpacked by hand: cuno does not rewrite
    /// it.
    Other,
}

/// How `exe`, the running executable, was installed, given the cuno home
/// and the cargo home (`None` when there is none).
pub fn method(exe: &Path, home: &Path, cargo_home: Option<&Path>, case: NameCase) -> Method {
    let Some(dir) = exe.parent() else {
        return Method::Other;
    };
    if paths::same(dir, &home.join("bin"), case) {
        Method::Installer
    } else if cargo_home.is_some_and(|cargo| paths::same(dir, &cargo.join("bin"), case)) {
        Method::Cargo
    } else {
        Method::Other
    }
}

/// Where `cargo install` puts programs under: `CARGO_HOME` when it is set
/// and absolute, as cargo takes it, otherwise `.cargo` in the user's home
/// (`USERPROFILE`, then `HOME`, as for the cuno home).
pub fn cargo_home(env: &Env) -> Option<PathBuf> {
    if let Some(home) = env.set_value("CARGO_HOME") {
        let home = PathBuf::from(home);
        return home.is_absolute().then_some(home);
    }
    let set = |name| env.get(name).filter(|value| !value.is_empty());
    set("USERPROFILE")
        .or_else(|| set("HOME"))
        .map(|user| Path::new(user).join(".cargo"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = r"C:\Users\me\.cuno";
    const CARGO: &str = r"C:\Users\me\.cargo";

    fn of(exe: &str) -> Method {
        method(
            Path::new(exe),
            Path::new(HOME),
            Some(Path::new(CARGO)),
            NameCase::Insensitive,
        )
    }

    /// The installer's place, cargo's, and anywhere else, compared as
    /// places rather than as spellings.
    ///
    /// Mutation: compare the directories as strings, swap the two checks, or
    /// accept anywhere under the cuno home; this test fails.
    #[test]
    fn the_place_tells_how_cuno_was_installed() {
        assert_eq!(of(r"C:\Users\me\.cuno\bin\cuno.exe"), Method::Installer);
        assert_eq!(of(r"c:\users\ME\.CUNO\BIN\cuno.exe"), Method::Installer);
        assert_eq!(
            of(r"C:\Users\me\.cuno\x\..\bin\cuno.exe"),
            Method::Installer
        );
        assert_eq!(of(r"C:\Users\me\.cargo\bin\cuno.exe"), Method::Cargo);
        assert_eq!(of(r"C:\Users\me\.cuno\cuno.exe"), Method::Other);
        assert_eq!(of(r"C:\Users\me\.cuno\bin\x\cuno.exe"), Method::Other);
        assert_eq!(of(r"C:\tools\cuno\cuno.exe"), Method::Other);
        let no_cargo = method(
            Path::new(r"C:\Users\me\.cargo\bin\cuno.exe"),
            Path::new(HOME),
            None,
            NameCase::Insensitive,
        );
        assert_eq!(no_cargo, Method::Other);
    }

    /// `CARGO_HOME` wins when absolute; a relative one is no cargo home, as
    /// cargo would not put programs there for this run; otherwise the user's
    /// `.cargo`.
    ///
    /// Mutation: ignore `CARGO_HOME`, accept a relative one, or read `HOME`
    /// before `USERPROFILE`; this test fails.
    #[test]
    fn the_cargo_home_is_cargos() {
        let env = |pairs: &[(&str, &str)]| {
            Env::from_pairs(
                pairs.iter().map(|(k, v)| ((*k).into(), (*v).into())),
                NameCase::Insensitive,
            )
        };
        assert_eq!(
            cargo_home(&env(&[
                ("CARGO_HOME", r"D:\cargo"),
                ("USERPROFILE", r"C:\u")
            ])),
            Some(PathBuf::from(r"D:\cargo"))
        );
        assert_eq!(cargo_home(&env(&[("CARGO_HOME", "cargo")])), None);
        assert_eq!(
            cargo_home(&env(&[("USERPROFILE", r"C:\u"), ("HOME", "/home/u")])),
            Some(PathBuf::from(r"C:\u\.cargo"))
        );
        assert_eq!(cargo_home(&env(&[])), None);
    }
}
