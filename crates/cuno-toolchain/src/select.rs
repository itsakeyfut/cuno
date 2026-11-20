//! Choosing the compiler: which one the user asked for, which installed copy
//! satisfies the version requirements, and whether a Clang is accepted by the
//! MSVC standard library. See `docs/en/specs/toolchain.md`, "Selection",
//! "Version pinning" and "Minimum compiler versions".

use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};

use cuno_core::Env;

use crate::llvm::{LlvmTool, Probe, Tool};
use crate::msvc::{MsvcToolset, VsInstallation};
use crate::{HostOs, Runner, ToolchainError, Version};

/// A compiler cuno can build with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompilerKind {
    /// cl.exe.
    Msvc,
    /// clang with GCC-style flags.
    Clang,
    /// clang-cl, Clang with MSVC-style flags.
    ClangCl,
}

impl CompilerKind {
    /// The name used in settings (`msvc`, `clang`, `clang-cl`).
    pub fn name(self) -> &'static str {
        match self {
            CompilerKind::Msvc => "msvc",
            CompilerKind::Clang => "clang",
            CompilerKind::ClangCl => "clang-cl",
        }
    }

    /// The oldest version cuno supports (`toolchain.md`, "Minimum compiler
    /// versions").
    fn minimum(self) -> Version {
        match self {
            CompilerKind::Msvc => Version {
                major: 19,
                minor: 29,
                patch: 0,
            },
            CompilerKind::Clang | CompilerKind::ClangCl => Version {
                major: 17,
                minor: 0,
                patch: 0,
            },
        }
    }

    fn parse(name: &str, source: Source) -> Result<CompilerKind, ToolchainError> {
        match name {
            "msvc" => Ok(CompilerKind::Msvc),
            "clang" => Ok(CompilerKind::Clang),
            "clang-cl" => Ok(CompilerKind::ClangCl),
            "gcc" | "safec" => Err(ToolchainError::UnsupportedCompiler {
                name: name.to_owned(),
                source: source.to_string(),
            }),
            _ => Err(ToolchainError::UnknownCompiler {
                name: name.to_owned(),
                source: source.to_string(),
            }),
        }
    }
}

/// Where the choice of compiler came from, highest precedence first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// `--compiler`.
    CommandLine,
    /// `CUNO_COMPILER`.
    Environment,
    /// The project's `.cuno/config.toml`.
    ProjectConfig,
    /// `[toolchain] compiler` in `cuno.toml`.
    Manifest,
    /// The user-level `config.toml`.
    UserConfig,
    /// Nothing asked: the platform's default.
    Automatic,
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Source::CommandLine => "--compiler",
            Source::Environment => "CUNO_COMPILER",
            Source::ProjectConfig => "compiler in .cuno/config.toml",
            Source::Manifest => "[toolchain] compiler in cuno.toml",
            Source::UserConfig => "compiler in the user-level config.toml",
            Source::Automatic => "automatic selection",
        })
    }
}

/// The compiler names each layer gives, as read by whoever reads them.
#[derive(Clone, Debug, Default)]
pub struct Choices {
    /// `--compiler`.
    pub cli: Option<String>,
    /// `CUNO_COMPILER`.
    pub env: Option<String>,
    /// The project's layers; `None` outside a project, where they do not
    /// exist (`toolchain.md`, "Selection").
    pub project: Option<ProjectChoices>,
    /// The user-level `config.toml`.
    pub user: Option<String>,
}

/// The compiler names a project's own layers give.
#[derive(Clone, Debug, Default)]
pub struct ProjectChoices {
    /// `.cuno/config.toml`.
    pub config: Option<String>,
    /// `[toolchain] compiler` in `cuno.toml`.
    pub manifest: Option<String>,
}

impl Choices {
    /// The highest-precedence name given, and where it came from.
    fn requested(&self) -> Option<(&str, Source)> {
        let project = self.project.as_ref();
        [
            (self.cli.as_deref(), Source::CommandLine),
            (self.env.as_deref(), Source::Environment),
            (
                project.and_then(|p| p.config.as_deref()),
                Source::ProjectConfig,
            ),
            (
                project.and_then(|p| p.manifest.as_deref()),
                Source::Manifest,
            ),
            (self.user.as_deref(), Source::UserConfig),
        ]
        .into_iter()
        .find_map(|(name, source)| Some((name?, source)))
    }
}

