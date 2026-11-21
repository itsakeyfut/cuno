//! `cuno doctor`: diagnoses the selected compiler and reports what is wrong
//! and how to fix it, without stopping at the first problem. See
//! `docs/en/specs/commands.md`, "`cuno doctor`".

use std::ffi::OsString;
use std::path::Path;

use cuno_core::{CunoError, Env, Shell};
use cuno_manifest::{CliSettings, CompilerName, Inputs, Lang, Os, Profile, Settings};
use cuno_toolchain::flags::{CompileInput, Edition, Mode, check_edition, compile_flags};
use cuno_toolchain::llvm::Tool;
use cuno_toolchain::msvc::{self, SystemRegistry};
use cuno_toolchain::select::{self, Choices, CompilerKind, Found, ProbeInputs};
use cuno_toolchain::{HostOs, SystemRunner, ToolchainError};

/// Prints `error:` and `help:` and counts problems.
struct Problems<'a> {
    shell: &'a mut Shell,
    count: usize,
}

impl Problems<'_> {
    fn report(&mut self, error: &dyn CunoError) {
        self.shell.error(error);
        if let Some(help) = error.help() {
            self.shell.help(help);
        }
        self.count += 1;
    }
}

/// Diagnoses the compiler `--compiler` (or the selection precedence) picks,
/// printing each finding to `shell`, and returns how many problems it found.
pub fn doctor(
    compiler: Option<String>,
    env: &Env,
    cwd: &Path,
    host: HostOs,
    shell: &mut Shell,
) -> usize {
    let mut problems = Problems { shell, count: 0 };
    let runner = SystemRunner::new();

    let cli = CliSettings {
        compiler: compiler.clone(),
        ..Default::default()
    };
    let settings = match cuno_manifest::resolve(Inputs {
        cwd,
        env,
        cli: &cli,
    }) {
        Ok(loaded) => {
            for warning in &loaded.warnings {
                problems.shell.warning(warning);
            }
            Some(loaded.value)
        }
        Err(error) => {
            problems.report(&error);
            None
        }
    };
    let settings = settings.as_ref();

    let mut no_visual_studio = false;
    let installs = if host == HostOs::Windows {
        msvc::find_installations(env, &runner).unwrap_or_else(|error| {
            problems.report(&error);
            no_visual_studio = true;
            Vec::new()
        })
    } else {
        Vec::new()
    };
    let explicit = settings.map(Settings::llvm_paths).unwrap_or_default();
    let mut no_llvm = false;
    let llvm = cuno_toolchain::llvm::find_llvm_tools(env, host, &explicit, &installs, &runner)
        .unwrap_or_else(|error| {
            problems.report(&error);
            no_llvm = true;
            Vec::new()
        });
    let found = Found {
        installs: &installs,
        llvm: &llvm,
        explicit_msvc: settings.and_then(Settings::msvc_path),
    };

    let diagnosed = diagnose(
        settings,
        compiler,
        found,
        no_visual_studio,
        env,
        host,
        &runner,
        &mut problems,
    );
    // A configured path with no file is already reported; every tool
    // missing after it would only repeat it.
    if !no_llvm {
        tools(&llvm, diagnosed.stl.as_ref(), &runner, &mut problems);
    }
    others(diagnosed.selected, found, host, &mut problems);

    let count = problems.count;
    problems.shell.status("Finished", finished(count));
    count
}

/// The `Finished` line's message for `count` problems.
fn finished(count: usize) -> String {
    match count {
        0 => "no problems found".to_owned(),
        1 => "1 problem found".to_owned(),
        n => format!("{n} problems found"),
    }
}

/// What the selected compiler's diagnosis leaves for the rest.
#[derive(Default)]
struct Diagnosed {
    /// The compiler selected, if one was.
    selected: Option<CompilerKind>,
    /// What clang-tidy's standard library check needs, when it is to be
    /// made: a compiler, a Windows environment and a C++ project or none.
    stl: Option<StlInputs>,
}

/// The build's environment and dev-profile compile flags for a C++ input,
/// in the selected compiler's syntax.
struct StlInputs {
    kind: CompilerKind,
    env: Env,
    flags: Vec<OsString>,
}

