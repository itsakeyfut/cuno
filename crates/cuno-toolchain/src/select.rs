//! Choosing the compiler: which one the user asked for, and which installed
//! copy satisfies the version requirements. See `docs/en/specs/toolchain.md`,
//! "Selection" and "Version pinning". Whether the copy chosen can build here
//! is [`crate::probe`]'s.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::llvm::{LlvmTool, Origin, Probe, Tool};
use crate::msvc::{MsvcToolset, VsInstallation};
use cuno_core::paths;

use crate::{HostOs, ToolchainError, Version};

pub use crate::compiler::CompilerKind;

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
    /// The compiler asked for and where the request came from: the
    /// highest-precedence name given, else the platform's default. An
    /// unknown or unsupported name is an error naming where it was given.
    pub fn kind(&self, host: HostOs) -> Result<(CompilerKind, Source), ToolchainError> {
        match self.requested() {
            Some((name, source)) => Ok((CompilerKind::parse(name, source)?, source)),
            None => match host {
                HostOs::Windows => Ok((CompilerKind::Msvc, Source::Automatic)),
                // GCC on Linux and Apple Clang on macOS come after the MVP.
                HostOs::Other => Err(ToolchainError::UnsupportedCompiler {
                    name: "gcc".to_owned(),
                    source: Source::Automatic.to_string(),
                }),
            },
        }
    }

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
#[derive(Clone, Debug, Default, PartialEq, Eq)]
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
    let (kind, source) = choices.kind(host)?;
    // On Windows every compiler uses an MSVC toolset (`toolchain.md`,
    // "Environment resolution on Windows"), so Visual Studio is required
    // even for Clang.
    let newest_toolset = match host {
        HostOs::Windows => {
            let toolsets = usable_toolsets(found, host)?;
            first_newest(toolsets.iter().map(|t| (t, t.cl_version))).cloned()
        }
        HostOs::Other => None,
    };
    let (executable, version, msvc) = copy_of(kind, found, constraints, host)?;
    Ok(Selected {
        kind,
        executable,
        version,
        // A Clang gets the newest toolset; MSVC is its own.
        msvc: msvc.or(newest_toolset),
        source,
    })
}

/// The copy of `kind` that selecting it would use, and its version (with
/// its own toolset for MSVC), or why there is none: the one written in
/// `[toolchain.paths]` when there is one, which must be usable, otherwise
/// the newest eligible copy; cuno's minimum and `constraints` apply either
/// way. The one answer `select` and `cuno doctor`'s `Info` lines share. A
/// Clang is answered without Visual Studio, which `select` requires
/// separately.
pub fn copy_of(
    kind: CompilerKind,
    found: Found<'_>,
    constraints: &Constraints,
    host: HostOs,
) -> Result<(PathBuf, Version, Option<MsvcToolset>), ToolchainError> {
    let requirement = constraints.of(kind);
    let acceptable = |version: &Version| {
        *version >= kind.minimum() && requirement.is_none_or(|r| r.matches(version))
    };
    let mut requirements = vec![format!(">={} (cuno's minimum)", kind.minimum())];
    if let Some(r) = requirement {
        requirements.push(format!("{r} ([toolchain.version] {})", kind.name()));
    }

    let mut candidates: Vec<(PathBuf, Version, Option<MsvcToolset>)> = Vec::new();
    // Copies found but not eligible, with why, for the error.
    let mut excluded: Vec<String> = Vec::new();
    match kind {
        CompilerKind::Msvc => {
            if host != HostOs::Windows {
                return Err(ToolchainError::NoVisualStudio);
            }
            candidates.extend(
                usable_toolsets(found, host)?
                    .into_iter()
                    .map(|t| (t.cl(), t.cl_version, Some(t))),
            );
        }
        CompilerKind::Clang | CompilerKind::ClangCl => {
            let tool = kind
                .llvm_tool()
                .expect("the LLVM family is found as an LLVM tool");
            let configured = found
                .llvm
                .iter()
                .any(|t| t.tool == tool && t.origin == Origin::Configured);
            if configured {
                // The configured copy is the only one used (`toolchain.md`,
                // "LLVM (clang / clang-cl / clang-format / clang-tidy)"): one
                // that cannot be used is an error, never a reason to take
                // another.
                let chosen = select_tool(tool, found.llvm, host)?;
                if !acceptable(&chosen.version) {
                    return Err(ToolchainError::ConfiguredToolUnusable {
                        setting: format!("[toolchain.paths] {}", tool.name()),
                        path: chosen.path,
                        why: format!(
                            "{} does not satisfy {}",
                            chosen.version,
                            requirements.join(" and ")
                        ),
                    });
                }
                return Ok((chosen.path, chosen.version, None));
            }
            for t in found.llvm.iter().filter(|t| t.tool == tool) {
                match release_for(t, host) {
                    Ok(version) => candidates.push((t.path.clone(), version, None)),
                    Err(why) => excluded.push(format!("{} ({why})", t.path.display())),
                }
            }
        }
    }
    if candidates.is_empty() {
        return Err(if excluded.is_empty() {
            ToolchainError::CompilerNotFound {
                compiler: kind.name(),
            }
        } else {
            ToolchainError::NoUsableCompiler {
                compiler: kind.name(),
                excluded,
            }
        });
    }
    let best = first_newest(
        candidates
            .iter()
            .filter(|c| acceptable(&c.1))
            .map(|c| (c, c.1)),
    );
    match best.cloned() {
        Some(best) => Ok(best),
        None => Err(ToolchainError::NoMatchingVersion {
            compiler: kind.name(),
            requirements,
            found: candidates.iter().map(|c| c.1).collect(),
        }),
    }
}

