//! `cuno doctor`: diagnoses the selected compiler and reports what is wrong
//! and how to fix it, without stopping at the first problem. See
//! `docs/en/specs/commands.md`, "`cuno doctor`".

use std::ops::ControlFlow;
use std::path::Path;

use cuno_core::{CunoError, Env, Shell};
use cuno_manifest::{ConfigError, Profile, Settings};
use cuno_toolchain::llvm::Tool;
use cuno_toolchain::msvc::SystemRegistry;
use cuno_toolchain::select::{self, Availability, Choices};
use cuno_toolchain::{HostOs, SystemRunner};

use cuno_build::toolchain::{self, ReadyFor, Steps, TidyCompile};

/// Prints `error:` and `help:` and counts problems, and prints the
/// findings that are not problems as status lines, which the shell leaves
/// out under `-q` (`commands.md`, "What `cuno doctor` prints").
struct Problems<'a> {
    shell: &'a mut Shell,
    count: usize,
}

impl Problems<'_> {
    /// A finding that is not a problem, such as `Selected` or `Info`: a
    /// status line, so `-q` leaves it out. Kept beside [`Problems::report`]
    /// so each finding says which of the two it is.
    fn status(&mut self, verb: &str, message: impl std::fmt::Display) {
        self.shell.status(verb, message);
    }

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
/// `resolved` are the settings read before dispatch; their error is one of
/// the problems, and the diagnosis goes on without them. The compiler goes
/// through the build's own steps (`toolchain::steps`), which report here
/// rather than stop.
pub fn doctor(
    compiler: Option<String>,
    resolved: Result<Settings, ConfigError>,
    env: &Env,
    cwd: &Path,
    host: HostOs,
    temp: &Path,
    shell: &mut Shell,
) -> usize {
    let mut problems = Problems { shell, count: 0 };
    let runner = SystemRunner::new();

    let settings = match resolved {
        Ok(settings) => Some(settings),
        Err(error) => {
            problems.report(&error);
            None
        }
    };
    let settings = settings.as_ref();
    let choices = choices(settings, compiler, env);
    let project = settings
        .and_then(|s| s.project.as_ref())
        .map(|(p, m)| (p.root.as_path(), m));

    let surveyed = match toolchain::steps(
        ReadyFor::Doctor,
        settings,
        &choices,
        project,
        Profile::Dev,
        cwd,
        env,
        host,
        temp,
        &runner,
        &SystemRegistry,
        &mut problems,
    ) {
        Ok(surveyed) => surveyed,
        Err(_) => unreachable!("`Problems` goes on after every problem"),
    };

    // A configured path with no file is already reported; every tool
    // missing after it would only repeat it.
    if let Ok(llvm) = &surveyed.detected.llvm {
        // clang-tidy's standard library check analyses what the build
        // would, so it needs the compiler ready.
        let tidy = surveyed.ready.as_ref().and_then(|ready| {
            // Not written by doctor, which only looks: see
            // `toolchain::steps`.
            let bundled = toolchain::bundled_include(settings, env)?;
            toolchain::tidy_compile(
                &ready.selected,
                &ready.prepared,
                project,
                cwd,
                host,
                &bundled,
            )
        });
        tools(llvm, tidy.as_ref(), host, temp, &runner, &mut problems);
    }
    let found = surveyed
        .detected
        .found(settings.and_then(Settings::msvc_path));
    for (kind, availability) in select::survey(
        &surveyed.detected,
        found,
        surveyed.wanted,
        surveyed.vs_reported,
        host,
    ) {
        let line = match availability {
            Availability::Selectable(version) => {
                format!("{} {version} (not selected)", kind.name())
            }
            Availability::NotFound => format!("{} not found", kind.name()),
            Availability::Unusable(error) => format!("{} not usable: {error}", kind.name()),
            // The user's own setting is wrong: a problem even for a
            // compiler not in use (commands.md, "`cuno doctor`").
            Availability::ConfiguredUnusable(error) => {
                problems.report(&error);
                continue;
            }
        };
        problems.status("Info", line);
    }

    let count = problems.count;
    problems.status("Finished", finished(count));
    count
}

/// doctor's [`Steps`]: each problem is reported and the diagnosis goes on,
/// so `problem` never answers `Break` (`doctor` relies on it);
/// the variables left out of the environment are the build commands' to
/// warn about (`docs/en/specs/toolchain.md`, "How it is resolved").
impl Steps for Problems<'_> {
    fn status(&mut self, label: &'static str, line: String) {
        Problems::status(self, label, line);
    }

    fn ignored(&mut self, _: &str) {}

    fn problem(&mut self, error: Box<dyn CunoError>) -> ControlFlow<Box<dyn CunoError>> {
        self.report(&*error);
        ControlFlow::Continue(())
    }
}

/// The `Finished` line's message for `count` problems.
fn finished(count: usize) -> String {
    match count {
        0 => "no problems found".to_owned(),
        1 => "1 problem found".to_owned(),
        n => format!("{n} problems found"),
    }
}

/// The compiler choices: the settings', or, when the configuration cannot
/// be read, only what does not come from a file: --compiler, CUNO_COMPILER
/// and the defaults.
fn choices(settings: Option<&Settings>, compiler: Option<String>, env: &Env) -> Choices {
    settings
        .map(|s| s.compiler.clone())
        .unwrap_or_else(|| Choices {
            cli: compiler,
            env: env
                .set_value("CUNO_COMPILER")
                .map(|v| v.to_string_lossy().into_owned()),
            project: None,
            user: None,
        })
}

/// `Found` lines for clang-format and clang-tidy, a problem for each that is
/// missing, and clang-tidy's standard library check when `tidy` is given.
fn tools(
    llvm: &[cuno_toolchain::llvm::LlvmTool],
    tidy: Option<&TidyCompile>,
    host: HostOs,
    temp: &Path,
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
        problems.status("Found", format!("{} {}", tool.name(), chosen.version));
        if tool == Tool::ClangTidy
            && let Some(tidy) = tidy
            && let Err(error) = toolchain::tidy_ready(&chosen, tidy, temp, "doctor", runner)
        {
            problems.report(&*error);
        }
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