/// A `[toolchain.version]` value: comma-separated `>=`, `>`, `<=`, `<` or `=`
/// conditions, each compared only up to the components it writes
/// (`manifest.md`, "`[toolchain.version]`").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionReq {
    text: String,
    conditions: Vec<(Op, Vec<u32>)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Ge,
    Gt,
    Le,
    Lt,
    Eq,
}

impl VersionReq {
    /// Parses a requirement for `compiler`, such as `">=17, <19"`.
    pub fn parse(compiler: &str, text: &str) -> Result<VersionReq, ToolchainError> {
        let invalid = || ToolchainError::InvalidVersionRequirement {
            compiler: compiler.to_owned(),
            text: text.to_owned(),
        };
        let conditions = text
            .split(',')
            .map(|condition| {
                let condition = condition.trim();
                let (op, rest) = [
                    (">=", Op::Ge),
                    ("<=", Op::Le),
                    (">", Op::Gt),
                    ("<", Op::Lt),
                    ("=", Op::Eq),
                ]
                .into_iter()
                .find_map(|(s, op)| Some((op, condition.strip_prefix(s)?)))
                .ok_or_else(invalid)?;
                let parts: Vec<u32> = rest
                    .trim()
                    .split('.')
                    .map(|part| part.parse().map_err(|_| invalid()))
                    .collect::<Result<_, _>>()?;
                if parts.len() > 3 {
                    return Err(invalid());
                }
                Ok((op, parts))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(VersionReq {
            text: text.to_owned(),
            conditions,
        })
    }

    /// Whether `version` satisfies every condition.
    pub fn matches(&self, version: &Version) -> bool {
        let full = [version.major, version.minor, version.patch];
        self.conditions.iter().all(|(op, parts)| {
            let ordering = full[..parts.len()].cmp(parts);
            match op {
                Op::Ge => ordering.is_ge(),
                Op::Gt => ordering.is_gt(),
                Op::Le => ordering.is_le(),
                Op::Lt => ordering.is_lt(),
                Op::Eq => ordering.is_eq(),
            }
        })
    }
}

impl fmt::Display for VersionReq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// `[toolchain.version]`, for the compilers cuno can build with.
#[derive(Clone, Debug, Default)]
pub struct Constraints {
    /// `msvc`.
    pub msvc: Option<VersionReq>,
    /// `clang`.
    pub clang: Option<VersionReq>,
    /// `clang-cl`.
    pub clang_cl: Option<VersionReq>,
}

impl Constraints {
    fn of(&self, kind: CompilerKind) -> Option<&VersionReq> {
        match kind {
            CompilerKind::Msvc => self.msvc.as_ref(),
            CompilerKind::Clang => self.clang.as_ref(),
            CompilerKind::ClangCl => self.clang_cl.as_ref(),
        }
    }
}

/// What detection found.
#[derive(Clone, Copy, Debug)]
pub struct Found<'a> {
    /// Every Visual Studio installation.
    pub installs: &'a [VsInstallation],
    /// Every LLVM tool.
    pub llvm: &'a [LlvmTool],
    /// `[toolchain.paths] msvc`: the only installation to use, when written.
    pub explicit_msvc: Option<&'a Path>,
}

/// The compiler to build with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selected {
    /// Which compiler.
    pub kind: CompilerKind,
    /// Its executable.
    pub executable: PathBuf,
    /// Its version.
    pub version: Version,
    /// The MSVC toolset whose headers and libraries it uses (Windows).
    pub msvc: Option<MsvcToolset>,
    /// Where the choice came from.
    pub source: Source,
}