/// The item with the highest version; among equals, the first, so that
/// search order decides and every compiler picks the same toolset.
fn first_newest<T>(items: impl Iterator<Item = (T, Version)>) -> Option<T> {
    let mut best: Option<(T, Version)> = None;
    for (item, version) in items {
        if best.as_ref().is_none_or(|(_, b)| version > *b) {
            best = Some((item, version));
        }
    }
    best.map(|(item, _)| item)
}

/// The toolsets of the complete, non-preview installations (only the one in
/// `[toolchain.paths] msvc`, when written).
fn usable_toolsets(found: Found<'_>, host: HostOs) -> Result<Vec<MsvcToolset>, ToolchainError> {
    let installs: Vec<&VsInstallation> = found
        .installs
        .iter()
        .filter(|i| i.complete && !i.prerelease)
        .filter(|i| {
            found
                .explicit_msvc
                .is_none_or(|dir| paths::same(dir, &i.path, host.name_case()))
        })
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

/// The version of a tool that ran and is a release, or why it is not one.
fn release(tool: &LlvmTool) -> Result<Version, String> {
    match &tool.probe {
        Probe::Failed => Err("it did not run".to_owned()),
        Probe::NoVersion => Err("it printed no version".to_owned()),
        Probe::Ran {
            version,
            prerelease: true,
            ..
        } => Err(format!("{version} is a preview")),
        Probe::Ran { version, .. } => Ok(*version),
    }
}

/// The version of a tool that ran, is a release, and on Windows targets MSVC (a
/// MinGW Clang cannot be linked with MSVC; `toolchain.md`, "LLVM (clang /
/// clang-cl / clang-format / clang-tidy)"), or why it is not eligible.
fn release_for(tool: &LlvmTool, host: HostOs) -> Result<Version, String> {
    let version = release(tool)?;
    if host == HostOs::Windows {
        let target = match &tool.probe {
            Probe::Ran { target, .. } => target,
            _ => &None,
        };
        match target.as_deref() {
            Some(t) if t.ends_with("-windows-msvc") => {}
            Some(t) => return Err(format!("it targets {t}, not MSVC")),
            None => return Err("it reports no target".to_owned()),
        }
    }
    Ok(version)
}

/// A clang-format or clang-tidy to use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LlvmChoice {
    /// The executable.
    pub path: PathBuf,
    /// Its version.
    pub version: Version,
}

/// The oldest clang-format and clang-tidy cuno supports
/// (`docs/en/specs/dev-tools.md`, "Tool versions").
pub const TOOL_MINIMUM: Version = Version {
    major: 17,
    minor: 0,
    patch: 0,
};

