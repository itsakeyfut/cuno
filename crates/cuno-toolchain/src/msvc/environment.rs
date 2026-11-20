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

/// The Windows SDK to build with: the newest complete version in the first
/// place that has one, of the registered SDK directory and
/// `%ProgramFiles(x86)%\Windows Kits\10`.
pub fn find_windows_sdk(env: &Env, registry: &dyn Registry) -> Result<WindowsSdk, ToolchainError> {
    let roots = sdk_roots(env, registry);
    for dir in &roots {
        if let Some(version) = newest_complete_version(dir) {
            return Ok(WindowsSdk {
                dir: dir.clone(),
                version,
            });
        }
    }
    let looked_in = if roots.is_empty() {
        vec![
            Path::new("%ProgramFiles(x86)%")
                .join("Windows Kits")
                .join("10"),
        ]
    } else {
        roots
    };
    Err(ToolchainError::NoWindowsSdk { looked_in })
}

/// Where the SDK may be, in order: the registered directory, then the one
/// under `%ProgramFiles(x86)%`, taking only absolute ones. An empty or
/// relative directory would be read from cuno's working directory (often the
/// project).
fn sdk_roots(env: &Env, registry: &dyn Registry) -> Vec<PathBuf> {
    let registered = registry
        .local_machine_string(KITS_KEY, KITS_VALUE)
        .map(PathBuf::from);
    let program_files = env
        .get("ProgramFiles(x86)")
        .map(|pf| Path::new(pf).join("Windows Kits").join("10"));
    let mut roots: Vec<PathBuf> = Vec::new();
    for dir in [registered, program_files].into_iter().flatten() {
        if dir.is_absolute() && !roots.contains(&dir) {
            roots.push(dir);
        }
    }
    roots
}

