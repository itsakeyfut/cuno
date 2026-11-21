//! Choosing the compiler: which one the user asked for, which installed copy
//! satisfies the version requirements, and whether a Clang is accepted by the
//! MSVC standard library. See `docs/en/specs/toolchain.md`, "Selection",
//! "Version pinning" and "Minimum compiler versions".

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use cuno_core::Env;

use crate::llvm::{LlvmTool, Origin, Probe, Tool};
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
    let mut requirements = vec![format!(">={} (cuno's minimum)", kind.minimum())];
    if let Some(r) = requirement {
        requirements.push(format!("{r} ([toolchain.version] {})", kind.name()));
    }

    // On Windows every compiler uses an MSVC toolset (`toolchain.md`,
    // "Environment resolution on Windows"), so Visual Studio is required
    // even for Clang.
    let toolsets = match host {
        HostOs::Windows => usable_toolsets(found)?,
        HostOs::Other => Vec::new(),
    };
    let newest_toolset = first_newest(toolsets.iter().map(|t| (t, t.cl_version))).cloned();

    let mut candidates: Vec<(PathBuf, Version, Option<MsvcToolset>)> = Vec::new();
    // Copies found but not eligible, with why, for the error.
    let mut excluded: Vec<String> = Vec::new();
    match kind {
        CompilerKind::Msvc => {
            if host != HostOs::Windows {
                return Err(ToolchainError::NoVisualStudio);
            }
            candidates.extend(
                toolsets
                    .iter()
                    .map(|t| (t.cl(), t.cl_version, Some(t.clone()))),
            );
        }
        CompilerKind::Clang | CompilerKind::ClangCl => {
            let tool = if kind == CompilerKind::Clang {
                Tool::Clang
            } else {
                Tool::ClangCl
            };
            let configured = found
                .llvm
                .iter()
                .any(|t| t.tool == tool && t.origin == Origin::Configured);
            if configured {
                // The configured copy is the only one used (`toolchain.md`,
                // "LLVM"): one that cannot be used is an error, never a
                // reason to take another.
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
                candidates.push((chosen.path, chosen.version, newest_toolset.clone()));
            } else {
                for t in found.llvm.iter().filter(|t| t.tool == tool) {
                    match release_for(t, host) {
                        Ok(version) => {
                            candidates.push((t.path.clone(), version, newest_toolset.clone()))
                        }
                        Err(why) => excluded.push(format!("{} ({why})", t.path.display())),
                    }
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
    let Some((executable, version, msvc)) = best.cloned() else {
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

/// The version of a tool that ran, is a release, and on Windows targets MSVC
/// (a MinGW Clang cannot be linked with MSVC; `toolchain.md`, "LLVM"), or why
/// it is not eligible.
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

/// What the build's compile command would also get, for the standard library
/// probe.
#[derive(Clone, Copy, Debug)]
pub struct ProbeInputs<'a> {
    /// A directory outside the project to write the probe file in.
    pub scratch: &'a Path,
    /// The build's compile flags in this compiler's syntax, except those
    /// naming inputs and outputs: the language standard, RTTI and exceptions,
    /// include directories, defines and the user's `flags`, as
    /// [`crate::flags::compile_flags`] gives them for a C++ input: the probe
    /// compiles C++, so a C package's are built with `Lang::Cxx` and a C++
    /// edition, or a C standard switch would be rejected. Paths must be
    /// absolute: the probe does not run in the project directory.
    pub flags: &'a [OsString],
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
    let switches: &[&str] = match selected.kind {
        CompilerKind::ClangCl => &["/nologo", "/TP", "/Zs"],
        _ => &["-x", "c++", "-fsyntax-only"],
    };
    stl_probe(
        selected.kind.name(),
        selected.version,
        inputs.scratch,
        |probe| {
            let mut args: Vec<OsString> = switches.iter().map(OsString::from).collect();
            args.push("--target=x86_64-pc-windows-msvc".into());
            args.extend(inputs.flags.iter().cloned());
            args.push(probe.into());
            run.run(&selected.executable, &as_args(&args), env)
        },
    )
}

/// Whether `tidy` is accepted by the MSVC standard library: the same probe
/// as [`check`], analysed by clang-tidy with one harmless check enabled
/// (with none it stops without analysing) and, after `--`, the build's
/// compile flags for `kind`, in `kind`'s syntax (`toolchain.md`, "Minimum
/// compiler versions"). The project's `.clang-tidy` is not read.
pub fn check_tidy(
    tidy: &LlvmChoice,
    kind: CompilerKind,
    env: &Env,
    inputs: ProbeInputs<'_>,
    run: &dyn Runner,
) -> Result<(), ToolchainError> {
    stl_probe("clang-tidy", tidy.version, inputs.scratch, |probe| {
        let mut args: Vec<OsString> = vec![
            "--checks=-*,readability-braces-around-statements".into(),
            "--config={}".into(),
            probe.into(),
            "--".into(),
        ];
        if kind != CompilerKind::Clang {
            // clang-tidy reads flags as the GNU-style driver unless told.
            args.push("--driver-mode=cl".into());
        }
        args.push("--target=x86_64-pc-windows-msvc".into());
        args.extend(inputs.flags.iter().cloned());
        run.run(&tidy.path, &as_args(&args), env)
    })
}

fn as_args(args: &[OsString]) -> Vec<&OsStr> {
    args.iter().map(|a| a.as_os_str()).collect()
}

/// Writes the probe file to `scratch`, runs `invoke` on it, and reads the
/// standard library's verdict: `STL1000:` on stdout (clang-tidy) or stderr
/// (the compilers) is a rejection; any other failure decides nothing.
fn stl_probe(
    name: &'static str,
    version: Version,
    scratch: &Path,
    invoke: impl FnOnce(&Path) -> std::io::Result<crate::Output>,
) -> Result<(), ToolchainError> {
    let failed = |output: String| ToolchainError::ProbeFailed {
        compiler: name,
        output,
    };
    // A name of its own, so that two cuno processes sharing `scratch` do not
    // write or remove each other's file.
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let probe = scratch.join(format!(
        "cuno-stl-probe-{}-{}.cpp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&probe, "#include <yvals_core.h>\n")
        .map_err(|e| failed(format!("could not write {}: {e}", probe.display())))?;
    let output = invoke(&probe);
    let _ = std::fs::remove_file(&probe);
    let output = output.map_err(|e| failed(e.to_string()))?;
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
            compiler: name,
            version,
            needed: needed_clang(line),
            message: line.trim().to_owned(),
        }),
        None => Err(failed(text.trim().to_owned())),
    }
}

/// A clang-format or clang-tidy to use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LlvmChoice {
    /// The executable.
    pub path: PathBuf,
    /// Its version.
    pub version: Version,
}

/// The copy of `tool` to use among those detected (`toolchain.md`, "LLVM"):
/// the one written in `[toolchain.paths]` when there is one, which must be
/// eligible, or else the newest eligible copy; among equals, the first in
/// search order. Eligible is a release that ran, and for clang and clang-cl
/// on Windows one targeting MSVC; clang-format and clang-tidy have no target
/// to check (clang-tidy prints none). No version requirement applies here.
pub fn select_tool(
    tool: Tool,
    llvm: &[LlvmTool],
    host: HostOs,
) -> Result<LlvmChoice, ToolchainError> {
    let eligible = |t: &LlvmTool| match tool {
        Tool::Clang | Tool::ClangCl => release_for(t, host),
        Tool::ClangFormat | Tool::ClangTidy => release(t),
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
    use std::cell::RefCell;

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

    /// A runner that answers the probe and records the arguments.
    struct Answers {
        ok: bool,
        stdout: &'static str,
        stderr: &'static str,
        args: RefCell<Vec<String>>,
        calls: RefCell<u32>,
    }

    fn answers(ok: bool, stdout: &'static str, stderr: &'static str) -> Answers {
        Answers {
            ok,
            stdout,
            stderr,
            args: Default::default(),
            calls: Default::default(),
        }
    }

    impl Runner for Answers {
        fn run(&self, _p: &Path, args: &[&OsStr], _e: &Env) -> std::io::Result<Output> {
            *self.calls.borrow_mut() += 1;
            *self.args.borrow_mut() = args
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            Ok(Output {
                success: self.ok,
                stdout: self.stdout.as_bytes().to_vec(),
                stderr: self.stderr.as_bytes().to_vec(),
            })
        }
    }

    struct CannotStart;
    impl Runner for CannotStart {
        fn run(&self, _p: &Path, _a: &[&OsStr], _e: &Env) -> std::io::Result<Output> {
            Err(std::io::Error::other("cannot start"))
        }
    }

    /// A runner whose tool runs past the limit, as `SystemRunner` reports it.
    struct TimesOut;
    impl Runner for TimesOut {
        fn run(&self, _p: &Path, _a: &[&OsStr], _e: &Env) -> std::io::Result<Output> {
            Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "did not finish within 30 seconds and was stopped",
            ))
        }
    }

    fn selected(kind: CompilerKind) -> Selected {
        Selected {
            kind,
            executable: "clang".into(),
            version: v(20, 1, 8),
            msvc: Some(install("VS", &[v(19, 52, 1)]).toolsets.remove(0)),
            source: Source::CommandLine,
        }
    }

    /// Mutation: treat a failed or timed-out probe as passing, call any
    /// failure STL1000,
    /// read only stdout, lose the version the library asks for, the
    /// compiler, its version or the library's message, keep the probe file,
    /// or swallow why it could not run; this test fails.
    #[test]
    fn the_probe_verdict_follows_the_standard_library() {
        let root = tempfile::tempdir().unwrap();
        let inputs = ProbeInputs {
            scratch: root.path(),
            flags: &[],
        };
        let clang = selected(CompilerKind::Clang);
        let stl = "x\\yvals_core.h(922,1): error: STL1000: Unexpected compiler version, expected Clang 22 or newer.\n";
        let err = check(&clang, &Env::default(), inputs, &answers(false, "", stl)).unwrap_err();
        assert_eq!(
            err.to_string(),
            "clang 20.1.8 is not accepted by the MSVC standard library in use (STL1000)"
        );
        let ToolchainError::RejectedByStl {
            needed, message, ..
        } = &err
        else {
            panic!("{err:?}");
        };
        assert_eq!(
            (needed.as_deref(), message.as_str()),
            (Some("22"), stl.trim())
        );

        let err = check(
            &clang,
            &Env::default(),
            inputs,
            &answers(false, "fatal error: 'yvals_core.h' file not found", ""),
        )
        .unwrap_err();
        assert!(
            matches!(&err, ToolchainError::ProbeFailed { compiler: "clang", output } if output == "fatal error: 'yvals_core.h' file not found"),
            "{err:?}"
        );
        let err = check(&clang, &Env::default(), inputs, &TimesOut).unwrap_err();
        assert!(
            matches!(&err, ToolchainError::ProbeFailed { output, .. } if output.contains("within 30 seconds")),
            "{err:?}"
        );
        let err = check(&clang, &Env::default(), inputs, &CannotStart).unwrap_err();
        assert!(
            matches!(&err, ToolchainError::ProbeFailed { output, .. } if output == "cannot start"),
            "{err:?}"
        );
        check(&clang, &Env::default(), inputs, &answers(true, "", "")).unwrap();
        assert_eq!(
            std::fs::read_dir(root.path()).unwrap().count(),
            0,
            "probe file left behind"
        );

        assert_eq!(
            needed_clang("expected Clang 17.0.0 or newer."),
            Some("17.0.0".into())
        );
        assert_eq!(needed_clang("expected Clang  or newer."), None);
        assert_eq!(needed_clang("STL1000: Unexpected compiler version"), None);
    }

    /// Mutation: probe as C, without the target, without the build's flags
    /// or with the other driver's mode, probe MSVC or a Clang without an
    /// MSVC toolset, or name every probe file alike; this test fails.
    #[test]
    fn the_probe_compiles_as_cpp_under_the_builds_flags() {
        let root = tempfile::tempdir().unwrap();
        let flags: Vec<OsString> = vec!["-fno-rtti".into(), "-D_HAS_STATIC_RTTI=0".into()];
        let inputs = ProbeInputs {
            scratch: root.path(),
            flags: &flags,
        };
        let mut probes = Vec::new();
        for (kind, mode) in [
            (CompilerKind::Clang, ["-x", "c++", "-fsyntax-only"]),
            (CompilerKind::ClangCl, ["/nologo", "/TP", "/Zs"]),
        ] {
            let run = answers(true, "", "");
            check(&selected(kind), &Env::default(), inputs, &run).unwrap();
            let args = run.args.borrow().clone();
            let (probe, rest) = args.split_last().unwrap();
            assert_eq!(
                rest,
                [
                    &mode[..],
                    &[
                        "--target=x86_64-pc-windows-msvc",
                        "-fno-rtti",
                        "-D_HAS_STATIC_RTTI=0"
                    ]
                ]
                .concat(),
                "{kind:?}"
            );
            assert!(probe.ends_with(".cpp"), "{probe}");
            probes.push(probe.clone());
        }
        assert_ne!(probes[0], probes[1]);

        let never = answers(false, "STL1000:", "");
        check(
            &selected(CompilerKind::Msvc),
            &Env::default(),
            inputs,
            &never,
        )
        .unwrap();
        let mut elsewhere = selected(CompilerKind::Clang);
        elsewhere.msvc = None;
        check(&elsewhere, &Env::default(), inputs, &never).unwrap();
        assert_eq!(*never.calls.borrow(), 0);
    }

    /// A configured clang or clang-cl is the only copy selection takes: an
    /// older one wins over a newer copy found, and one that cannot be used,
    /// or does not satisfy the version requirements, is an error naming the
    /// setting rather than a reason to take another.
    ///
    /// Mutation: let a newer copy found win over the configured one, fall
    /// back to another copy when the configured one is unusable or does not
    /// satisfy the requirements, or drop the MSVC-target rule for it; this
    /// test fails.
    #[test]
    fn a_configured_compiler_is_the_only_one_selected() {
        let installs = [install("VS", &[v(19, 52, 1)])];
        let mine = |probe| LlvmTool {
            origin: Origin::Configured,
            ..tool(Tool::Clang, "mine", probe)
        };
        let newer = clang(Tool::Clang, "found", v(22, 1, 8));
        let pick_with = |llvm: &[LlvmTool], constraints: &Constraints| {
            pick(&asking("clang"), found(&installs, llvm), constraints)
        };
        let none = Constraints::default();

        let older = [
            mine(ran(v(19, 1, 0), false, Some(MSVC_TARGET))),
            newer.clone(),
        ];
        let selected = pick_with(&older, &none).unwrap();
        assert_eq!(
            (selected.executable, selected.version),
            (PathBuf::from("mine"), v(19, 1, 0))
        );

        let cases: [(LlvmTool, Constraints, &str); 4] = [
            (mine(Probe::Failed), none.clone(), "it did not run"),
            (
                mine(ran(v(22, 1, 0), false, Some("x86_64-w64-windows-gnu"))),
                none.clone(),
                "it targets x86_64-w64-windows-gnu, not MSVC",
            ),
            (
                mine(ran(v(16, 0, 0), false, Some(MSVC_TARGET))),
                none.clone(),
                "16.0.0 does not satisfy >=17.0.0 (cuno's minimum)",
            ),
            (
                mine(ran(v(19, 1, 0), false, Some(MSVC_TARGET))),
                Constraints {
                    clang: Some(VersionReq::parse("clang", ">=20").unwrap()),
                    ..Default::default()
                },
                "19.1.0 does not satisfy >=17.0.0 (cuno's minimum) and >=20 ([toolchain.version] clang)",
            ),
        ];
        for (configured, constraints, why) in cases {
            let err = pick_with(&[configured, newer.clone()], &constraints).unwrap_err();
            assert_eq!(
                err.to_string(),
                format!("[toolchain.paths] clang is mine, but {why}")
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

    /// Mutation: read only stderr (clang-tidy reports STL1000 on stdout),
    /// name the compiler instead of clang-tidy, leave out the one check or
    /// `--`, give the build's flags before `--`, or put cl-style flags
    /// through clang-tidy without `--driver-mode=cl` (or GNU-style ones
    /// with it); this test fails.
    #[test]
    fn clang_tidy_is_asked_the_standard_librarys_verdict() {
        let root = tempfile::tempdir().unwrap();
        let flags: Vec<OsString> = vec!["/GR-".into()];
        let inputs = ProbeInputs {
            scratch: root.path(),
            flags: &flags,
        };
        let tidy = LlvmChoice {
            path: "clang-tidy".into(),
            version: v(20, 1, 8),
        };
        let stl = "x\\yvals_core.h(922,1): error: STL1000: Unexpected compiler version, expected Clang 22 or newer. [clang-diagnostic-error]\n";
        let err = check_tidy(
            &tidy,
            CompilerKind::Msvc,
            &Env::default(),
            inputs,
            &answers(false, stl, ""),
        )
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "clang-tidy 20.1.8 is not accepted by the MSVC standard library in use (STL1000)"
        );
        assert_eq!(
            cuno_core::CunoError::help(&err).unwrap(),
            "install clang-tidy 22 or newer"
        );

        for (kind, mode) in [
            (CompilerKind::Msvc, &["--driver-mode=cl"][..]),
            (CompilerKind::ClangCl, &["--driver-mode=cl"][..]),
            (CompilerKind::Clang, &[][..]),
        ] {
            let run = answers(true, "", "");
            check_tidy(&tidy, kind, &Env::default(), inputs, &run).unwrap();
            let args = run.args.borrow().clone();
            assert_eq!(args[0], "--checks=-*,readability-braces-around-statements");
            assert!(args[2].ends_with(".cpp"), "{args:?}");
            assert_eq!(
                args[3..],
                [
                    &["--"][..],
                    mode,
                    &["--target=x86_64-pc-windows-msvc", "/GR-"]
                ]
                .concat(),
                "{kind:?}"
            );
        }
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }
}