/// Chooses the compiler: the highest-precedence name given, else the
/// platform's default, then its newest installed copy that satisfies cuno's
/// minimum and `[toolchain.version]`, never a preview.
pub fn select(
    choices: &Choices,
    found: Found<'_>,
    constraints: &Constraints,
    host: HostOs,
) -> Result<Selected, ToolchainError> {
    let (kind, source) = match choices.requested() {
        Some((name, source)) => (CompilerKind::parse(name, source)?, source),
        None => match host {
            HostOs::Windows => (CompilerKind::Msvc, Source::Automatic),
            // GCC on Linux and Apple Clang on macOS come after the MVP.
            HostOs::Other => {
                return Err(ToolchainError::UnsupportedCompiler {
                    name: "gcc".to_owned(),
                    source: Source::Automatic.to_string(),
                });
            }
        },
    };
    let requirement = constraints.of(kind);
    let acceptable = |version: &Version| {
        *version >= kind.minimum() && requirement.is_none_or(|r| r.matches(version))
    };

    // On Windows every compiler uses an MSVC toolset (`toolchain.md`,
    // "Environment resolution on Windows"), so Visual Studio is required
    // even for Clang.
    let toolsets = match host {
        HostOs::Windows => usable_toolsets(found)?,
        HostOs::Other => Vec::new(),
    };
    let newest_toolset = toolsets.iter().max_by_key(|t| t.cl_version).cloned();

    let candidates: Vec<(PathBuf, Version, Option<MsvcToolset>)> = match kind {
        CompilerKind::Msvc => {
            if host != HostOs::Windows {
                return Err(ToolchainError::NoVisualStudio);
            }
            toolsets
                .iter()
                .map(|t| (t.cl(), t.cl_version, Some(t.clone())))
                .collect()
        }
        CompilerKind::Clang | CompilerKind::ClangCl => {
            let tool = if kind == CompilerKind::Clang {
                Tool::Clang
            } else {
                Tool::ClangCl
            };
            found
                .llvm
                .iter()
                .filter(|t| t.tool == tool)
                .filter_map(|t| release_for(t, host))
                .map(|(path, version)| (path, version, newest_toolset.clone()))
                .collect()
        }
    };
    if candidates.is_empty() {
        return Err(ToolchainError::CompilerNotFound {
            compiler: kind.name(),
        });
    }
    // The newest acceptable one; among equals, the first in search order.
    let mut best: Option<&(PathBuf, Version, Option<MsvcToolset>)> = None;
    for candidate in candidates.iter().filter(|c| acceptable(&c.1)) {
        if best.is_none_or(|b| candidate.1 > b.1) {
            best = Some(candidate);
        }
    }
    let Some((executable, version, msvc)) = best.cloned() else {
        let mut requirements = vec![format!(">={} (cuno's minimum)", kind.minimum())];
        if let Some(r) = requirement {
            requirements.push(format!("{r} ([toolchain.version] {})", kind.name()));
        }
        return Err(ToolchainError::NoMatchingVersion {
            compiler: kind.name(),
            requirements,
            found: candidates.iter().map(|c| c.1).collect(),
        });
    };
    Ok(Selected {
        kind,
        executable,
        version,
        msvc,
        source,
    })
}

/// The toolsets of the complete, non-preview installations (only the one in
/// `[toolchain.paths] msvc`, when written).
fn usable_toolsets(found: Found<'_>) -> Result<Vec<MsvcToolset>, ToolchainError> {
    let installs: Vec<&VsInstallation> = found
        .installs
        .iter()
        .filter(|i| i.complete && !i.prerelease)
        .filter(|i| found.explicit_msvc.is_none_or(|dir| same_dir(dir, &i.path)))
        .collect();
    if installs.is_empty() {
        return Err(match found.explicit_msvc {
            Some(dir) => ToolchainError::VisualStudioNotAt {
                path: dir.to_owned(),
            },
            None => ToolchainError::NoVisualStudio,
        });
    }
    Ok(installs
        .into_iter()
        .flat_map(|i| i.toolsets.iter().cloned())
        .collect())
}

fn same_dir(a: &Path, b: &Path) -> bool {
    let canonical = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_owned());
    canonical(a) == canonical(b)
}

/// The path and version of a tool that ran, is a release, and on Windows
/// targets MSVC (a MinGW Clang cannot be linked with MSVC; `toolchain.md`,
/// "LLVM").
fn release_for(tool: &LlvmTool, host: HostOs) -> Option<(PathBuf, Version)> {
    let Probe::Ran {
        version,
        prerelease: false,
        target,
    } = &tool.probe
    else {
        return None;
    };
    if host == HostOs::Windows
        && !target
            .as_deref()
            .is_some_and(|t| t.ends_with("-windows-msvc"))
    {
        return None;
    }
    Some((tool.path.clone(), *version))
}

