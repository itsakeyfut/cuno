//! `cuno doctor`: diagnoses the selected compiler and reports what is wrong
//! and how to fix it, without stopping at the first problem. See
//! `docs/en/specs/commands.md`, "`cuno doctor`".

use std::path::Path;

use cuno_core::{CunoError, Env, Shell};
use cuno_manifest::{CliSettings, Inputs, Profile, Settings};
use cuno_toolchain::compiler::{CompilerKind, Family};
use cuno_toolchain::flags::check_edition;
use cuno_toolchain::llvm::Tool;
use cuno_toolchain::msvc::SystemRegistry;
use cuno_toolchain::prepare;
use cuno_toolchain::probe::{self, ProbeInputs};
use cuno_toolchain::select::{self, Choices, Found};
use cuno_toolchain::{HostOs, SystemRunner, ToolchainError};

use crate::scratch;
use crate::toolchain::{self, ProbeCompile};

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
    let resolved = crate::settings::resolve(
        Inputs {
            cwd,
            env,
            cli: &cli,
        },
        problems.shell,
    );
    let settings = match resolved {
        Ok(settings) => Some(settings),
        Err(error) => {
            problems.report(&error);
            None
        }
    };
    let settings = settings.as_ref();

    let detected = toolchain::detect(settings, env, host, &runner);
    let no_visual_studio = detected.installs.is_err();
    let installs = detected.installs.unwrap_or_else(|error| {
        problems.report(&error);
        Vec::new()
    });
    let no_llvm = detected.llvm.is_err();
    let llvm = detected.llvm.unwrap_or_else(|error| {
        problems.report(&error);
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
        cwd,
        host,
        &runner,
        &mut problems,
    );
    // A configured path with no file is already reported; every tool
    // missing after it would only repeat it.
    if !no_llvm {
        tools(&llvm, diagnosed.stl.as_ref(), host, &runner, &mut problems);
    }
    others(&diagnosed, found, !no_llvm, host, &mut problems);

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
    /// The compiler asked for (selected, or reported as a problem), if the
    /// name given was one cuno knows.
    attempted: Option<CompilerKind>,
    /// Whether selection already reported that the Visual Studio in
    /// `[toolchain.paths] msvc` cannot be used, which the MSVC `Info` line
    /// would only repeat.
    vs_reported: bool,
    /// What clang-tidy's standard library check needs, when it is to be
    /// made: a compiler, a Windows environment and a C++ project or none.
    stl: Option<ProbeCompile>,
}