#[allow(clippy::too_many_arguments)]
fn diagnose(
    settings: Option<&Settings>,
    compiler: Option<String>,
    found: Found<'_>,
    no_visual_studio: bool,
    env: &Env,
    host: HostOs,
    runner: &SystemRunner,
    problems: &mut Problems<'_>,
) -> Diagnosed {
    // A configuration that cannot be read leaves only what does not come
    // from a file: --compiler, CUNO_COMPILER and the defaults.
    let choices = settings
        .map(|s| s.compiler.clone())
        .unwrap_or_else(|| Choices {
            cli: compiler,
            env: env
                .get("CUNO_COMPILER")
                .filter(|v| !v.is_empty())
                .map(|v| v.to_string_lossy().into_owned()),
            project: None,
            user: None,
        });

    let constraints = settings.map(|s| s.constraints.clone()).unwrap_or_default();
    let selected = match select::select(&choices, found, &constraints, host) {
        Ok(selected) => selected,
        // Already reported: no Visual Studio is what vswhere's failure means.
        Err(ToolchainError::NoVisualStudio) if no_visual_studio => return Diagnosed::default(),
        Err(error) => {
            problems.report(&error);
            return Diagnosed::default();
        }
    };
    let mut diagnosed = Diagnosed {
        selected: Some(selected.kind),
        stl: None,
    };
    // Without a readable configuration, automatic selection is not what the
    // configuration chose (commands.md, "`cuno doctor`").
    let source = match (selected.source, settings) {
        (select::Source::Automatic, None) => {
            "automatic selection; the configuration could not be read".to_owned()
        }
        (source, _) => source.to_string(),
    };
    problems.shell.status(
        "Selected",
        format!("{} {} ({source})", selected.kind.name(), selected.version),
    );
    problems.shell.status("", selected.executable.display());

    let project = settings.and_then(|s| s.project.as_ref());
    if let Some((_, manifest)) = project
        && let Err(error) = check_edition(selected.kind, selected.version, manifest.package.edition)
    {
        problems.report(&error);
    }

    let Some(toolset) = &selected.msvc else {
        return diagnosed;
    };
    if selected.kind != CompilerKind::Msvc {
        // <VS>\VC\Tools\MSVC\<version>
        let install = toolset.dir.ancestors().nth(4).unwrap_or(&toolset.dir);
        problems.shell.status(
            "Using",
            format!("MSVC {} ({})", toolset.cl_version, install.display()),
        );
    }
    let sdk = match msvc::find_windows_sdk(env, &SystemRegistry) {
        Ok(sdk) => sdk,
        Err(error) => {
            problems.report(&error);
            return diagnosed;
        }
    };
    problems
        .shell
        .status("Using", format!("Windows SDK {}", sdk.version));

    // The standard library probe, in C++ projects and outside a project
    // (toolchain.md, "Minimum compiler versions").
    let c_project = project.is_some_and(|(_, m)| m.package.lang == Lang::C);
    if c_project {
        return diagnosed;
    }
    let flags = match probe_flags(selected.kind, project.map(|(p, m)| (p.root.as_path(), m))) {
        Ok(flags) => flags,
        Err(error) => {
            problems.report(&*error);
            return diagnosed;
        }
    };
    let stl = StlInputs {
        kind: selected.kind,
        env: msvc::resolve(toolset, &sdk).apply(env),
        flags,
    };
    if selected.kind != CompilerKind::Msvc {
        let result = in_scratch(|scratch| {
            select::check(
                &selected,
                &stl.env,
                ProbeInputs {
                    scratch,
                    flags: &stl.flags,
                },
                runner,
            )
        });
        match result {
            Ok(()) => problems
                .shell
                .status("Accepted", "by the MSVC standard library"),
            Err(error) => problems.report(&*error),
        }
    }
    diagnosed.stl = Some(stl);
    diagnosed
}

/// `Found` lines for clang-format and clang-tidy, a problem for each that is
/// missing, and clang-tidy's standard library check when `stl` is given.
fn tools(
    llvm: &[cuno_toolchain::llvm::LlvmTool],
    stl: Option<&StlInputs>,
    runner: &SystemRunner,
    problems: &mut Problems<'_>,
) {
    for tool in [Tool::ClangFormat, Tool::ClangTidy] {
        let chosen = match select::select_tool(tool, llvm) {
            Ok(chosen) => chosen,
            Err(error) => {
                problems.report(&error);
                continue;
            }
        };
        problems
            .shell
            .status("Found", format!("{} {}", tool.name(), chosen.version));
        if tool == Tool::ClangTidy
            && let Some(stl) = stl
            && let Err(error) = in_scratch(|scratch| {
                select::check_tidy(
                    &chosen,
                    stl.kind,
                    &stl.env,
                    ProbeInputs {
                        scratch,
                        flags: &stl.flags,
                    },
                    runner,
                )
            })
        {
            problems.report(&*error);
        }
    }
}

