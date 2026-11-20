//! The environment MSVC, clang and clang-cl need on Windows, built from a
//! toolset and the Windows SDK rather than by running `vcvars64.bat`. See
//! `docs/en/specs/toolchain.md`, "Environment resolution on Windows".

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use cuno_core::Env;

use super::MsvcToolset;
use crate::ToolchainError;

/// Where the Windows SDK installer records its location.
const KITS_KEY: &str = r"SOFTWARE\Microsoft\Windows Kits\Installed Roots";
const KITS_VALUE: &str = "KitsRoot10";

/// The machine's registry, as far as finding the Windows SDK needs it: a
/// value, so tests can stand in for it.
pub trait Registry {
    /// A string value under `HKEY_LOCAL_MACHINE`, if it exists.
    fn local_machine_string(&self, key: &str, value: &str) -> Option<OsString>;
}

/// The real registry. Elsewhere than on Windows there is none.
pub struct SystemRegistry;

impl Registry for SystemRegistry {
    #[cfg(windows)]
    fn local_machine_string(&self, key: &str, value: &str) -> Option<OsString> {
        let hklm = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE);
        hklm.open_subkey(key)
            .ok()?
            .get_value::<OsString, _>(value)
            .ok()
    }

    #[cfg(not(windows))]
    fn local_machine_string(&self, _key: &str, _value: &str) -> Option<OsString> {
        None
    }
}

/// One Windows SDK version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowsSdk {
    /// The SDK directory (`...\Windows Kits\10`).
    pub dir: PathBuf,
    /// The version directory name (`10.0.28000.0`).
    pub version: String,
}

/// The Windows SDK to build with: the newest version under the registered
/// SDK directory (or `%ProgramFiles(x86)%\Windows Kits\10`) whose x64 headers
/// and libraries are both there.
pub fn find_windows_sdk(env: &Env, registry: &dyn Registry) -> Result<WindowsSdk, ToolchainError> {
    let dir = registry
        .local_machine_string(KITS_KEY, KITS_VALUE)
        .map(PathBuf::from)
        .or_else(|| {
            env.get("ProgramFiles(x86)")
                .map(|pf| Path::new(pf).join("Windows Kits").join("10"))
        })
        .unwrap_or_default();
    let mut versions: Vec<(Vec<u32>, String)> = std::fs::read_dir(dir.join("Include"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|version| complete(&dir, version))
        .filter_map(|version| Some((numeric(&version)?, version)))
        .collect();
    versions.sort();
    match versions.pop() {
        Some((_, version)) => Ok(WindowsSdk { dir, version }),
        None => Err(ToolchainError::NoWindowsSdk { looked_in: dir }),
    }
}

/// Whether `version` has both the headers and the x64 libraries: an SDK can
/// leave a version's `Include` behind when only part of it is installed.
fn complete(dir: &Path, version: &str) -> bool {
    dir.join("Include")
        .join(version)
        .join("um")
        .join("windows.h")
        .is_file()
        && dir
            .join("Lib")
            .join(version)
            .join("um")
            .join("x64")
            .is_dir()
}

/// `10.0.28000.0` as numbers, so that `10.0.9` is older than `10.0.10`.
fn numeric(version: &str) -> Option<Vec<u32>> {
    version.split('.').map(|part| part.parse().ok()).collect()
}

/// What a compile needs on top of the user's environment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MsvcEnvironment {
    /// `INCLUDE`.
    pub include: Vec<PathBuf>,
    /// `LIB`.
    pub lib: Vec<PathBuf>,
    /// `LIBPATH`.
    pub libpath: Vec<PathBuf>,
    /// Put in front of `PATH`.
    pub path: Vec<PathBuf>,
}

/// The environment for `toolset` with `sdk`, for MSVC, clang and clang-cl
/// alike.
pub fn resolve(toolset: &MsvcToolset, sdk: &WindowsSdk) -> MsvcEnvironment {
    let msvc = &toolset.dir;
    let include = sdk.dir.join("Include").join(&sdk.version);
    let lib = sdk.dir.join("Lib").join(&sdk.version);
    MsvcEnvironment {
        include: [msvc.join("include")]
            .into_iter()
            .chain(
                ["ucrt", "um", "shared", "winrt", "cppwinrt"]
                    .iter()
                    .map(|part| include.join(part)),
            )
            .collect(),
        lib: vec![
            msvc.join("lib").join("x64"),
            lib.join("ucrt").join("x64"),
            lib.join("um").join("x64"),
        ],
        libpath: vec![msvc.join("lib").join("x64")],
        path: vec![
            msvc.join("bin").join("Hostx64").join("x64"),
            sdk.dir.join("bin").join(&sdk.version).join("x64"),
        ],
    }
}

impl MsvcEnvironment {
    /// `base` with `INCLUDE`, `LIB` and `LIBPATH` replaced and `PATH`
    /// prefixed, for running a compiler or linker.
    pub fn apply(&self, base: &Env) -> Env {
        let path = self
            .path
            .iter()
            .cloned()
            .chain(base.get("PATH").into_iter().flat_map(std::env::split_paths));
        [
            ("INCLUDE", join(self.include.iter().cloned())),
            ("LIB", join(self.lib.iter().cloned())),
            ("LIBPATH", join(self.libpath.iter().cloned())),
            ("PATH", join(path)),
        ]
        .into_iter()
        .fold(base.clone(), |env, (name, value)| env.with(name, value))
    }
}