/// The newest version under `dir` that is complete, compared as numbers.
fn newest_complete_version(dir: &Path) -> Option<String> {
    let mut versions: Vec<(Vec<u32>, String)> = std::fs::read_dir(dir.join("Include"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|version| complete(dir, version))
        .filter_map(|version| Some((numeric(&version)?, version)))
        .collect();
    versions.sort();
    versions.pop().map(|(_, version)| version)
}

/// Whether `version` has the x64 headers and libraries of both the Windows
/// API (`um`) and the C runtime (`ucrt`): an SDK can leave part of a version
/// behind when only part of it is installed or removed.
fn complete(dir: &Path, version: &str) -> bool {
    let include = dir.join("Include").join(version);
    let lib = dir.join("Lib").join(version);
    include.join("um").join("windows.h").is_file()
        && include.join("ucrt").join("stdio.h").is_file()
        && lib.join("um").join("x64").is_dir()
        && lib.join("ucrt").join("x64").is_dir()
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
    // `<VS>\VC\Auxiliary\VS\include` (`CppCoreCheck\warnings.h`), next to the
    // toolset's `VC\Tools\MSVC\<version>`, when this Visual Studio has it.
    let auxiliary = msvc
        .ancestors()
        .nth(3)
        .map(|vc| vc.join("Auxiliary").join("VS").join("include"))
        .filter(|dir| dir.is_dir());
    MsvcEnvironment {
        include: [msvc.join("include")]
            .into_iter()
            .chain(auxiliary)
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
    /// `base` with `INCLUDE`, `LIB` and `LIBPATH` replaced, `PATH` prefixed
    /// and the Developer Command Prompt's variables removed, for running a
    /// compiler or linker.
    pub fn apply(&self, base: &Env) -> Env {
        let base = &base.without(DEVELOPER_PROMPT_VARIABLES);
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

/// What a Developer Command Prompt sets that clang and clang-cl read to find
/// a toolset and a linker: left in place, a prompt for another Visual Studio
/// would win over the toolset cuno chose. See `docs/en/specs/toolchain.md`,
/// "How it is resolved".
const DEVELOPER_PROMPT_VARIABLES: &[&str] = &[
    "VSINSTALLDIR",
    "VCINSTALLDIR",
    "VCToolsInstallDir",
    "EXTERNAL_INCLUDE",
    "WindowsSdkDir",
    "WindowsSDKVersion",
    "UniversalCRTSdkDir",
    "UCRTVersion",
];

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

    /// What a version directory holds.
    #[derive(Clone, Copy)]
    enum Part {
        Complete,
        NoWindowsH,
        NoUcrtHeader,
        NoUmLib,
        NoUcrtLib,
    }

    /// An SDK tree under `dir` with `versions`.
    fn sdk_tree(dir: &Path, versions: &[(&str, Part)]) {
        for (version, part) in versions {
            let include = dir.join("Include").join(version);
            let lib = dir.join("Lib").join(version);
            for sub in ["um", "ucrt"] {
                std::fs::create_dir_all(include.join(sub)).unwrap();
            }
            if !matches!(part, Part::NoWindowsH) {
                std::fs::write(include.join("um").join("windows.h"), "").unwrap();
            }
            if !matches!(part, Part::NoUcrtHeader) {
                std::fs::write(include.join("ucrt").join("stdio.h"), "").unwrap();
            }
            if !matches!(part, Part::NoUmLib) {
                std::fs::create_dir_all(lib.join("um").join("x64")).unwrap();
            }
            if !matches!(part, Part::NoUcrtLib) {
                std::fs::create_dir_all(lib.join("ucrt").join("x64")).unwrap();
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

    /// Mutation: compare versions as strings, take a name that is not a
    /// number as a version, or drop any of the four checks of a complete
    /// version; this test fails.
    #[test]
    fn the_newest_complete_version_is_taken() {
        let root = tempfile::tempdir().unwrap();
        let kits = root.path().join("Kits");
        sdk_tree(
            &kits,
            &[
                ("10.0.9.0", Part::Complete),
                ("10.0.10.0", Part::Complete),
                ("10.0.11.0", Part::NoWindowsH),
                ("10.0.12.0", Part::NoUcrtHeader),
                ("10.0.13.0", Part::NoUmLib),
                ("10.0.14.0", Part::NoUcrtLib),
                ("wdf", Part::Complete),
                ("10.0.x.0", Part::Complete),
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
    /// `env`, look there before the registry, or stop at a registered
    /// directory with no SDK left in it; this test fails. The directory name
    /// is not ASCII, as on a machine with a Japanese user name: paths stay
    /// paths end to end.
    #[test]
    fn the_registry_comes_first_and_program_files_second() {
        let root = tempfile::tempdir().unwrap();
        let pf = root.path().join("プログラム");
        let installed = pf.join("Windows Kits").join("10");
        sdk_tree(&installed, &[("10.0.1.0", Part::Complete)]);
        let registered = root.path().join("Registered");
        let stale = root.path().join("Stale");
        std::fs::create_dir_all(&stale).unwrap();
        sdk_tree(&registered, &[("10.0.1.0", Part::Complete)]);
        let env = env(&[("ProgramFiles(x86)", &pf)]);
        let found = |registry| find_windows_sdk(&env, &registry).unwrap().dir;
        assert_eq!(found(Kits(Some(registered.clone()))), registered);
        assert_eq!(found(Kits(Some(stale))), installed);
        assert_eq!(found(Kits(None)), installed);
    }

    /// Mutation: report only one of the places looked in, take a directory
    /// whose name is not a number as a version, or name no place
    /// when there is none to look in; this test fails.
    #[test]
    fn no_complete_version_is_an_error_naming_where_it_looked() {
        let root = tempfile::tempdir().unwrap();
        let pf = root.path().join("pf");
        let installed = pf.join("Windows Kits").join("10");
        sdk_tree(
            root.path(),
            &[("10.0.1.0", Part::NoWindowsH), ("wdf", Part::Complete)],
        );
        let err = find_windows_sdk(
            &env(&[("ProgramFiles(x86)", &pf)]),
            &Kits(Some(root.path().into())),
        )
        .unwrap_err();
        assert!(
            matches!(&err, ToolchainError::NoWindowsSdk { looked_in } if *looked_in == [root.path().to_owned(), installed]),
            "{err:?}"
        );
        let err = find_windows_sdk(&env(&[]), &Kits(None)).unwrap_err();
        assert!(err.to_string().contains("%ProgramFiles(x86)%"), "{err}");
    }

    /// Mutation: take an empty or relative directory from the registry or
    /// `%ProgramFiles(x86)%` instead of passing over it; this test fails.
    #[test]
    fn only_absolute_sdk_directories_are_searched() {
        let pf = std::env::temp_dir();
        let absolute = env(&[("ProgramFiles(x86)", &pf)]);
        let relative = env(&[("ProgramFiles(x86)", Path::new("pf"))]);
        let expected = vec![pf.join("Windows Kits").join("10")];
        assert_eq!(sdk_roots(&absolute, &Kits(Some("".into()))), expected);
        assert_eq!(sdk_roots(&absolute, &Kits(Some("Kits".into()))), expected);
        assert!(sdk_roots(&relative, &Kits(None)).is_empty());
        assert!(sdk_roots(&env(&[]), &Kits(None)).is_empty());
    }

    fn toolset(dir: impl Into<PathBuf>) -> MsvcToolset {
        MsvcToolset {
            dir: dir.into(),
            cl_version: Version {
                major: 19,
                minor: 52,
                patch: 1,
            },
        }
    }

    /// Mutation: drop `um` from `INCLUDE`, `ucrt` from `LIB`, prepend `PATH`
    /// after the user's instead of before, or keep a Developer Command Prompt
    /// variable; this test fails.
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
            ]
            .into_iter()
            .chain(
                DEVELOPER_PROMPT_VARIABLES
                    .iter()
                    .map(|name| (name.to_ascii_lowercase().into(), "stale".into())),
            ),
            NameCase::Insensitive,
        );
        let env = resolved.apply(&base);
        for name in DEVELOPER_PROMPT_VARIABLES {
            assert_eq!(env.get(name), None, "{name}");
        }
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

    /// Mutation: leave out `VC\Auxiliary\VS\include`, or add it when this
    /// Visual Studio does not have it; this test fails.
    #[test]
    fn the_auxiliary_include_directory_is_used_when_present() {
        let root = tempfile::tempdir().unwrap();
        let vc = root.path().join("VS").join("VC");
        let msvc = vc.join("Tools").join("MSVC").join("14.52.1");
        let sdk = WindowsSdk {
            dir: "K".into(),
            version: "10.0.1.0".into(),
        };
        let auxiliary = vc.join("Auxiliary").join("VS").join("include");
        assert!(
            !resolve(&toolset(msvc.clone()), &sdk)
                .include
                .contains(&auxiliary)
        );
        std::fs::create_dir_all(&auxiliary).unwrap();
        assert_eq!(resolve(&toolset(msvc.clone()), &sdk).include[1], auxiliary);
    }
}