/// An `Info` line for each compiler that was not selected: its version, or
/// that none was found. Never a problem. Version requirements are not
/// applied: this says what is installed.
fn others(
    selected: Option<CompilerKind>,
    found: Found<'_>,
    host: HostOs,
    problems: &mut Problems<'_>,
) {
    for kind in [
        CompilerKind::Msvc,
        CompilerKind::Clang,
        CompilerKind::ClangCl,
    ] {
        if Some(kind) == selected {
            continue;
        }
        // A Clang is reported by the copy that runs, whether or not Visual
        // Studio (which selecting it needs) was found.
        let version = match kind {
            CompilerKind::Msvc => {
                let choices = Choices {
                    cli: Some(kind.name().to_owned()),
                    ..Default::default()
                };
                select::select(&choices, found, &Default::default(), host)
                    .ok()
                    .map(|s| s.version)
            }
            CompilerKind::Clang => select::select_tool(Tool::Clang, found.llvm)
                .ok()
                .map(|t| t.version),
            CompilerKind::ClangCl => select::select_tool(Tool::ClangCl, found.llvm)
                .ok()
                .map(|t| t.version),
        };
        let line = match version {
            Some(version) => format!("{} {version} (not selected)", kind.name()),
            None => format!("{} not found", kind.name()),
        };
        problems.shell.status("Info", line);
    }
}

/// The build's dev-profile compile flags for a C++ input, in `kind`'s
/// syntax: the project's settings for the host and `kind`, or the defaults
/// outside a project, with the project's C++ edition (C++20 for a C one).
fn probe_flags(
    kind: CompilerKind,
    project: Option<(&Path, &cuno_manifest::Manifest)>,
) -> Result<Vec<OsString>, Box<dyn CunoError>> {
    let name = match kind {
        CompilerKind::Msvc => CompilerName::Msvc,
        CompilerKind::Clang => CompilerName::Clang,
        CompilerKind::ClangCl => CompilerName::ClangCl,
    };
    let default_manifest;
    let (root, manifest) = match project {
        Some((root, manifest)) => (Some(root), manifest),
        None => {
            default_manifest = cuno_manifest::parse_manifest(
                "[package]\nname = \"doctor\"\nversion = \"0.0.0\"\n",
                Path::new("cuno.toml"),
                Path::new("."),
            )
            .map_err(|e| Box::new(e) as Box<dyn CunoError>)?
            .value;
            (None, &default_manifest)
        }
    };
    let build = manifest.build_settings(Os::Windows, name, Profile::Dev);
    let profile = manifest.profile_settings(Profile::Dev);
    let edition = match manifest.package.edition {
        e @ (Edition::Cxx17 | Edition::Cxx20 | Edition::Cxx23) => e,
        _ => Edition::Cxx20,
    };
    let include_dirs: Vec<_> = root.map(|r| r.join("src")).into_iter().collect();
    Ok(compile_flags(&CompileInput {
        kind,
        lang: Lang::Cxx,
        edition,
        build: &build,
        profile: &profile,
        include_dirs: &include_dirs,
        test: false,
        mode: Mode::Check,
    }))
}

/// Runs `check` with a scratch directory of this process's own in the
/// temporary directory, removed afterwards.
fn in_scratch(
    check: impl FnOnce(&Path) -> Result<(), ToolchainError>,
) -> Result<(), Box<dyn CunoError>> {
    // `create_dir`, not `create_dir_all`: a temporary directory that does not
    // exist is a problem to report, not a tree to make.
    let scratch = std::env::temp_dir().join(format!("cuno-doctor-{}", std::process::id()));
    std::fs::create_dir(&scratch).map_err(|e| {
        Box::new(ScratchError {
            dir: scratch.clone(),
            reason: e.to_string(),
        }) as Box<dyn CunoError>
    })?;
    let result = check(&scratch);
    let _ = std::fs::remove_dir_all(&scratch);
    result.map_err(|e| Box::new(e) as Box<dyn CunoError>)
}

/// The probe's scratch directory cannot be made in the temporary
/// directory (`TEMP` names one that does not exist or cannot be written).
#[derive(Debug)]
struct ScratchError {
    dir: std::path::PathBuf,
    reason: String,
}

impl std::fmt::Display for ScratchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "cannot create {} to check the compiler: {}",
            self.dir.display(),
            self.reason
        )
    }
}

impl std::error::Error for ScratchError {}

impl CunoError for ScratchError {
    fn exit_class(&self) -> cuno_core::ExitClass {
        cuno_core::ExitClass::Setup
    }
    fn help(&self) -> Option<String> {
        Some("set TEMP to a directory that exists and can be written".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mutation: say "problems" for one, or count from anything but the
    /// problems printed; this test fails.
    #[test]
    fn the_last_line_counts_the_problems() {
        assert_eq!(finished(0), "no problems found");
        assert_eq!(finished(1), "1 problem found");
        assert_eq!(finished(2), "2 problems found");
    }
}