/// Directories as one `PATH`-style value. On Windows joining fails only for
/// a `"` in a name, which Windows does not allow in file names.
fn join(dirs: impl Iterator<Item = PathBuf>) -> OsString {
    std::env::join_paths(dirs).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Version;
    use cuno_core::NameCase;

    struct Kits(Option<PathBuf>);
    impl Registry for Kits {
        fn local_machine_string(&self, key: &str, value: &str) -> Option<OsString> {
            assert_eq!((key, value), (KITS_KEY, KITS_VALUE));
            self.0.clone().map(Into::into)
        }
    }

    /// An SDK tree under `dir` with `versions`; `true` marks a complete one.
    fn sdk_tree(dir: &Path, versions: &[(&str, bool)]) {
        for (version, complete) in versions {
            let um = dir.join("Include").join(version).join("um");
            std::fs::create_dir_all(&um).unwrap();
            std::fs::create_dir_all(dir.join("Lib").join(version).join("um").join("x64")).unwrap();
            if *complete {
                std::fs::write(um.join("windows.h"), "").unwrap();
            }
        }
    }

    fn env(pairs: &[(&str, &Path)]) -> Env {
        Env::from_pairs(
            pairs
                .iter()
                .map(|(k, v)| ((*k).into(), v.as_os_str().into())),
            NameCase::Insensitive,
        )
    }

    /// Mutation: compare versions as strings, or skip the `windows.h` check;
    /// this test fails.
    #[test]
    fn the_newest_complete_version_is_taken() {
        let root = tempfile::tempdir().unwrap();
        let kits = root.path().join("Kits");
        sdk_tree(
            &kits,
            &[
                ("10.0.9.0", true),
                ("10.0.10.0", true),
                ("10.0.11.0", false),
            ],
        );
        let sdk = find_windows_sdk(&env(&[]), &Kits(Some(kits.clone()))).unwrap();
        assert_eq!(
            sdk,
            WindowsSdk {
                dir: kits,
                version: "10.0.10.0".into()
            }
        );
    }

    /// Mutation: read `%ProgramFiles(x86)%` from the process instead of
    /// `env`, or ignore it when the registry has no value; this test fails.
    /// The directory name is not ASCII, as on a machine with a Japanese user
    /// name: paths stay paths end to end.
    #[test]
    fn without_a_registry_value_program_files_is_used() {
        let root = tempfile::tempdir().unwrap();
        let pf = root.path().join("プログラム");
        let kits = pf.join("Windows Kits").join("10");
        sdk_tree(&kits, &[("10.0.1.0", true)]);
        let sdk = find_windows_sdk(&env(&[("ProgramFiles(x86)", &pf)]), &Kits(None)).unwrap();
        assert_eq!(sdk.dir, kits);
    }

    /// Mutation: accept a version without `um\windows.h`; this test fails.
    #[test]
    fn no_complete_version_is_an_error_naming_the_directory() {
        let root = tempfile::tempdir().unwrap();
        sdk_tree(root.path(), &[("10.0.1.0", false)]);
        let err = find_windows_sdk(&env(&[]), &Kits(Some(root.path().into()))).unwrap_err();
        assert!(
            matches!(&err, ToolchainError::NoWindowsSdk { looked_in } if looked_in == root.path())
        );
    }

    fn toolset(dir: &str) -> MsvcToolset {
        MsvcToolset {
            dir: dir.into(),
            cl_version: Version {
                major: 19,
                minor: 52,
                patch: 1,
            },
        }
    }

    /// Mutation: drop `um` from `INCLUDE`, `ucrt` from `LIB`, or prepend
    /// `PATH` after the user's instead of before; this test fails.
    #[test]
    fn the_environment_has_the_toolset_and_sdk_directories_in_order() {
        let sdk = WindowsSdk {
            dir: "K".into(),
            version: "10.0.1.0".into(),
        };
        let resolved = resolve(&toolset("T"), &sdk);
        let base = Env::from_pairs(
            [
                ("Path".into(), "U".into()),
                ("INCLUDE".into(), "stale".into()),
            ],
            NameCase::Insensitive,
        );
        let env = resolved.apply(&base);
        let list = |name: &str| -> Vec<PathBuf> {
            std::env::split_paths(env.get(name).unwrap()).collect()
        };
        let p = |s: &str| PathBuf::from(s.replace('/', std::path::MAIN_SEPARATOR_STR));
        assert_eq!(
            list("INCLUDE"),
            [
                p("T/include"),
                p("K/Include/10.0.1.0/ucrt"),
                p("K/Include/10.0.1.0/um"),
                p("K/Include/10.0.1.0/shared"),
                p("K/Include/10.0.1.0/winrt"),
                p("K/Include/10.0.1.0/cppwinrt"),
            ]
        );
        assert_eq!(
            list("LIB"),
            [
                p("T/lib/x64"),
                p("K/Lib/10.0.1.0/ucrt/x64"),
                p("K/Lib/10.0.1.0/um/x64")
            ]
        );
        assert_eq!(list("LIBPATH"), [p("T/lib/x64")]);
        assert_eq!(
            list("PATH"),
            [p("T/bin/Hostx64/x64"), p("K/bin/10.0.1.0/x64"), p("U")]
        );
    }
}