/// The copy of `tool` to use among those detected (`toolchain.md`, "LLVM (clang
/// / clang-cl / clang-format / clang-tidy)"): the one written in
/// `[toolchain.paths]` when there is one, which must be eligible, or else the
/// newest eligible copy; among equals, the first in search order. Eligible is a
/// release that ran, and for clang and clang-cl on Windows one targeting MSVC;
/// clang-format and clang-tidy have no target to check (clang-tidy prints
/// none), and must be at least [`TOOL_MINIMUM`]; the compilers' minimum and
/// `[toolchain.version]` are the compiler selection's.
pub fn select_tool(
    tool: Tool,
    llvm: &[LlvmTool],
    host: HostOs,
) -> Result<LlvmChoice, ToolchainError> {
    let eligible = |t: &LlvmTool| match tool {
        Tool::Clang | Tool::ClangCl => release_for(t, host),
        // 17 at least, so formatting and analysis do not change from one
        // machine to another (`dev-tools.md`, "Tool versions").
        Tool::ClangFormat | Tool::ClangTidy => release(t).and_then(|version| {
            if version >= TOOL_MINIMUM {
                Ok(version)
            } else {
                Err(format!(
                    "{version} does not satisfy >={TOOL_MINIMUM} (cuno's minimum)"
                ))
            }
        }),
    };
    let copies = || llvm.iter().filter(|t| t.tool == tool);
    if let Some(configured) = copies().find(|t| t.origin == Origin::Configured) {
        return match eligible(configured) {
            Ok(version) => Ok(LlvmChoice {
                path: configured.path.clone(),
                version,
            }),
            Err(why) => Err(ToolchainError::ConfiguredToolUnusable {
                setting: format!("[toolchain.paths] {}", tool.name()),
                path: configured.path.clone(),
                why,
            }),
        };
    }
    let mut excluded = Vec::new();
    let mut candidates = Vec::new();
    for t in copies() {
        match eligible(t) {
            Ok(version) => candidates.push((
                LlvmChoice {
                    path: t.path.clone(),
                    version,
                },
                version,
            )),
            Err(why) => excluded.push(format!("{} ({why})", t.path.display())),
        }
    }
    match first_newest(candidates.into_iter()) {
        Some(best) => Ok(best),
        None if excluded.is_empty() => Err(ToolchainError::CompilerNotFound {
            compiler: tool.name(),
        }),
        None => Err(ToolchainError::NoUsableCompiler {
            compiler: tool.name(),
            excluded,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llvm::Origin;

    fn v(major: u32, minor: u32, patch: u32) -> Version {
        Version {
            major,
            minor,
            patch,
        }
    }

    fn install(path: &str, toolsets: &[Version]) -> VsInstallation {
        VsInstallation {
            path: path.into(),
            prerelease: false,
            complete: true,
            toolsets: toolsets
                .iter()
                .map(|v| MsvcToolset {
                    dir: Path::new(path).join(format!("VC/Tools/MSVC/{v}")),
                    cl_version: *v,
                })
                .collect(),
            unusable_toolsets: Vec::new(),
            default_toolset: None,
        }
    }

    fn tool(tool: Tool, path: &str, probe: Probe) -> LlvmTool {
        LlvmTool {
            tool,
            path: path.into(),
            probe,
            origin: Origin::Path,
        }
    }

    fn ran(version: Version, prerelease: bool, target: Option<&str>) -> Probe {
        Probe::Ran {
            version,
            prerelease,
            target: target.map(Into::into),
        }
    }

    fn clang(kind: Tool, path: &str, version: Version) -> LlvmTool {
        tool(kind, path, ran(version, false, Some(MSVC_TARGET)))
    }

    const MSVC_TARGET: &str = "x86_64-pc-windows-msvc";

    fn found<'a>(installs: &'a [VsInstallation], llvm: &'a [LlvmTool]) -> Found<'a> {
        Found {
            installs,
            llvm,
            explicit_msvc: None,
        }
    }

    fn asking(name: &str) -> Choices {
        Choices {
            cli: Some(name.to_owned()),
            ..Default::default()
        }
    }

    fn pick(
        choices: &Choices,
        found: Found<'_>,
        constraints: &Constraints,
    ) -> Result<Selected, ToolchainError> {
        select(choices, found, constraints, HostOs::Windows)
    }

    /// Mutation: swap any two levels of the precedence; this test fails.
    #[test]
    fn each_level_beats_every_level_below_it() {
        let installs = [install("VS", &[v(19, 52, 1)])];
        let llvm = [
            clang(Tool::Clang, "clang", v(22, 1, 8)),
            clang(Tool::ClangCl, "clang-cl", v(22, 1, 8)),
        ];
        // Each level names a different compiler than the level below it;
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
            let selected =
                pick(&choices, found(&installs, &llvm), &Constraints::default()).unwrap();
            if top < levels.len() {
                assert_eq!(selected.source, sources[top]);
                assert_eq!(selected.kind.name(), levels[top]);
                assert_eq!(
                    selected.executable.file_name().unwrap(),
                    if levels[top] == "msvc" {
                        "cl.exe"
                    } else {
                        levels[top]
                    }
                );
            } else {
                assert_eq!(selected.source, Source::Automatic);
                assert_eq!(selected.kind, CompilerKind::Msvc);
            }
        }
    }

    /// Outside a project the project's layers cannot be given (`Choices`
    /// has no place for them), so the user-level choice comes right after
    /// `CUNO_COMPILER`.
    ///
    /// Mutation: drop the user-level layer; this test fails.
    #[test]
    fn outside_a_project_the_user_config_is_next_after_the_environment() {
        let installs = [install("VS", &[v(19, 52, 1)])];
        let llvm = [clang(Tool::Clang, "clang", v(22, 1, 8))];
        let choices = Choices {
            user: Some("clang".to_owned()),
            ..Default::default()
        };
        let selected = pick(&choices, found(&installs, &llvm), &Constraints::default()).unwrap();
        assert_eq!(
            (selected.kind, selected.source),
            (CompilerKind::Clang, Source::UserConfig)
        );
    }

    /// Mutation: change what any source is called, or fall back to
    /// automatic selection for `gcc`, `safec` or an unknown name, or report
    /// another source than the one that named it; this test fails.
    #[test]
    fn unsupported_and_unknown_names_are_errors_naming_their_source() {
        let installs = [install("VS", &[v(19, 52, 1)])];
        let names = [
            (Source::CommandLine, "--compiler"),
            (Source::Environment, "CUNO_COMPILER"),
            (Source::ProjectConfig, "compiler in .cuno/config.toml"),
            (Source::Manifest, "[toolchain] compiler in cuno.toml"),
            (Source::UserConfig, "compiler in the user-level config.toml"),
            (Source::Automatic, "automatic selection"),
        ];
        for (source, text) in names {
            assert_eq!(source.to_string(), text);
        }
        for (choices, text) in [
            (asking("gcc"), "--compiler"),
            (
                Choices {
                    env: Some("safec".into()),
                    ..Default::default()
                },
                "CUNO_COMPILER",
            ),
            (
                Choices {
                    project: Some(ProjectChoices {
                        config: None,
                        manifest: Some("nope".into()),
                    }),
                    ..Default::default()
                },
                "[toolchain] compiler in cuno.toml",
            ),
        ] {
            let err = pick(&choices, found(&installs, &[]), &Constraints::default()).unwrap_err();
            match &err {
                ToolchainError::UnsupportedCompiler { source, .. }
                | ToolchainError::UnknownCompiler { source, .. } => assert_eq!(source, text),
                _ => panic!("{err:?}"),
            }
            assert_eq!(
                matches!(err, ToolchainError::UnknownCompiler { .. }),
                text.contains("cuno.toml")
            );
        }
    }

    /// Mutation: include previews or incomplete installations, take the
    /// oldest, or ignore the upper bound; this test fails.
    #[test]
    fn the_newest_satisfying_release_is_chosen() {
        let mut preview = install("Preview", &[v(19, 44, 1)]);
        preview.prerelease = true;
        let mut incomplete = install("Incomplete", &[v(19, 45, 1)]);
        incomplete.complete = false;
        let installs = [
            install("VS", &[v(19, 40, 1), v(19, 52, 1)]),
            preview,
            incomplete,
        ];
        let constraints = Constraints {
            msvc: Some(VersionReq::parse("msvc", ">=19.29, <19.50").unwrap()),
            ..Default::default()
        };
        let selected = pick(&Choices::default(), found(&installs, &[]), &constraints).unwrap();
        assert_eq!(selected.version, v(19, 40, 1));
        let selected = pick(
            &Choices::default(),
            found(&installs[1..], &[]),
            &Constraints::default(),
        );
        assert!(
            matches!(selected, Err(ToolchainError::NoVisualStudio)),
            "{selected:?}"
        );
    }

    /// Mutation: accept a preview, a MinGW Clang or one that reports no
    /// target, take the last of equal versions, or give Clang another
    /// toolset than MSVC would use; this test fails.
    #[test]
    fn only_msvc_targeting_clang_releases_are_eligible() {
        let installs = [install("A", &[v(19, 52, 1)]), install("B", &[v(19, 52, 1)])];
        let llvm = [
            clang(Tool::Clang, "old", v(18, 1, 0)),
            tool(Tool::Clang, "rc", ran(v(23, 0, 0), true, Some(MSVC_TARGET))),
            tool(
                Tool::Clang,
                "mingw",
                ran(v(22, 5, 0), false, Some("x86_64-w64-windows-gnu")),
            ),
            tool(Tool::Clang, "untargeted", ran(v(22, 4, 0), false, None)),
            clang(Tool::Clang, "new", v(22, 1, 8)),
            clang(Tool::Clang, "same", v(22, 1, 8)),
        ];
        let selected = pick(
            &asking("clang"),
            found(&installs, &llvm),
            &Constraints::default(),
        )
        .unwrap();
        assert_eq!(selected.executable, PathBuf::from("new"));
        let msvc = pick(
            &asking("msvc"),
            found(&installs, &llvm),
            &Constraints::default(),
        )
        .unwrap();
        assert_eq!(selected.msvc, msvc.msvc);
        assert!(msvc.msvc.unwrap().dir.starts_with("A"));
    }

    /// Mutation: report "not found" when copies were found but none is
    /// eligible, or drop why each was left out; this test fails.
    #[test]
    fn ineligible_copies_are_named_with_why() {
        let installs = [install("VS", &[v(19, 52, 1)])];
        let llvm = [
            tool(Tool::Clang, "rc", ran(v(23, 0, 0), true, Some(MSVC_TARGET))),
            tool(
                Tool::Clang,
                "mingw",
                ran(v(22, 5, 0), false, Some("x86_64-w64-windows-gnu")),
            ),
            tool(Tool::Clang, "untargeted", ran(v(22, 4, 0), false, None)),
            tool(Tool::Clang, "broken", Probe::Failed),
            tool(Tool::Clang, "silent", Probe::NoVersion),
        ];
        let err = pick(
            &asking("clang"),
            found(&installs, &llvm),
            &Constraints::default(),
        )
        .unwrap_err();
        let ToolchainError::NoUsableCompiler { excluded, .. } = &err else {
            panic!("{err:?}");
        };
        assert_eq!(
            *excluded,
            [
                "rc (23.0.0 is a preview)",
                "mingw (it targets x86_64-w64-windows-gnu, not MSVC)",
                "untargeted (it reports no target)",
                "broken (it did not run)",
                "silent (it printed no version)",
            ]
        );
        let err = pick(
            &asking("clang"),
            found(&installs, &[]),
            &Constraints::default(),
        )
        .unwrap_err();
        assert!(
            matches!(err, ToolchainError::CompilerNotFound { compiler: "clang" }),
            "{err:?}"
        );
    }

    /// Mutation: change or drop any compiler's minimum, accept below it, or
    /// reject the minimum itself; this test fails.
    #[test]
    fn each_compiler_starts_at_its_minimum() {
        for (kind, below, at) in [
            ("msvc", v(19, 28, 99), v(19, 29, 0)),
            ("clang", v(16, 9, 9), v(17, 0, 0)),
            ("clang-cl", v(16, 9, 9), v(17, 0, 0)),
        ] {
            for (version, ok) in [(below, false), (at, true)] {
                let installs = [install(
                    "VS",
                    &[if kind == "msvc" {
                        version
                    } else {
                        v(19, 52, 1)
                    }],
                )];
                let llvm = [
                    clang(Tool::Clang, "clang", version),
                    clang(Tool::ClangCl, "clang-cl", version),
                ];
                let result = pick(
                    &asking(kind),
                    found(&installs, &llvm),
                    &Constraints::default(),
                );
                match result {
                    Ok(_) => assert!(ok, "{kind} {version} accepted"),
                    Err(ToolchainError::NoMatchingVersion { found, .. }) => {
                        assert!(!ok, "{kind} {version} rejected");
                        assert_eq!(found, [version]);
                    }
                    Err(e) => panic!("{kind} {version}: {e:?}"),
                }
            }
        }
    }

    /// Mutation: apply one compiler's `[toolchain.version]` to another, or
    /// leave the user's requirement out of the error; this test fails.
    #[test]
    fn each_requirement_is_its_own_compilers() {
        let installs = [install("VS", &[v(19, 52, 1)])];
        let llvm = [
            clang(Tool::Clang, "clang-old", v(18, 1, 0)),
            clang(Tool::Clang, "clang-new", v(22, 1, 8)),
            clang(Tool::ClangCl, "cl-old", v(18, 1, 0)),
            clang(Tool::ClangCl, "cl-new", v(22, 1, 8)),
        ];
        let below_22 = || Some(VersionReq::parse("x", "<22").unwrap());
        for (kind, constraints) in [
            (
                "clang",
                Constraints {
                    clang: below_22(),
                    ..Default::default()
                },
            ),
            (
                "clang-cl",
                Constraints {
                    clang_cl: below_22(),
                    ..Default::default()
                },
            ),
            (
                "msvc",
                Constraints {
                    msvc: Some(VersionReq::parse("msvc", "<19.50").unwrap()),
                    ..Default::default()
                },
            ),
        ] {
            let constrained = pick(&asking(kind), found(&installs, &llvm), &constraints);
            let others: Vec<_> = ["clang", "clang-cl"]
                .into_iter()
                .filter(|other| *other != kind)
                .map(|other| pick(&asking(other), found(&installs, &llvm), &constraints).unwrap())
                .collect();
            for other in others {
                assert_eq!(other.version, v(22, 1, 8), "{kind} constrained {other:?}");
            }
            if kind == "msvc" {
                let err = constrained.unwrap_err();
                assert_eq!(
                    err.to_string(),
                    "no msvc found satisfies >=19.29.0 (cuno's minimum) and <19.50 ([toolchain.version] msvc); found 19.52.1"
                );
            } else {
                assert_eq!(constrained.unwrap().version, v(18, 1, 0), "{kind}");
            }
        }
    }

    /// Mutation: choose Clang without a Visual Studio, ignore
    /// `[toolchain.paths] msvc`, or compare it as written rather than as the
    /// same directory; this test fails.
    #[test]
    fn visual_studio_is_required_and_paths_msvc_limits_it() {
        let llvm = [clang(Tool::Clang, "clang", v(22, 1, 8))];
        let err = pick(&asking("clang"), found(&[], &llvm), &Constraints::default()).unwrap_err();
        assert!(matches!(err, ToolchainError::NoVisualStudio), "{err:?}");

        let root = tempfile::tempdir().unwrap();
        for dir in ["A", "B"] {
            std::fs::create_dir(root.path().join(dir)).unwrap();
        }
        let installs = [
            install(root.path().join("A").to_str().unwrap(), &[v(19, 52, 1)]),
            install(root.path().join("B").to_str().unwrap(), &[v(19, 40, 1)]),
        ];
        let with = |dir: &Path| {
            pick(
                &Choices::default(),
                Found {
                    installs: &installs,
                    llvm: &[],
                    explicit_msvc: Some(dir),
                },
                &Constraints::default(),
            )
        };
        let spelled_otherwise = root.path().join("A").join("..").join("B");
        assert_eq!(with(&spelled_otherwise).unwrap().version, v(19, 40, 1));
        let err = with(&root.path().join("C")).unwrap_err();
        assert!(
            matches!(err, ToolchainError::VisualStudioNotAt { .. }),
            "{err:?}"
        );
    }

    /// Mutation: elsewhere than on Windows, select a default other than the
    /// unsupported GCC, accept MSVC, or require Visual Studio or an MSVC
    /// target for Clang; this test fails.
    #[test]
    fn other_hosts_need_no_visual_studio_and_have_no_msvc() {
        let other = |choices: &Choices, llvm: &[LlvmTool]| {
            select(
                choices,
                found(&[], llvm),
                &Constraints::default(),
                HostOs::Other,
            )
        };
        let err = other(&Choices::default(), &[]).unwrap_err();
        assert!(
            matches!(&err, ToolchainError::UnsupportedCompiler { name, source } if name == "gcc" && source == "automatic selection"),
            "{err:?}"
        );
        let err = other(&asking("msvc"), &[]).unwrap_err();
        assert!(matches!(err, ToolchainError::NoVisualStudio), "{err:?}");
        let llvm = [tool(
            Tool::Clang,
            "clang",
            ran(v(22, 1, 8), false, Some("x86_64-pc-linux-gnu")),
        )];
        let selected = other(&asking("clang"), &llvm).unwrap();
        assert_eq!((selected.version, selected.msvc), (v(22, 1, 8), None));
    }

    /// Mutation: compare every component instead of only those written, or
    /// treat `>` as `>=` or `=` as `>=`; this test fails.
    #[test]
    fn requirements_compare_only_the_components_written() {
        let req = |text| VersionReq::parse("msvc", text).unwrap();
        assert!(req("=19.40").matches(&v(19, 40, 33811)));
        assert!(!req("=19.40").matches(&v(19, 41, 0)));
        assert!(!req("=19.40").matches(&v(19, 39, 9)));
        assert!(req(">=17").matches(&v(17, 0, 6)));
        assert!(!req("<19").matches(&v(19, 1, 0)));
        assert!(!req(">19.40").matches(&v(19, 40, 9)));
        assert!(req(">19.40").matches(&v(19, 41, 0)));
        assert!(req("<= 19.40, >=19.29").matches(&v(19, 40, 99)));
        assert_eq!(req(">=17, <19").to_string(), ">=17, <19");
        for bad in ["19", "~19", ">=", ">=a.b", ">=1.2.3.4", ">=17,", ""] {
            assert!(
                matches!(
                    VersionReq::parse("msvc", bad),
                    Err(ToolchainError::InvalidVersionRequirement { .. })
                ),
                "{bad}"
            );
        }
    }

    /// MSVC keeps the toolset of the cl selected, even when
    /// `[toolchain.version]` leaves out a newer one; a Clang gets the newest.
    ///
    /// Mutation: give MSVC the newest toolset instead of its own, or a Clang
    /// none; this test fails.
    #[test]
    fn msvc_keeps_its_own_toolset_and_clang_gets_the_newest() {
        let installs = [install("VS", &[v(19, 44, 1), v(19, 52, 1)])];
        let llvm = [clang(Tool::Clang, "clang", v(22, 1, 8))];
        let older = Constraints {
            msvc: Some(VersionReq::parse("msvc", "<19.50").unwrap()),
            ..Default::default()
        };
        let msvc = pick(&asking("msvc"), found(&installs, &llvm), &older).unwrap();
        assert_eq!(msvc.version, v(19, 44, 1));
        let toolset = msvc.msvc.unwrap();
        assert_eq!(
            (toolset.cl_version, toolset.cl()),
            (v(19, 44, 1), msvc.executable)
        );

        let clang = pick(&asking("clang"), found(&installs, &llvm), &older).unwrap();
        assert_eq!(clang.msvc.unwrap().cl_version, v(19, 52, 1));
    }

    /// `copy_of` answers a Clang without Visual Studio (doctor's `Info`
    /// lines need it when Visual Studio is missing), holds every copy to
    /// cuno's minimum and to the constraints passed, and gives MSVC its own
    /// toolset; `Choices::kind` names the kind asked for even when its
    /// selection would fail.
    ///
    /// Mutation: require Visual Studio for a Clang in `copy_of`, drop cuno's
    /// minimum or the constraints, or make `kind` fail when selection would;
    /// this test fails.
    #[test]
    fn copy_of_is_the_answer_selection_and_doctor_share() {
        let none = Constraints::default();
        let llvm = [clang(Tool::Clang, "found", v(22, 1, 8))];
        let without_vs = found(&[], &llvm);
        assert_eq!(
            copy_of(CompilerKind::Clang, without_vs, &none, HostOs::Windows).unwrap(),
            (PathBuf::from("found"), v(22, 1, 8), None)
        );
        assert!(matches!(
            copy_of(CompilerKind::Msvc, without_vs, &none, HostOs::Windows),
            Err(ToolchainError::NoVisualStudio)
        ));

        let old = [clang(Tool::Clang, "old", v(16, 0, 6))];
        assert!(matches!(
            copy_of(
                CompilerKind::Clang,
                found(&[], &old),
                &none,
                HostOs::Windows
            ),
            Err(ToolchainError::NoMatchingVersion { .. })
        ));
        let required = Constraints {
            clang: Some(VersionReq::parse("clang", ">=30").unwrap()),
            ..Default::default()
        };
        assert!(matches!(
            copy_of(CompilerKind::Clang, without_vs, &required, HostOs::Windows),
            Err(ToolchainError::NoMatchingVersion { .. })
        ));

        let installs = [install("VS", &[v(19, 44, 1), v(19, 52, 1)])];
        let (cl, version, toolset) = copy_of(
            CompilerKind::Msvc,
            found(&installs, &[]),
            &none,
            HostOs::Windows,
        )
        .unwrap();
        assert_eq!(version, v(19, 52, 1));
        assert_eq!(toolset.unwrap().cl(), cl);

        // Asked for clang with no Visual Studio: selection fails, the kind
        // asked for is still clang.
        assert!(pick(&asking("clang"), without_vs, &none).is_err());
        assert_eq!(
            asking("clang").kind(HostOs::Windows).unwrap(),
            (CompilerKind::Clang, Source::CommandLine)
        );
        assert_eq!(
            Choices::default().kind(HostOs::Windows).unwrap(),
            (CompilerKind::Msvc, Source::Automatic)
        );
    }

    /// A configured clang or clang-cl is the only copy selection takes, with
    /// the MSVC toolset every Clang gets: an older one wins over a newer copy
    /// found, and one that cannot be used, or does not satisfy the version
    /// requirements, is an error naming its own setting rather than a reason
    /// to take another. A copy configured for the other driver plays no part.
    ///
    /// Mutation: let a newer copy found win over the configured one, fall
    /// back to another copy when the configured one is unusable, select it
    /// without a toolset, name another setting, or let a configured copy of
    /// the other driver count; this test fails.
    #[test]
    fn a_configured_compiler_is_the_only_one_selected() {
        let installs = [install("VS", &[v(19, 52, 1)])];
        let toolset = installs[0].toolsets[0].clone();
        let none = Constraints::default();
        for (kind, driver, other) in [
            (CompilerKind::Clang, Tool::Clang, Tool::ClangCl),
            (CompilerKind::ClangCl, Tool::ClangCl, Tool::Clang),
        ] {
            let name = kind.name();
            let mine = |probe| LlvmTool {
                origin: Origin::Configured,
                ..tool(driver, "mine", probe)
            };
            let newer = clang(driver, "found", v(22, 1, 8));
            let pick_with = |llvm: &[LlvmTool], constraints: &Constraints| {
                pick(&asking(name), found(&installs, llvm), constraints)
            };

            let older = [
                mine(ran(v(19, 1, 0), false, Some(MSVC_TARGET))),
                newer.clone(),
            ];
            let selected = pick_with(&older, &none).unwrap();
            assert_eq!(
                selected,
                Selected {
                    kind,
                    executable: "mine".into(),
                    version: v(19, 1, 0),
                    msvc: Some(toolset.clone()),
                    source: Source::CommandLine,
                }
            );

            // A broken copy configured for the other driver is not this
            // one's: the newest found is selected.
            let elsewhere = [
                LlvmTool {
                    origin: Origin::Configured,
                    ..tool(other, "theirs", Probe::Failed)
                },
                newer.clone(),
            ];
            assert_eq!(
                pick_with(&elsewhere, &none).unwrap().executable,
                PathBuf::from("found")
            );
            // ... and when the copy found fails the requirements, the error
            // is about it, not about the other driver's setting.
            let mut unmet = Constraints::default();
            let req = Some(VersionReq::parse(name, ">=30").unwrap());
            match kind {
                CompilerKind::Clang => unmet.clang = req,
                _ => unmet.clang_cl = req,
            }
            assert!(
                matches!(
                    pick_with(&elsewhere, &unmet),
                    Err(ToolchainError::NoMatchingVersion { .. })
                ),
                "{kind:?}"
            );

            let mut required = Constraints::default();
            let req = Some(VersionReq::parse(name, ">=20").unwrap());
            match kind {
                CompilerKind::Clang => required.clang = req,
                _ => required.clang_cl = req,
            }
            let cases: [(LlvmTool, &Constraints, String); 4] = [
                (mine(Probe::Failed), &none, "it did not run".into()),
                (
                    mine(ran(v(22, 1, 0), false, Some("x86_64-w64-windows-gnu"))),
                    &none,
                    "it targets x86_64-w64-windows-gnu, not MSVC".into(),
                ),
                (
                    mine(ran(v(16, 0, 0), false, Some(MSVC_TARGET))),
                    &none,
                    "16.0.0 does not satisfy >=17.0.0 (cuno's minimum)".into(),
                ),
                (
                    mine(ran(v(19, 1, 0), false, Some(MSVC_TARGET))),
                    &required,
                    format!(
                        "19.1.0 does not satisfy >=17.0.0 (cuno's minimum) and >=20 ([toolchain.version] {name})"
                    ),
                ),
            ];
            for (configured, constraints, why) in cases {
                let err = pick_with(&[configured, newer.clone()], constraints).unwrap_err();
                assert_eq!(
                    err.to_string(),
                    format!("[toolchain.paths] {name} is mine, but {why}")
                );
            }
        }
    }

    /// clang-format and clang-tidy below 17 are not used: a found one is
    /// passed over for a newer, and a configured one is an error saying so
    /// (`dev-tools.md`, "Tool versions").
    ///
    /// Mutation: drop the minimum, or apply it to one tool only; this test
    /// fails.
    #[test]
    fn llvm_tools_below_17_are_not_used() {
        for kind in [Tool::ClangFormat, Tool::ClangTidy] {
            let old = tool(kind, "old", ran(v(16, 0, 6), false, None));
            let new = tool(kind, "new", ran(v(17, 0, 1), false, None));
            assert_eq!(
                select_tool(kind, &[old.clone(), new], HostOs::Windows)
                    .unwrap()
                    .path,
                PathBuf::from("new")
            );
            assert!(select_tool(kind, std::slice::from_ref(&old), HostOs::Windows).is_err());
            let configured = LlvmTool {
                origin: Origin::Configured,
                ..old
            };
            let err = select_tool(kind, &[configured], HostOs::Windows).unwrap_err();
            assert!(
                err.to_string()
                    .ends_with("16.0.6 does not satisfy >=17.0.0 (cuno's minimum)"),
                "{err}"
            );
        }
    }

    /// A configured copy is the only one used, even when older, and is an
    /// error naming the setting when it cannot be used; a Clang must target
    /// MSVC.
    ///
    /// Mutation: let a newer found copy win over the configured one, fall
    /// back to another copy when the configured one did not run, or drop the
    /// MSVC-target rule for clang; this test fails.
    #[test]
    fn a_configured_tool_is_used_or_is_an_error() {
        let configured = |probe| LlvmTool {
            origin: Origin::Configured,
            ..tool(Tool::ClangTidy, "mine", probe)
        };
        let newer = tool(Tool::ClangTidy, "found", ran(v(22, 1, 8), false, None));
        let llvm = [configured(ran(v(19, 1, 0), false, None)), newer.clone()];
        assert_eq!(
            select_tool(Tool::ClangTidy, &llvm, HostOs::Windows).unwrap(),
            LlvmChoice {
                path: "mine".into(),
                version: v(19, 1, 0)
            }
        );
        let llvm = [configured(Probe::Failed), newer];
        let err = select_tool(Tool::ClangTidy, &llvm, HostOs::Windows).unwrap_err();
        assert_eq!(
            err.to_string(),
            "[toolchain.paths] clang-tidy is mine, but it did not run"
        );
        assert_eq!(
            cuno_core::CunoError::help(&err).unwrap(),
            "correct [toolchain.paths] clang-tidy, or remove it to let cuno find the tool"
        );

        let clangs = [
            tool(
                Tool::Clang,
                "mingw",
                ran(v(23, 1, 0), false, Some("x86_64-w64-windows-gnu")),
            ),
            clang(Tool::Clang, "msvc", v(22, 1, 8)),
        ];
        assert_eq!(
            select_tool(Tool::Clang, &clangs, HostOs::Windows)
                .unwrap()
                .path,
            PathBuf::from("msvc")
        );
    }

    /// Mutation: pick the first copy rather than the newest, accept a preview
    /// or a copy that did not run, demand an MSVC target of clang-tidy (it
    /// prints none), or say "not found" when copies were left out; this test
    /// fails.
    #[test]
    fn the_newest_release_of_a_tool_is_used() {
        let llvm = [
            tool(Tool::ClangTidy, "old", ran(v(19, 1, 0), false, None)),
            tool(Tool::ClangTidy, "new", ran(v(22, 1, 8), false, None)),
            tool(Tool::ClangTidy, "preview", ran(v(23, 0, 0), true, None)),
            tool(Tool::ClangTidy, "broken", Probe::Failed),
            tool(Tool::ClangFormat, "format", ran(v(18, 1, 0), false, None)),
        ];
        assert_eq!(
            select_tool(Tool::ClangTidy, &llvm, HostOs::Windows).unwrap(),
            LlvmChoice {
                path: "new".into(),
                version: v(22, 1, 8)
            }
        );
        assert!(matches!(
            select_tool(Tool::Clang, &llvm, HostOs::Windows),
            Err(ToolchainError::CompilerNotFound { compiler: "clang" })
        ));
        let unusable = [
            tool(Tool::ClangFormat, "preview", ran(v(23, 0, 0), true, None)),
            tool(Tool::ClangFormat, "silent", Probe::NoVersion),
            tool(Tool::ClangFormat, "broken", Probe::Failed),
        ];
        let err = select_tool(Tool::ClangFormat, &unusable, HostOs::Windows).unwrap_err();
        assert_eq!(
            err.to_string(),
            "no usable clang-format was found; left out: preview (23.0.0 is a preview); \
             silent (it printed no version); broken (it did not run)"
        );
    }
}
