//! Finding Visual Studio and Build Tools installations through vswhere, their
//! MSVC toolsets, and each toolset's cl.exe version.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use cuno_core::Env;

use crate::{Runner, ToolchainError, Version};

/// The component vswhere is asked for: the C++ x64 build tools.
const VC_TOOLS: &str = "Microsoft.VisualStudio.Component.VC.Tools.x86.x64";

/// One Visual Studio or Build Tools installation with the C++ x64 tools.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VsInstallation {
    /// The installation directory (vswhere's `installationPath`).
    pub path: PathBuf,
    /// Whether this is a preview. Detected, but never chosen by selection.
    pub prerelease: bool,
    /// The MSVC toolsets it contains, newest cl.exe first.
    pub toolsets: Vec<MsvcToolset>,
    /// The toolset directory name Visual Studio marks as its default, if it
    /// says.
    pub default_toolset: Option<String>,
}

/// One MSVC toolset: a `VC\Tools\MSVC\<version>` directory with a cl.exe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MsvcToolset {
    /// The toolset directory (`...\VC\Tools\MSVC\14.52.37025`).
    pub dir: PathBuf,
    /// The cl.exe version (`19.52.37029`), which is not the directory name.
    pub cl_version: Version,
}

impl MsvcToolset {
    /// The x64-hosted, x64-targeting cl.exe of this toolset.
    pub fn cl(&self) -> PathBuf {
        cl_in(&self.dir)
    }
}

fn cl_in(toolset_dir: &Path) -> PathBuf {
    toolset_dir
        .join("bin")
        .join("Hostx64")
        .join("x64")
        .join("cl.exe")
}

