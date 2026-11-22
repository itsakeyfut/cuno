//! Finding Visual Studio and Build Tools installations through vswhere, their
//! MSVC toolsets, and each toolset's cl.exe version. See
//! `docs/en/specs/toolchain.md`, "Visual Studio / Build Tools".

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use cuno_core::Env;

use crate::{Runner, ToolchainError, Version};

/// The component vswhere is asked for: the C++ x64 build tools.
const VC_TOOLS: &str = "Microsoft.VisualStudio.Component.VC.Tools.x86.x64";

/// The arguments vswhere is run with, exactly as the spec gives them.
const VSWHERE_ARGS: [&str; 9] = [
    "-all",
    "-products",
    "*",
    "-prerelease",
    "-requires",
    VC_TOOLS,
    "-utf8",
    "-format",
    "json",
];

/// Variables cl.exe reads options from (`CL` before the command line, `_CL_`
/// after it). A user's `CL=/nologo` would hide the banner the version is read
/// from, so they are removed when cl.exe is asked its version.
const CL_OPTION_VARIABLES: [&str; 2] = ["CL", "_CL_"];

/// One Visual Studio or Build Tools installation with the C++ x64 tools.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VsInstallation {
    /// The installation directory (vswhere's `installationPath`).
    pub path: PathBuf,
    /// Whether this is a preview. Detected, but never chosen by selection.
    pub prerelease: bool,
    /// Whether vswhere reports it complete and launchable. An incomplete one
    /// (an error in setup, or a reboot pending) is detected, but never chosen
    /// by selection.
    pub complete: bool,
    /// The usable MSVC toolsets it contains, newest cl.exe first.
    pub toolsets: Vec<MsvcToolset>,
    /// Toolset directories whose cl.exe could not tell its version; left out
    /// of `toolsets` so that one broken toolset does not hide the others.
    pub unusable_toolsets: Vec<PathBuf>,
    /// The toolset directory name Visual Studio marks as its default, if it
    /// says. Not checked against `toolsets`: a stale file can name one that
    /// is not there.
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

    /// The Visual Studio (or Build Tools) installation this toolset belongs
    /// to: `dir` is `<installation>\VC\Tools\MSVC\<version>`.
    pub fn install_dir(&self) -> &Path {
        self.dir.ancestors().nth(4).unwrap_or(&self.dir)
    }
}

fn cl_in(toolset_dir: &Path) -> PathBuf {
    toolset_dir
        .join("bin")
        .join("Hostx64")
        .join("x64")
        .join("cl.exe")
}