/// What the build's compile command would also get, for the standard library
/// probe.
#[derive(Clone, Copy, Debug)]
pub struct ProbeInputs<'a> {
    /// A directory outside the project to write the probe file in.
    pub scratch: &'a Path,
    /// The build's include directories.
    pub include_dirs: &'a [PathBuf],
    /// The project's `defines` (`NAME` or `NAME=value`).
    pub defines: &'a [String],
}

/// Whether the selected compiler can build here: on Windows a Clang must be
/// accepted by the MSVC standard library, which is asked directly by
/// syntax-checking a file that includes `<yvals_core.h>` (`toolchain.md`,
/// "Minimum compiler versions"). `env` is the resolved Windows environment.
/// The one check `cuno doctor` and the build commands share.
pub fn check(
    selected: &Selected,
    env: &Env,
    inputs: ProbeInputs<'_>,
    run: &dyn Runner,
) -> Result<(), ToolchainError> {
    if selected.kind == CompilerKind::Msvc || selected.msvc.is_none() {
        return Ok(());
    }
    let probe = inputs.scratch.join("cuno-stl-probe.cpp");
    std::fs::write(&probe, "#include <yvals_core.h>\n").map_err(|e| {
        ToolchainError::ProbeFailed {
            compiler: selected.kind.name(),
            output: format!("could not write {}: {e}", probe.display()),
        }
    })?;
    let mut args: Vec<std::ffi::OsString> = match selected.kind {
        CompilerKind::ClangCl => vec!["/nologo".into(), "/TP".into(), "/Zs".into()],
        _ => vec!["-x".into(), "c++".into(), "-fsyntax-only".into()],
    };
    args.push("--target=x86_64-pc-windows-msvc".into());
    let (define, include) = match selected.kind {
        CompilerKind::ClangCl => ("/D", "/I"),
        _ => ("-D", "-I"),
    };
    args.extend(inputs.defines.iter().map(|d| format!("{define}{d}").into()));
    for dir in inputs.include_dirs {
        let mut arg = std::ffi::OsString::from(include);
        arg.push(dir);
        args.push(arg);
    }
    args.push(probe.clone().into());
    let args: Vec<&OsStr> = args.iter().map(|a| a.as_os_str()).collect();
    let output = run.run(&selected.executable, &args, env);
    let _ = std::fs::remove_file(&probe);
    let output = output.map_err(|e| ToolchainError::ProbeFailed {
        compiler: selected.kind.name(),
        output: e.to_string(),
    })?;
    if output.success {
        return Ok(());
    }
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    match text.lines().find(|line| line.contains("STL1000:")) {
        Some(line) => Err(ToolchainError::RejectedByStl {
            compiler: selected.kind.name(),
            version: selected.version,
            needed: needed_clang(line),
            message: line.trim().to_owned(),
        }),
        None => Err(ToolchainError::ProbeFailed {
            compiler: selected.kind.name(),
            output: text.trim().to_owned(),
        }),
    }
}