#[allow(clippy::too_many_arguments)]
fn diagnose(
    settings: Option<&Settings>,
    compiler: Option<String>,
    found: Found<'_>,
    no_visual_studio: bool,
    env: &Env,
    cwd: &Path,
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

    // The kind asked for is diagnosed here, and so has no `Info` line, even
    // when its selection fails.
    let attempted = choices.kind(host).ok().map(|(kind, _)| kind);
    let constraints = settings.map(|s| s.constraints.clone()).unwrap_or_default();
    let selected = match select::select(&choices, found, &constraints, host) {
        Ok(selected) => selected,
        Err(error) => {
            let vs_error = matches!(
                error,
                ToolchainError::NoVisualStudio | ToolchainError::VisualStudioNotAt { .. }
            );
            // Already reported: no Visual Studio is what vswhere's failure
            // means.
            if !(no_visual_studio && matches!(error, ToolchainError::NoVisualStudio)) {
                problems.report(&error);
            }
            // Selection stops at Visual Studio before it looks at a Clang, and
            // the kind asked for has no `Info` line: a configured copy of it
            // that cannot be used is reported here, or nowhere.
            if vs_error
                && let Some(kind) = attempted.filter(|k| match k.family() {
                    Family::Llvm => true,
                    Family::Msvc => false,
                })
                && let Err(unusable @ ToolchainError::ConfiguredToolUnusable { .. }) =
                    select::copy_of(kind, found, &constraints, host)
            {
                problems.report(&unusable);
            }
            return Diagnosed {
                attempted,
                vs_reported: matches!(error, ToolchainError::VisualStudioNotAt { .. }),
                stl: None,
            };
        }
    };
    let mut diagnosed = Diagnosed {
        attempted,
        vs_reported: false,
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
    // An LLVM compiler borrows a toolset, named here, and is checked against
    // its standard library below; MSVC's own is on its `Selected` line.
    let borrows_msvc = match selected.kind.family() {
        Family::Llvm => true,
        Family::Msvc => false,
    };
    if borrows_msvc {
        problems.shell.status(
            "Using",
            format!(
                "MSVC {} ({})",
                toolset.cl_version,
                toolset.install_dir().display()
            ),
        );
    }
    let prepared = match prepare::prepare(&selected, env, &SystemRegistry) {
        Ok(prepared) => prepared,
        Err(error) => {
            problems.report(&error);
            return diagnosed;
        }
    };
    if let Some(sdk) = &prepared.sdk {
        problems
            .shell
            .status("Using", format!("Windows SDK {}", sdk.version));
    }

    // The standard library probe, in C++ projects and outside a project,
    // with the dev profile's flags (toolchain.md, "Minimum compiler
    // versions").
    let Some(stl) = toolchain::probe_compile(
        &selected,
        &prepared,
        project.map(|(p, m)| (p.root.as_path(), m)),
        Profile::Dev,
        cwd,
        host,
    ) else {
        return diagnosed;
    };
    if borrows_msvc {
        match toolchain::probe(&selected, &stl, "doctor", runner) {
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
    stl: Option<&ProbeCompile>,
    host: HostOs,
    runner: &SystemRunner,
    problems: &mut Problems<'_>,
) {
    for tool in [Tool::ClangFormat, Tool::ClangTidy] {
        let chosen = match select::select_tool(tool, llvm, host) {
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
            && let Err(error) = scratch::in_scratch("doctor", "clang-tidy", |scratch| {
                probe::check_tidy(
                    &chosen,
                    &stl.env,
                    ProbeInputs {
                        scratch,
                        input: &stl.settings.input(),
                        cwd: &stl.cwd,
                    },
                    runner,
                )
            })
        {
            problems.report(&*error);
        }
    }
}

/// An `Info` line for each compiler other than the one asked for: the
/// version of the copy selecting it would use, `not found`, or `not usable`
/// with the reason. cuno's minimum applies, `[toolchain.version]` does not
/// (only the compiler used is held to it). Never a problem, except a copy
/// written in `[toolchain.paths]` that cannot be used (commands.md, "`cuno
/// doctor`"), which is reported once: no MSVC line follows selection's own
/// report of the configured Visual Studio. Without `looked_for_llvm` (a configured LLVM path had no file,
/// already reported), no Clang line is printed: nothing was looked for.
fn others(
    diagnosed: &Diagnosed,
    found: Found<'_>,
    looked_for_llvm: bool,
    host: HostOs,
    problems: &mut Problems<'_>,
) {
    for kind in CompilerKind::ALL {
        let skip = match kind.family() {
            Family::Llvm => !looked_for_llvm,
            Family::Msvc => diagnosed.vs_reported,
        };
        if Some(kind) == diagnosed.attempted || skip {
            continue;
        }
        let line = match select::copy_of(kind, found, &Default::default(), host) {
            Ok((_, version, _)) => format!("{} {version} (not selected)", kind.name()),
            Err(
                error @ (ToolchainError::ConfiguredToolUnusable { .. }
                | ToolchainError::VisualStudioNotAt { .. }),
            ) => {
                problems.report(&error);
                continue;
            }
            Err(ToolchainError::CompilerNotFound { .. } | ToolchainError::NoVisualStudio) => {
                format!("{} not found", kind.name())
            }
            Err(error) => format!("{} not usable: {error}", kind.name()),
        };
        problems.shell.status("Info", line);
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
