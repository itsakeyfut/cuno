//! `cuno doctor`: diagnoses the selected compiler and reports what is wrong
//! and how to fix it, without stopping at the first problem. See
//! `docs/en/specs/commands.md`, "`cuno doctor`".

use std::ffi::OsString;
use std::path::Path;

use cuno_core::{CunoError, Env, Shell};
use cuno_manifest::{CliSettings, CompilerName, Inputs, Lang, Os, Profile, Settings};
use cuno_toolchain::flags::{CompileInput, Edition, Mode, check_edition, compile_flags};
use cuno_toolchain::msvc::{self, SystemRegistry};
use cuno_toolchain::select::{self, Choices, CompilerKind, Found, ProbeInputs, Selected};
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
    diagnose(
        settings.as_ref(),
        compiler,
        env,
        host,
        &runner,
        &mut problems,
    );

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

fn diagnose(
    settings: Option<&Settings>,
    compiler: Option<String>,
    env: &Env,
    host: HostOs,
    runner: &SystemRunner,
    problems: &mut Problems<'_>,
) {
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

    let mut no_visual_studio = false;
    let installs = if host == HostOs::Windows {
        msvc::find_installations(env, runner).unwrap_or_else(|error| {
            problems.report(&error);
            no_visual_studio = true;
            Vec::new()
        })
    } else {
        Vec::new()
    };
    let explicit = settings.map(Settings::llvm_paths).unwrap_or_default();
    let llvm = cuno_toolchain::llvm::find_llvm_tools(env, host, &explicit, &installs, runner)
        .unwrap_or_else(|error| {
            problems.report(&error);
            Vec::new()
        });
    let found = Found {
        installs: &installs,
        llvm: &llvm,
        explicit_msvc: settings.and_then(Settings::msvc_path),
    };
    let constraints = settings.map(|s| s.constraints.clone()).unwrap_or_default();
    let selected = match select::select(&choices, found, &constraints, host) {
        Ok(selected) => selected,
        // Already reported: no Visual Studio is what vswhere's failure means.
        Err(ToolchainError::NoVisualStudio) if no_visual_studio => return,
        Err(error) => return problems.report(&error),
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
        return;
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
        Err(error) => return problems.report(&error),
    };
    problems
        .shell
        .status("Using", format!("Windows SDK {}", sdk.version));

    // The standard library probe, in C++ projects and outside a project
    // (toolchain.md, "Minimum compiler versions").
    let c_project = project.is_some_and(|(_, m)| m.package.lang == Lang::C);
    if selected.kind == CompilerKind::Msvc || c_project {
        return;
    }
    let build_env = msvc::resolve(toolset, &sdk).apply(env);
    match probe(
        &selected,
        project.map(|(p, m)| (p.root.as_path(), m)),
        &build_env,
        runner,
    ) {
        Ok(()) => problems
            .shell
            .status("Accepted", "by the MSVC standard library"),
        Err(error) => problems.report(&*error),
    }
}

/// The standard library probe with the build's dev-profile compile flags
/// for a C++ input, in a scratch directory of its own.
fn probe(
    selected: &Selected,
    project: Option<(&Path, &cuno_manifest::Manifest)>,
    env: &Env,
    runner: &SystemRunner,
) -> Result<(), Box<dyn CunoError>> {
    let name = match selected.kind {
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
    let flags: Vec<OsString> = compile_flags(&CompileInput {
        kind: selected.kind,
        lang: Lang::Cxx,
        edition,
        build: &build,
        profile: &profile,
        include_dirs: &include_dirs,
        test: false,
        mode: Mode::Check,
    });
    // `create_dir`, not `create_dir_all`: a temporary directory that does not
    // exist is a problem to report, not a tree to make.
    let scratch = std::env::temp_dir().join(format!("cuno-doctor-{}", std::process::id()));
    std::fs::create_dir(&scratch).map_err(|e| {
        Box::new(ScratchError {
            dir: scratch.clone(),
            reason: e.to_string(),
        }) as Box<dyn CunoError>
    })?;
    let result = select::check(
        selected,
        env,
        ProbeInputs {
            scratch: &scratch,
            flags: &flags,
        },
        runner,
    );
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