/// `<version>` in the standard library's `expected Clang <version> or newer`,
/// as written.
fn needed_clang(message: &str) -> Option<String> {
    let rest = &message[message.find("expected Clang ")? + "expected Clang ".len()..];
    let version = &rest[..rest.find(" or newer")?];
    (!version.is_empty()).then(|| version.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Output;
    use crate::llvm::Origin;

    fn v(major: u32, minor: u32, patch: u32) -> Version {
        Version {
            major,
            minor,
            patch,
        }
    }

    fn toolset(version: Version) -> MsvcToolset {
        MsvcToolset {
            dir: format!("VS/VC/Tools/MSVC/{version}").into(),
            cl_version: version,
        }
    }

    fn install(path: &str, toolsets: &[Version]) -> VsInstallation {
        VsInstallation {
            path: path.into(),
            prerelease: false,
            complete: true,
            toolsets: toolsets.iter().map(|v| toolset(*v)).collect(),
            unusable_toolsets: Vec::new(),
            default_toolset: None,
        }
    }

    fn clang(tool: Tool, path: &str, version: Version, prerelease: bool, target: &str) -> LlvmTool {
        LlvmTool {
            tool,
            path: path.into(),
            probe: Probe::Ran {
                version,
                prerelease,
                target: Some(target.into()),
            },
            origin: Origin::Path,
        }
    }

    const MSVC_TARGET: &str = "x86_64-pc-windows-msvc";

    fn found<'a>(installs: &'a [VsInstallation], llvm: &'a [LlvmTool]) -> Found<'a> {
        Found {
            installs,
            llvm,
            explicit_msvc: None,
        }
    }

    fn some(name: &str) -> Option<String> {
        Some(name.to_owned())
    }

    /// Mutation: swap any two levels of the precedence; this test fails.
    #[test]
    fn each_level_beats_every_level_below_it() {
        let installs = [install("VS", &[v(19, 52, 1)])];
        let llvm = [
            clang(Tool::Clang, "clang", v(22, 1, 8), false, MSVC_TARGET),
            clang(Tool::ClangCl, "clang-cl", v(22, 1, 8), false, MSVC_TARGET),
        ];
        // Each level names a different compiler than every level below it;
        // whichever wins tells which level won.
        let levels = ["clang", "clang-cl", "msvc", "clang", "clang-cl"];
        let sources = [
            Source::CommandLine,
            Source::Environment,
            Source::ProjectConfig,
            Source::Manifest,
            Source::UserConfig,
        ];
        for top in 0..=levels.len() {
            let given = |i: usize| (i >= top).then(|| levels[i].to_owned());
            let choices = Choices {
                cli: given(0),
                env: given(1),
                project: Some(ProjectChoices {
                    config: given(2),
                    manifest: given(3),
                }),
                user: given(4),
            };
            let selected = select(
                &choices,
                found(&installs, &llvm),
                &Constraints::default(),
                HostOs::Windows,
            )
            .unwrap();
            if top < levels.len() {
                assert_eq!(selected.source, sources[top]);
                assert_eq!(selected.kind.name(), levels[top]);
            } else {
                assert_eq!(selected.source, Source::Automatic);
                assert_eq!(selected.kind, CompilerKind::Msvc);
            }
        }
    }

    /// Mutation: read the project's layers outside a project; this test
    /// fails. Outside a project they are not even representable, so this
    /// checks the user-level choice is the one taken.
    #[test]
    fn outside_a_project_the_user_config_is_next_after_the_environment() {
        let installs = [install("VS", &[v(19, 52, 1)])];
        let llvm = [clang(Tool::Clang, "clang", v(22, 1, 8), false, MSVC_TARGET)];
        let choices = Choices {
            user: some("clang"),
            ..Default::default()
        };
        let selected = select(
            &choices,
            found(&installs, &llvm),
            &Constraints::default(),
            HostOs::Windows,
        )
        .unwrap();
        assert_eq!(
            (selected.kind, selected.source),
            (CompilerKind::Clang, Source::UserConfig)
        );
    }

    /// Mutation: fall back to automatic selection for `gcc`, `safec` or an
    /// unknown name, or lose where the name came from; this test fails.
    #[test]
    fn unsupported_and_unknown_names_are_errors_naming_their_source() {
        let installs = [install("VS", &[v(19, 52, 1)])];
        for (name, unsupported) in [("gcc", true), ("safec", true), ("nope", false)] {
            let choices = Choices {
                env: some(name),
                ..Default::default()
            };
            let err = select(
                &choices,
                found(&installs, &[]),
                &Constraints::default(),
                HostOs::Windows,
            )
            .unwrap_err();
            match (&err, unsupported) {
                (ToolchainError::UnsupportedCompiler { source, .. }, true)
                | (ToolchainError::UnknownCompiler { source, .. }, false) => {
                    assert_eq!(source, "CUNO_COMPILER")
                }
                _ => panic!("{name}: {err:?}"),
            }
        }
    }

    /// Mutation: include previews, take the oldest, or ignore the upper
    /// bound; this test fails.
    #[test]
    fn the_newest_satisfying_release_is_chosen() {
        let mut preview = install("Preview", &[v(19, 44, 1)]);
        preview.prerelease = true;
        let installs = [install("VS", &[v(19, 40, 1), v(19, 52, 1)]), preview];
        let constraints = Constraints {
            msvc: Some(VersionReq::parse("msvc", ">=19.29, <19.50").unwrap()),
            ..Default::default()
        };
        let selected = select(
            &Choices::default(),
            found(&installs, &[]),
            &constraints,
            HostOs::Windows,
        )
        .unwrap();
        assert_eq!(selected.version, v(19, 40, 1));

        let llvm = [
            clang(Tool::Clang, "old", v(18, 1, 0), false, MSVC_TARGET),
            clang(Tool::Clang, "rc", v(23, 0, 0), true, MSVC_TARGET),
            clang(
                Tool::Clang,
                "mingw",
                v(22, 5, 0),
                false,
                "x86_64-w64-windows-gnu",
            ),
            clang(Tool::Clang, "new", v(22, 1, 8), false, MSVC_TARGET),
            clang(Tool::Clang, "same", v(22, 1, 8), false, MSVC_TARGET),
        ];
        let choices = Choices {
            cli: some("clang"),
            ..Default::default()
        };
        let selected = select(
            &choices,
            found(&installs, &llvm),
            &Constraints::default(),
            HostOs::Windows,
        )
        .unwrap();
        assert_eq!(selected.executable, PathBuf::from("new"));
        // Clang uses the newest MSVC toolset; the MSVC constraint is not
        // Clang's.
        assert_eq!(selected.msvc.unwrap().cl_version, v(19, 52, 1));
    }

    /// Mutation: drop cuno's minimum, or report a failure without the versions
    /// found; this test fails.
    #[test]
    fn below_the_minimum_is_an_error_listing_what_was_found() {
        let installs = [install("VS", &[v(19, 28, 1)])];
        let err = select(
            &Choices::default(),
            found(&installs, &[]),
            &Constraints::default(),
            HostOs::Windows,
        )
        .unwrap_err();
        assert!(
            matches!(&err, ToolchainError::NoMatchingVersion { found, .. } if *found == [v(19, 28, 1)]),
            "{err:?}"
        );
    }

    /// Mutation: choose Clang without a Visual Studio, ignore `[toolchain.paths]
    /// msvc`, or report a missing Clang as a version problem; this test fails.
    #[test]
    fn visual_studio_is_required_and_paths_msvc_limits_it() {
        let llvm = [clang(Tool::Clang, "clang", v(22, 1, 8), false, MSVC_TARGET)];
        let clang_choice = Choices {
            cli: some("clang"),
            ..Default::default()
        };
        let err = select(
            &clang_choice,
            found(&[], &llvm),
            &Constraints::default(),
            HostOs::Windows,
        )
        .unwrap_err();
        assert!(matches!(err, ToolchainError::NoVisualStudio), "{err:?}");

        let root = tempfile::tempdir().unwrap();
        let installs = [
            install(root.path().join("A").to_str().unwrap(), &[v(19, 52, 1)]),
            install(root.path().join("B").to_str().unwrap(), &[v(19, 40, 1)]),
        ];
        let explicit = root.path().join("B");
        let selected = select(
            &Choices::default(),
            Found {
                installs: &installs,
                llvm: &[],
                explicit_msvc: Some(&explicit),
            },
            &Constraints::default(),
            HostOs::Windows,
        )
        .unwrap();
        assert_eq!(selected.version, v(19, 40, 1));
        let elsewhere = root.path().join("C");
        let err = select(
            &Choices::default(),
            Found {
                installs: &installs,
                llvm: &[],
                explicit_msvc: Some(&elsewhere),
            },
            &Constraints::default(),
            HostOs::Windows,
        )
        .unwrap_err();
        assert!(
            matches!(err, ToolchainError::VisualStudioNotAt { .. }),
            "{err:?}"
        );

        let err = select(
            &clang_choice,
            found(&installs, &[]),
            &Constraints::default(),
            HostOs::Windows,
        )
        .unwrap_err();
        assert!(
            matches!(err, ToolchainError::CompilerNotFound { compiler: "clang" }),
            "{err:?}"
        );
    }

    /// Mutation: compare every component instead of only those written, or
    /// treat `>` as `>=`; this test fails.
    #[test]
    fn requirements_compare_only_the_components_written() {
        let req = |text| VersionReq::parse("msvc", text).unwrap();
        assert!(req("=19.40").matches(&v(19, 40, 33811)));
        assert!(req(">=17").matches(&v(17, 0, 6)));
        assert!(!req("<19").matches(&v(19, 1, 0)));
        assert!(!req(">19.40").matches(&v(19, 40, 9)));
        assert!(req(">19.40").matches(&v(19, 41, 0)));
        assert!(req("<= 19.40, >=19.29").matches(&v(19, 40, 99)));
        for bad in ["19", "~19", ">=", ">=a.b", ">=1.2.3.4"] {
            assert!(
                matches!(
                    VersionReq::parse("msvc", bad),
                    Err(ToolchainError::InvalidVersionRequirement { .. })
                ),
                "{bad}"
            );
        }
    }

    struct Answers(bool, &'static str, std::cell::RefCell<Vec<String>>);
    impl Runner for Answers {
        fn run(&self, _p: &Path, args: &[&OsStr], _e: &Env) -> std::io::Result<Output> {
            *self.2.borrow_mut() = args
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            Ok(Output {
                success: self.0,
                stdout: self.1.as_bytes().to_vec(),
                stderr: Vec::new(),
            })
        }
    }

    fn selected(kind: CompilerKind) -> Selected {
        Selected {
            kind,
            executable: "clang".into(),
            version: v(20, 1, 8),
            msvc: Some(toolset(v(19, 52, 1))),
            source: Source::CommandLine,
        }
    }

    /// Mutation: treat a failed probe as passing, lose the version the
    /// library asks for, or call any failure STL1000; this test fails.
    #[test]
    fn the_probe_verdict_follows_the_standard_library() {
        let root = tempfile::tempdir().unwrap();
        let inputs = ProbeInputs {
            scratch: root.path(),
            include_dirs: &[],
            defines: &[],
        };
        let stl = "x\\yvals_core.h(922,1): error: STL1000: Unexpected compiler version, expected Clang 22 or newer.\n";
        let run = Answers(false, stl, Default::default());
        let err = check(
            &selected(CompilerKind::Clang),
            &Env::default(),
            inputs,
            &run,
        )
        .unwrap_err();
        assert!(
            matches!(&err, ToolchainError::RejectedByStl { needed: Some(n), .. } if n == "22"),
            "{err:?}"
        );
        let other = Answers(
            false,
            "fatal error: 'yvals_core.h' file not found",
            Default::default(),
        );
        let err = check(
            &selected(CompilerKind::Clang),
            &Env::default(),
            inputs,
            &other,
        )
        .unwrap_err();
        assert!(matches!(err, ToolchainError::ProbeFailed { .. }), "{err:?}");
        let ok = Answers(true, "", Default::default());
        check(&selected(CompilerKind::Clang), &Env::default(), inputs, &ok).unwrap();
        assert_eq!(
            needed_clang("expected Clang 17.0.0 or newer."),
            Some("17.0.0".into())
        );
        assert_eq!(needed_clang("STL1000: Unexpected compiler version"), None);
    }

    /// Mutation: probe as C, without the target, or without the project's
    /// defines and include directories, or with the other driver's flags;
    /// this test fails.
    #[test]
    fn the_probe_compiles_as_cpp_under_the_builds_conditions() {
        let root = tempfile::tempdir().unwrap();
        let include_dirs = [PathBuf::from("inc")];
        let defines = ["_ALLOW_COMPILER_AND_STL_VERSION_MISMATCH".to_owned()];
        let inputs = ProbeInputs {
            scratch: root.path(),
            include_dirs: &include_dirs,
            defines: &defines,
        };
        let probe = root
            .path()
            .join("cuno-stl-probe.cpp")
            .to_string_lossy()
            .into_owned();
        for (kind, expected) in [
            (
                CompilerKind::Clang,
                vec![
                    "-x",
                    "c++",
                    "-fsyntax-only",
                    "--target=x86_64-pc-windows-msvc",
                    "-D_ALLOW_COMPILER_AND_STL_VERSION_MISMATCH",
                    "-Iinc",
                    &probe,
                ],
            ),
            (
                CompilerKind::ClangCl,
                vec![
                    "/nologo",
                    "/TP",
                    "/Zs",
                    "--target=x86_64-pc-windows-msvc",
                    "/D_ALLOW_COMPILER_AND_STL_VERSION_MISMATCH",
                    "/Iinc",
                    &probe,
                ],
            ),
        ] {
            let run = Answers(true, "", Default::default());
            check(&selected(kind), &Env::default(), inputs, &run).unwrap();
            assert_eq!(*run.2.borrow(), expected, "{kind:?}");
        }
        // MSVC is its own standard library's compiler: nothing to ask.
        let run = Answers(false, "STL1000:", Default::default());
        check(&selected(CompilerKind::Msvc), &Env::default(), inputs, &run).unwrap();
    }
}