/// Every installation with the C++ x64 tools, previews and incomplete ones
/// included. An error when vswhere is missing or fails, or when no
/// installation has a usable toolset.
pub fn find_installations(
    env: &Env,
    run: &dyn Runner,
) -> Result<Vec<VsInstallation>, ToolchainError> {
    let vswhere = vswhere_path(env);
    if !vswhere.is_file() {
        return Err(ToolchainError::VswhereMissing { path: vswhere });
    }
    let args: Vec<&OsStr> = VSWHERE_ARGS.iter().map(OsStr::new).collect();
    let output = run
        .run(&vswhere, &args, env)
        .map_err(|e| ToolchainError::VswhereFailed {
            detail: e.to_string(),
        })?;
    if !output.success {
        let mut detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        if detail.is_empty() {
            detail = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        }
        return Err(ToolchainError::VswhereFailed { detail });
    }

    let cl_env = env.without(&CL_OPTION_VARIABLES);
    let mut installations = Vec::new();
    let mut first_unusable = None;
    for found in parse_vswhere(&output.stdout)? {
        let (toolsets, unusable_toolsets) = toolsets_in(&found.path, run, &cl_env);
        if first_unusable.is_none() {
            first_unusable = unusable_toolsets.first().cloned();
        }
        if toolsets.is_empty() {
            continue;
        }
        let default_toolset = std::fs::read_to_string(
            found
                .path
                .join("VC")
                .join("Auxiliary")
                .join("Build")
                .join("Microsoft.VCToolsVersion.default.txt"),
        )
        .ok()
        .map(|s| s.trim().to_owned());
        installations.push(VsInstallation {
            path: found.path,
            prerelease: found.prerelease,
            complete: found.complete,
            toolsets,
            unusable_toolsets,
            default_toolset,
        });
    }
    if installations.is_empty() {
        // Toolsets were there but none could tell its version: say which,
        // rather than that Visual Studio is missing.
        return Err(match first_unusable {
            Some(dir) => ToolchainError::ClVersionUnknown { path: cl_in(&dir) },
            None => ToolchainError::NoVisualStudio,
        });
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

/// One entry of vswhere's JSON.
struct Found {
    path: PathBuf,
    prerelease: bool,
    complete: bool,
}

/// The installations in vswhere's JSON. An entry without `installationPath`
/// is an error rather than skipped: it means vswhere's output is not what
/// this code understands.
fn parse_vswhere(stdout: &[u8]) -> Result<Vec<Found>, ToolchainError> {
    let not_understood = |detail: String| ToolchainError::VswhereFailed { detail };
    let value: serde_json::Value = serde_json::from_slice(stdout)
        .map_err(|e| not_understood(format!("its output is not JSON ({e})")))?;
    let list = value
        .as_array()
        .ok_or_else(|| not_understood("its output is not a JSON list".to_owned()))?;
    list.iter()
        .map(|item| {
            let path = item
                .get("installationPath")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| not_understood("an entry has no installationPath".to_owned()))?;
            let flag = |name: &str| item.get(name).and_then(serde_json::Value::as_bool);
            Ok(Found {
                path: PathBuf::from(path),
                // A missing flag counts against the installation: a preview
                // or an incomplete one is never chosen, so guessing "stable"
                // would be the unsafe default.
                prerelease: flag("isPrerelease").unwrap_or(true),
                complete: flag("isComplete").unwrap_or(false)
                    && flag("isLaunchable").unwrap_or(false),
            })
        })
        .collect()
}

/// The toolsets of one installation, newest cl.exe first (ties by directory
/// name, so the order does not depend on the file system), and the toolset
/// directories whose cl.exe could not tell its version. A directory without
/// cl.exe is not a toolset and is not listed at all.
fn toolsets_in(
    installation: &Path,
    run: &dyn Runner,
    cl_env: &Env,
) -> (Vec<MsvcToolset>, Vec<PathBuf>) {
    let msvc = installation.join("VC").join("Tools").join("MSVC");
    let Ok(entries) = std::fs::read_dir(msvc) else {
        return (Vec::new(), Vec::new());
    };
    let mut toolsets = Vec::new();
    let mut unusable = Vec::new();
    for entry in entries.flatten() {
        let dir = entry.path();
        let cl = cl_in(&dir);
        if !cl.is_file() {
            continue;
        }
        match cl_version(&cl, run, cl_env) {
            Some(cl_version) => toolsets.push(MsvcToolset { dir, cl_version }),
            None => unusable.push(dir),
        }
    }
    toolsets.sort_by(|a, b| {
        b.cl_version
            .cmp(&a.cl_version)
            .then_with(|| b.dir.cmp(&a.dir))
    });
    unusable.sort();
    (toolsets, unusable)
}

/// cl.exe's version, from the banner it prints on stderr when run with no
/// arguments. The output is in the console code page (CP932 on Japanese
/// Windows), so it is decoded lossily: only the digits are read, and they are
/// ASCII in every code page.
fn cl_version(cl: &Path, run: &dyn Runner, cl_env: &Env) -> Option<Version> {
    let output = run.run(cl, &[], cl_env).ok()?;
    let mut text = String::from_utf8_lossy(&output.stderr).into_owned();
    text.push('\n');
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    Version::find_in(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mutation: take another ancestor of the toolset directory, or name a
    /// directory too short to be one as nothing; this test fails.
    #[test]
    fn a_toolset_names_its_installation() {
        let toolset = MsvcToolset {
            dir: Path::new("C:/VS/2022/VC/Tools/MSVC/14.44.35207").to_owned(),
            cl_version: crate::Version {
                major: 19,
                minor: 44,
                patch: 35207,
            },
        };
        assert_eq!(toolset.install_dir(), Path::new("C:/VS/2022"));
        // Too short to be a toolset directory: named as itself rather than
        // as nothing.
        let short = MsvcToolset {
            dir: Path::new("C:/x").to_owned(),
            ..toolset
        };
        assert_eq!(short.install_dir(), Path::new("C:/x"));
    }
    use crate::Output;
    use cuno_core::NameCase;
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// One program run the fake saw.
    struct Ran {
        line: String,
        env: Vec<String>,
    }

    /// Answers vswhere and cl.exe from canned output, and records what ran.
    #[derive(Default)]
    struct Fake {
        answers: HashMap<String, Output>,
        ran: RefCell<Vec<Ran>>,
    }

    impl Fake {
        fn answer(self, program_suffix: &str, stdout: &[u8], stderr: &[u8]) -> Fake {
            self.answer_with(program_suffix, true, stdout, stderr)
        }

        fn answer_with(
            mut self,
            program_suffix: &str,
            success: bool,
            stdout: &[u8],
            stderr: &[u8],
        ) -> Fake {
            self.answers.insert(
                program_suffix.to_owned(),
                Output {
                    success,
                    stdout: stdout.to_vec(),
                    stderr: stderr.to_vec(),
                },
            );
            self
        }
    }

    impl Runner for Fake {
        fn run(&self, program: &Path, args: &[&OsStr], env: &Env) -> std::io::Result<Output> {
            let program = program.to_string_lossy().replace('\\', "/");
            let line = std::iter::once(program.clone())
                .chain(args.iter().map(|a| a.to_string_lossy().into_owned()))
                .collect::<Vec<_>>()
                .join(" ");
            let env = env
                .vars()
                .map(|(n, _)| n.to_string_lossy().into_owned())
                .collect();
            self.ran.borrow_mut().push(Ran { line, env });
            self.answers
                .iter()
                .find(|(suffix, _)| program.ends_with(suffix.as_str()))
                .map(|(_, out)| out.clone())
                .ok_or_else(|| std::io::Error::other("not answered"))
        }
    }

    /// A fake `Program Files (x86)` with vswhere, and a fake installation per
    /// `(name, toolset dirs)` with a cl.exe in each toolset. `extra` adds
    /// variables to the environment detection is given.
    fn tree(installs: &[(&str, &[&str])], extra: &[(&str, &str)]) -> (tempfile::TempDir, Env) {
        let root = tempfile::tempdir().unwrap();
        let installer = root.path().join("x86/Microsoft Visual Studio/Installer");
        std::fs::create_dir_all(&installer).unwrap();
        std::fs::write(installer.join("vswhere.exe"), "").unwrap();
        for (name, toolsets) in installs {
            std::fs::create_dir_all(root.path().join(name).join("VC/Tools/MSVC")).unwrap();
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
        let mut pairs = vec![(
            "ProgramFiles(x86)".into(),
            root.path().join("x86").into_os_string(),
        )];
        pairs.extend(extra.iter().map(|(n, v)| ((*n).into(), (*v).into())));
        (root, Env::from_pairs(pairs, NameCase::Insensitive))
    }

    /// An entry of vswhere's JSON.
    fn entry(root: &Path, name: &str, prerelease: bool, complete: bool) -> serde_json::Value {
        serde_json::json!({
            "installationPath": root.join(name),
            "isPrerelease": prerelease,
            "isComplete": complete,
            "isLaunchable": complete,
        })
    }

    fn json(items: &[serde_json::Value]) -> Vec<u8> {
        serde_json::to_vec(items).unwrap()
    }

    const BANNER_52: &[u8] = b"Microsoft (R) C/C++ Optimizing Compiler Version 19.52.37029 for x64";
    const BANNER_44: &[u8] = b"Microsoft (R) C/C++ Optimizing Compiler Version 19.44.35207 for x64";

    /// Mutation: change, drop or reorder any vswhere argument (`-products *`
    /// finds Build Tools, `-prerelease` previews, `-utf8` makes the output
    /// independent of the code page); this test fails.
    #[test]
    fn vswhere_is_run_with_exactly_the_specified_arguments() {
        let (root, env) = tree(&[("VS", &["14.52.37025"])], &[]);
        let fake = Fake::default()
            .answer(
                "vswhere.exe",
                &json(&[entry(root.path(), "VS", false, true)]),
                b"",
            )
            .answer("cl.exe", b"", BANNER_52);
        find_installations(&env, &fake).unwrap();
        let vswhere = &fake.ran.borrow()[0].line;
        assert!(
            vswhere.ends_with(
                "/vswhere.exe -all -products * -prerelease -requires \
                 Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -utf8 -format json"
            ),
            "{vswhere}"
        );
    }

    /// Mutation: ignore `isPrerelease`, `isComplete` or `isLaunchable`; this
    /// test fails.
    #[test]
    fn previews_and_incomplete_installations_are_marked() {
        let (root, env) = tree(
            &[
                ("Stable", &["14.44.35207"]),
                ("Preview", &["14.52.37025"]),
                ("Broken", &["14.44.35207"]),
            ],
            &[],
        );
        let fake = Fake::default()
            .answer(
                "vswhere.exe",
                &json(&[
                    entry(root.path(), "Stable", false, true),
                    entry(root.path(), "Preview", true, true),
                    entry(root.path(), "Broken", false, false),
                ]),
                b"",
            )
            .answer("14.44.35207/bin/Hostx64/x64/cl.exe", b"", BANNER_44)
            .answer("14.52.37025/bin/Hostx64/x64/cl.exe", b"", BANNER_52);
        let found = find_installations(&env, &fake).unwrap();
        let flags: Vec<_> = found.iter().map(|i| (i.prerelease, i.complete)).collect();
        assert_eq!(flags, [(false, true), (true, true), (false, false)]);
        assert_eq!(found[1].path, root.path().join("Preview"));
    }

    /// Mutation: default a missing `isPrerelease` to `false` or a missing
    /// `isComplete` to `true`; this test fails.
    #[test]
    fn missing_flags_count_against_the_installation() {
        let (root, env) = tree(&[("VS", &["14.52.37025"])], &[]);
        let item = serde_json::json!({ "installationPath": root.path().join("VS") });
        let fake = Fake::default()
            .answer("vswhere.exe", &json(&[item]), b"")
            .answer("cl.exe", b"", BANNER_52);
        let found = find_installations(&env, &fake).unwrap();
        assert!(found[0].prerelease && !found[0].complete, "{found:?}");
    }

    /// Mutation: pass the environment through unchanged to cl.exe (a user's
    /// `CL=/nologo` then hides the banner), or drop the variables vswhere is
    /// given; this test fails.
    #[test]
    fn cl_runs_without_the_cl_option_variables() {
        let (root, env) = tree(
            &[("VS", &["14.52.37025"])],
            &[("CL", "/nologo"), ("_CL_", "/W4"), ("KEEP", "1")],
        );
        let fake = Fake::default()
            .answer(
                "vswhere.exe",
                &json(&[entry(root.path(), "VS", false, true)]),
                b"",
            )
            .answer("cl.exe", b"", BANNER_52);
        find_installations(&env, &fake).unwrap();
        let ran = fake.ran.borrow();
        let cl = ran.iter().find(|r| r.line.ends_with("cl.exe")).unwrap();
        assert!(
            !cl.env.iter().any(|n| n == "CL" || n == "_CL_"),
            "{:?}",
            cl.env
        );
        assert!(cl.env.iter().any(|n| n == "KEEP"), "{:?}", cl.env);
        assert!(
            ran[0].env.iter().any(|n| n == "CL"),
            "vswhere gets the environment as given"
        );
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
        let (root, env) = tree(&[("VS", &["14.40.33807"])], &[]);
        let fake = Fake::default()
            .answer(
                "vswhere.exe",
                &json(&[entry(root.path(), "VS", false, true)]),
                b"",
            )
            .answer("cl.exe", b"", banner);
        let found = find_installations(&env, &fake).unwrap();
        assert_eq!(found[0].toolsets[0].cl_version.to_string(), "19.40.33811");
    }

    /// Mutation: sort toolsets oldest first, drop the tiebreak on the
    /// directory name, or read the version from the directory name; this test
    /// fails.
    #[test]
    fn toolsets_are_newest_first_by_cl_version_then_name() {
        let (root, env) = tree(
            &[("VS", &["14.44.35207", "14.52.37025", "14.52.37099"])],
            &[],
        );
        let fake = Fake::default()
            .answer(
                "vswhere.exe",
                &json(&[entry(root.path(), "VS", false, true)]),
                b"",
            )
            .answer("14.44.35207/bin/Hostx64/x64/cl.exe", b"", BANNER_44)
            .answer("14.52.37025/bin/Hostx64/x64/cl.exe", b"", BANNER_52)
            .answer("14.52.37099/bin/Hostx64/x64/cl.exe", b"", BANNER_52);
        let toolsets = &find_installations(&env, &fake).unwrap()[0].toolsets;
        let names: Vec<_> = toolsets
            .iter()
            .map(|t| t.dir.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["14.52.37099", "14.52.37025", "14.44.35207"]);
        assert_eq!(toolsets[2].cl_version.to_string(), "19.44.35207");
    }

    /// Mutation: abort on a toolset whose cl.exe tells no version, or keep it
    /// among the usable ones; this test fails.
    #[test]
    fn a_broken_toolset_does_not_hide_the_others() {
        let (root, env) = tree(&[("VS", &["14.44.35207", "14.52.37025"])], &[]);
        let fake = Fake::default()
            .answer(
                "vswhere.exe",
                &json(&[entry(root.path(), "VS", false, true)]),
                b"",
            )
            .answer("14.44.35207/bin/Hostx64/x64/cl.exe", b"", b"garbage")
            .answer("14.52.37025/bin/Hostx64/x64/cl.exe", b"", BANNER_52);
        let found = &find_installations(&env, &fake).unwrap()[0];
        assert_eq!(found.toolsets.len(), 1);
        assert_eq!(found.unusable_toolsets.len(), 1);
        assert!(found.unusable_toolsets[0].ends_with("14.44.35207"));
    }

    /// Mutation: report `NoVisualStudio` when toolsets exist but none tells
    /// its version; this test fails.
    #[test]
    fn only_broken_toolsets_is_named_as_such() {
        let (root, env) = tree(&[("VS", &["14.52.37025"])], &[]);
        let fake = Fake::default()
            .answer(
                "vswhere.exe",
                &json(&[entry(root.path(), "VS", false, true)]),
                b"",
            )
            .answer("cl.exe", b"", b"no version here");
        let err = find_installations(&env, &fake).unwrap_err();
        assert!(
            matches!(err, ToolchainError::ClVersionUnknown { .. }),
            "{err}"
        );
    }

    /// Mutation: list a directory without cl.exe as a toolset or as unusable,
    /// or return an installation with no toolset; this test fails.
    #[test]
    fn directories_without_cl_and_installations_without_toolsets_are_left_out() {
        let (root, env) = tree(&[("VS", &["14.52.37025"]), ("Empty", &[])], &[]);
        std::fs::create_dir_all(root.path().join("VS/VC/Tools/MSVC/14.99.0")).unwrap();
        let fake = Fake::default()
            .answer(
                "vswhere.exe",
                &json(&[
                    entry(root.path(), "VS", false, true),
                    entry(root.path(), "Empty", false, true),
                ]),
                b"",
            )
            .answer("cl.exe", b"", BANNER_52);
        let found = find_installations(&env, &fake).unwrap();
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].toolsets.len(), 1);
        assert!(found[0].unusable_toolsets.is_empty());
    }

    /// Mutation: return an empty list instead of `NoVisualStudio`; this test
    /// fails.
    #[test]
    fn no_installation_is_an_error() {
        let (_root, env) = tree(&[], &[]);
        let fake = Fake::default().answer("vswhere.exe", &json(&[]), b"");
        assert!(matches!(
            find_installations(&env, &fake),
            Err(ToolchainError::NoVisualStudio)
        ));
    }

    /// Mutation: skip the `is_file` check on vswhere.exe, or drop the
    /// `ProgramFiles(x86)` lookup; this test fails.
    #[test]
    fn a_missing_vswhere_is_named_with_its_path() {
        let env = Env::from_pairs(
            [("ProgramFiles(x86)".into(), "Z:/nowhere".into())],
            NameCase::Insensitive,
        );
        let err = find_installations(&env, &Fake::default()).unwrap_err();
        assert!(
            matches!(err, ToolchainError::VswhereMissing { .. }),
            "{err}"
        );
        assert!(err.to_string().contains("nowhere"), "{err}");
    }

    /// Mutation: ignore vswhere's exit status, or drop its output from the
    /// error; this test fails.
    #[test]
    fn a_failing_vswhere_is_reported_with_its_output() {
        let (_root, env) = tree(&[], &[]);
        let fake = Fake::default().answer_with(
            "vswhere.exe",
            false,
            b"",
            b"Error 0x80070005: access denied",
        );
        let err = find_installations(&env, &fake).unwrap_err();
        assert!(matches!(err, ToolchainError::VswhereFailed { .. }), "{err}");
        assert!(err.to_string().contains("access denied"), "{err}");
    }

    /// Mutation: treat a vswhere that cannot start as no installations; this
    /// test fails.
    #[test]
    fn a_vswhere_that_cannot_start_is_a_vswhere_failure() {
        let (_root, env) = tree(&[], &[]);
        let err = find_installations(&env, &Fake::default()).unwrap_err();
        assert!(matches!(err, ToolchainError::VswhereFailed { .. }), "{err}");
    }

    /// Mutation: accept output that is not JSON, or JSON that is not a list,
    /// or an entry without `installationPath`, as no installations; this test
    /// fails.
    #[test]
    fn output_vswhere_does_not_normally_give_is_a_vswhere_failure() {
        for stdout in [
            &b"Visual Studio Locator"[..],
            b"{}",
            br#"[{"isPrerelease":false}]"#,
        ] {
            let (_root, env) = tree(&[], &[]);
            let fake = Fake::default().answer("vswhere.exe", stdout, b"");
            let result = find_installations(&env, &fake);
            assert!(
                matches!(result, Err(ToolchainError::VswhereFailed { .. })),
                "{}: {result:?}",
                String::from_utf8_lossy(stdout)
            );
        }
    }

    /// Mutation: drop the default-toolset file read, or its `trim`; this test
    /// fails.
    #[test]
    fn the_default_toolset_is_read_from_visual_studio() {
        let (root, env) = tree(&[("VS", &["14.52.37025"])], &[]);
        let build = root.path().join("VS/VC/Auxiliary/Build");
        std::fs::create_dir_all(&build).unwrap();
        std::fs::write(
            build.join("Microsoft.VCToolsVersion.default.txt"),
            "14.52.37025\r\n",
        )
        .unwrap();
        let fake = Fake::default()
            .answer(
                "vswhere.exe",
                &json(&[entry(root.path(), "VS", false, true)]),
                b"",
            )
            .answer("cl.exe", b"", BANNER_52);
        let found = find_installations(&env, &fake).unwrap();
        assert_eq!(found[0].default_toolset.as_deref(), Some("14.52.37025"));
    }
}