/// Every installation with the C++ x64 tools, previews included. An error
/// when vswhere is missing or fails, or when it finds none.
pub fn find_installations(
    env: &Env,
    run: &dyn Runner,
) -> Result<Vec<VsInstallation>, ToolchainError> {
    let vswhere = vswhere_path(env);
    if !vswhere.is_file() {
        return Err(ToolchainError::VswhereMissing { path: vswhere });
    }
    let args: [&OsStr; 10] = [
        "-all".as_ref(),
        "-products".as_ref(),
        "*".as_ref(),
        "-prerelease".as_ref(),
        "-requires".as_ref(),
        VC_TOOLS.as_ref(),
        "-utf8".as_ref(),
        "-format".as_ref(),
        "json".as_ref(),
        "-nologo".as_ref(),
    ];
    let output = run
        .run(&vswhere, &args)
        .map_err(|e| ToolchainError::VswhereFailed {
            detail: e.to_string(),
        })?;
    if !output.success {
        return Err(ToolchainError::VswhereFailed {
            detail: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    let mut installations = Vec::new();
    for (path, prerelease) in parse_vswhere(&output.stdout)? {
        let toolsets = toolsets_in(&path, run)?;
        if toolsets.is_empty() {
            continue;
        }
        let default_toolset = std::fs::read_to_string(
            path.join("VC/Auxiliary/Build/Microsoft.VCToolsVersion.default.txt"),
        )
        .ok()
        .map(|s| s.trim().to_owned());
        installations.push(VsInstallation {
            path,
            prerelease,
            toolsets,
            default_toolset,
        });
    }
    if installations.is_empty() {
        return Err(ToolchainError::NoVisualStudio);
    }
    Ok(installations)
}

/// Where every Visual Studio 2017+ installer puts vswhere.exe.
fn vswhere_path(env: &Env) -> PathBuf {
    let base = env
        .get("ProgramFiles(x86)")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)"));
    base.join("Microsoft Visual Studio")
        .join("Installer")
        .join("vswhere.exe")
}

/// The installations in vswhere's JSON, as (path, prerelease).
fn parse_vswhere(stdout: &[u8]) -> Result<Vec<(PathBuf, bool)>, ToolchainError> {
    let not_json = |detail: String| ToolchainError::VswhereFailed { detail };
    let value: serde_json::Value = serde_json::from_slice(stdout)
        .map_err(|e| not_json(format!("its output is not JSON ({e})")))?;
    let list = value
        .as_array()
        .ok_or_else(|| not_json("its output is not a JSON list".to_owned()))?;
    Ok(list
        .iter()
        .filter_map(|item| {
            let path = item.get("installationPath")?.as_str()?;
            let prerelease = item
                .get("isPrerelease")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            Some((PathBuf::from(path), prerelease))
        })
        .collect())
}

/// The toolsets of one installation, newest cl.exe first. A directory without
/// cl.exe is not a toolset and is skipped.
fn toolsets_in(installation: &Path, run: &dyn Runner) -> Result<Vec<MsvcToolset>, ToolchainError> {
    let Ok(entries) = std::fs::read_dir(installation.join("VC/Tools/MSVC")) else {
        return Ok(Vec::new());
    };
    let mut toolsets = Vec::new();
    for entry in entries.flatten() {
        let dir = entry.path();
        let cl = cl_in(&dir);
        if !cl.is_file() {
            continue;
        }
        toolsets.push(MsvcToolset {
            cl_version: cl_version(&cl, run)?,
            dir,
        });
    }
    toolsets.sort_by_key(|t| std::cmp::Reverse(t.cl_version));
    Ok(toolsets)
}

/// cl.exe's version, from the banner it prints when run with no arguments.
/// The banner is in the console code page (CP932 on Japanese Windows), so it
/// is decoded lossily: only the digits are read, and they are ASCII in every
/// code page.
fn cl_version(cl: &Path, run: &dyn Runner) -> Result<Version, ToolchainError> {
    let unknown = || ToolchainError::ClVersionUnknown {
        path: cl.to_owned(),
    };
    let output = run.run(cl, &[]).map_err(|_| unknown())?;
    let mut text = String::from_utf8_lossy(&output.stderr).into_owned();
    text.push('\n');
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    Version::find_in(&text).ok_or_else(unknown)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Output;
    use cuno_core::NameCase;
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// Answers vswhere and cl.exe from canned output, and records what ran.
    #[derive(Default)]
    struct Fake {
        answers: HashMap<String, Output>,
        ran: RefCell<Vec<String>>,
    }

    impl Fake {
        fn answer(mut self, program_suffix: &str, stdout: &[u8], stderr: &[u8]) -> Fake {
            self.answers.insert(
                program_suffix.to_owned(),
                Output {
                    success: true,
                    stdout: stdout.to_vec(),
                    stderr: stderr.to_vec(),
                },
            );
            self
        }
    }

    impl Runner for Fake {
        fn run(&self, program: &Path, args: &[&OsStr]) -> std::io::Result<Output> {
            let program = program.to_string_lossy().replace('\\', "/");
            let line = std::iter::once(program.clone())
                .chain(args.iter().map(|a| a.to_string_lossy().into_owned()))
                .collect::<Vec<_>>()
                .join(" ");
            self.ran.borrow_mut().push(line);
            self.answers
                .iter()
                .find(|(suffix, _)| program.ends_with(suffix.as_str()))
                .map(|(_, out)| out.clone())
                .ok_or_else(|| std::io::Error::other("not answered"))
        }
    }

    /// A fake `Program Files (x86)` with vswhere, and a fake installation per
    /// `(name, toolset dirs)` with a cl.exe in each toolset.
    fn tree(installs: &[(&str, &[&str])]) -> (tempfile::TempDir, Env) {
        let root = tempfile::tempdir().unwrap();
        let installer = root.path().join("x86/Microsoft Visual Studio/Installer");
        std::fs::create_dir_all(&installer).unwrap();
        std::fs::write(installer.join("vswhere.exe"), "").unwrap();
        for (name, toolsets) in installs {
            for toolset in *toolsets {
                let bin = root
                    .path()
                    .join(name)
                    .join("VC/Tools/MSVC")
                    .join(toolset)
                    .join("bin/Hostx64/x64");
                std::fs::create_dir_all(&bin).unwrap();
                std::fs::write(bin.join("cl.exe"), "").unwrap();
            }
        }
        let env = Env::from_pairs(
            [(
                "ProgramFiles(x86)".into(),
                root.path().join("x86").into_os_string(),
            )],
            NameCase::Insensitive,
        );
        (root, env)
    }

    fn json(root: &Path, items: &[(&str, bool)]) -> Vec<u8> {
        let items: Vec<_> = items
            .iter()
            .map(|(name, pre)| {
                serde_json::json!({
                    "installationPath": root.join(name),
                    "isPrerelease": pre,
                })
            })
            .collect();
        serde_json::to_vec(&items).unwrap()
    }

    /// Mutation: drop `-prerelease` or `-utf8` from the vswhere arguments, or
    /// ignore `isPrerelease`; this test fails.
    #[test]
    fn finds_every_installation_and_marks_previews() {
        let (root, env) = tree(&[("Stable", &["14.44.35207"]), ("Preview", &["14.52.37025"])]);
        let fake = Fake::default()
            .answer(
                "vswhere.exe",
                &json(root.path(), &[("Stable", false), ("Preview", true)]),
                b"",
            )
            .answer(
                "14.44.35207/bin/Hostx64/x64/cl.exe",
                b"",
                b"Microsoft (R) C/C++ Optimizing Compiler Version 19.44.35207 for x64",
            )
            .answer(
                "14.52.37025/bin/Hostx64/x64/cl.exe",
                b"",
                b"Microsoft (R) C/C++ Optimizing Compiler Version 19.52.37029 for x64",
            );
        let found = find_installations(&env, &fake).unwrap();
        assert_eq!(found.len(), 2);
        assert!(!found[0].prerelease && found[1].prerelease, "{found:?}");
        assert_eq!(found[1].toolsets[0].cl_version.to_string(), "19.52.37029");
        let vswhere_call = &fake.ran.borrow()[0];
        for arg in ["-all", "-prerelease", "-utf8", "-format json", VC_TOOLS] {
            assert!(vswhere_call.contains(arg), "{vswhere_call}");
        }
    }

    /// Mutation: decode cl's banner strictly as UTF-8; this test fails on the
    /// CP932 banner of a Japanese Visual Studio.
    #[test]
    fn reads_the_version_from_a_japanese_banner() {
        // "Microsoft(R) C/C++ 最適化コンパイラ バージョン 19.40.33811 (x64)" in CP932.
        let banner: &[u8] = &[
            77, 105, 99, 114, 111, 115, 111, 102, 116, 40, 82, 41, 32, 67, 47, 67, 43, 43, 32, 141,
            197, 147, 75, 137, 187, 131, 82, 131, 147, 131, 112, 131, 67, 131, 137, 32, 131, 111,
            129, 91, 131, 87, 131, 135, 131, 147, 32, 49, 57, 46, 52, 48, 46, 51, 51, 56, 49, 49,
            32, 40, 120, 54, 52, 41,
        ];
        let (root, env) = tree(&[("VS", &["14.40.33807"])]);
        let fake = Fake::default()
            .answer("vswhere.exe", &json(root.path(), &[("VS", false)]), b"")
            .answer("cl.exe", b"", banner);
        let found = find_installations(&env, &fake).unwrap();
        assert_eq!(found[0].toolsets[0].cl_version.to_string(), "19.40.33811");
    }

    /// Mutation: sort toolsets oldest first, or read the version from the
    /// directory name; this test fails.
    #[test]
    fn toolsets_are_newest_first_by_cl_version() {
        let (root, env) = tree(&[("VS", &["14.44.35207", "14.52.37025"])]);
        let fake = Fake::default()
            .answer("vswhere.exe", &json(root.path(), &[("VS", false)]), b"")
            .answer(
                "14.44.35207/bin/Hostx64/x64/cl.exe",
                b"",
                b"Version 19.44.35207 for x64",
            )
            .answer(
                "14.52.37025/bin/Hostx64/x64/cl.exe",
                b"",
                b"Version 19.52.37029 for x64",
            );
        let toolsets = &find_installations(&env, &fake).unwrap()[0].toolsets;
        let versions: Vec<_> = toolsets.iter().map(|t| t.cl_version.to_string()).collect();
        assert_eq!(versions, ["19.52.37029", "19.44.35207"]);
    }

    /// Mutation: return an empty list instead of `NoVisualStudio`; this test
    /// fails.
    #[test]
    fn no_installation_is_an_error() {
        let (root, env) = tree(&[]);
        let fake = Fake::default().answer("vswhere.exe", &json(root.path(), &[]), b"");
        assert!(matches!(
            find_installations(&env, &fake),
            Err(ToolchainError::NoVisualStudio)
        ));
    }

    /// Mutation: skip the `is_file` check on vswhere.exe (running it would
    /// fail with a less useful error); this test fails.
    #[test]
    fn a_missing_vswhere_is_named() {
        let env = Env::from_pairs(
            [("ProgramFiles(x86)".into(), "Z:/nowhere".into())],
            NameCase::Insensitive,
        );
        let err = find_installations(&env, &Fake::default()).unwrap_err();
        assert!(
            matches!(err, ToolchainError::VswhereMissing { .. }),
            "{err}"
        );
    }

    /// Mutation: accept vswhere output that is not JSON as no installations;
    /// this test fails.
    #[test]
    fn output_that_is_not_json_is_a_vswhere_failure() {
        let (_root, env) = tree(&[]);
        let fake = Fake::default().answer("vswhere.exe", b"Visual Studio Locator", b"");
        assert!(matches!(
            find_installations(&env, &fake),
            Err(ToolchainError::VswhereFailed { .. })
        ));
    }

    /// Mutation: drop the default-toolset file read; this test fails.
    #[test]
    fn the_default_toolset_is_read_from_visual_studio() {
        let (root, env) = tree(&[("VS", &["14.52.37025"])]);
        let build = root.path().join("VS/VC/Auxiliary/Build");
        std::fs::create_dir_all(&build).unwrap();
        std::fs::write(
            build.join("Microsoft.VCToolsVersion.default.txt"),
            "14.52.37025\r\n",
        )
        .unwrap();
        let fake = Fake::default()
            .answer("vswhere.exe", &json(root.path(), &[("VS", false)]), b"")
            .answer("cl.exe", b"", b"Version 19.52.37029");
        let found = find_installations(&env, &fake).unwrap();
        assert_eq!(found[0].default_toolset.as_deref(), Some("14.52.37025"));
    }
}
