//! `cuno toolchain list` and `cuno toolchain default <compiler>`: the
//! compilers cuno detects, and the user's default among them. Selection's
//! answers come from `cuno_toolchain::select`, as `cuno doctor`'s do; this
//! module only lays them out and writes the default. See
//! `docs/en/specs/commands.md`, "`cuno toolchain list`" and "`cuno toolchain
//! default <compiler>`".

use std::io::Write;
use std::path::Path;

use cuno_core::{CunoError, Env, Shell};
use cuno_manifest::Settings;
use cuno_toolchain::compiler::CompilerKind;
use cuno_toolchain::llvm::Tool;
use cuno_toolchain::prepare::Detected;
use cuno_toolchain::select::{self, Availability, Choices, Constraints, Source};
use cuno_toolchain::{HostOs, SystemRunner, ToolchainError, Version};

// Named apart from this module, which is the command of the same name.
use cuno_build::toolchain as ready;

use crate::build::boxed;

/// One line of the list: a compiler or tool, and what selecting it would
/// use, or why nothing.
#[derive(Debug, PartialEq, Eq)]
struct Row {
    name: &'static str,
    detail: Detail,
    /// `(selected: …)` or `(would be selected: …)`, on the line of the
    /// compiler selection picks.
    note: Option<String>,
}

/// What a line says after the name.
#[derive(Debug, PartialEq, Eq)]
enum Detail {
    /// The copy selecting it would use: its version and executable.
    Copy { version: String, path: String },
    /// No copy can be used, and why: `not found` or `not usable: …`.
    Unavailable(String),
}

impl Detail {
    fn copy(version: &Version, path: &Path) -> Detail {
        Detail::Copy {
            version: version.to_string(),
            path: path.display().to_string(),
        }
    }

    fn unusable(error: &ToolchainError) -> Detail {
        Detail::Unavailable(format!("not usable: {error}"))
    }

    fn not_found() -> Detail {
        Detail::Unavailable("not found".to_owned())
    }
}

/// The list as printed: the names padded to the longest, then the versions
/// padded to the longest, each column followed by two spaces.
fn render(rows: &[Row]) -> String {
    let name_width = rows.iter().map(|r| r.name.len()).max().unwrap_or(0);
    let version_width = rows
        .iter()
        .filter_map(|r| match &r.detail {
            Detail::Copy { version, .. } => Some(version.len()),
            Detail::Unavailable(_) => None,
        })
        .max()
        .unwrap_or(0);
    let mut out = String::new();
    for row in rows {
        out += &format!("{:name_width$}  ", row.name);
        match &row.detail {
            Detail::Copy { version, path } => {
                out += &format!("{version:version_width$}  {path}");
            }
            Detail::Unavailable(why) => out += why,
        }
        if let Some(note) = &row.note {
            out += &format!("  ({note})");
        }
        out.push('\n');
    }
    out
}

/// Why `kind` was not searched for, when its search failed (a configured
/// path with no file there).
fn search_error(detected: &Detected, kind: CompilerKind) -> Option<&ToolchainError> {
    match kind {
        CompilerKind::Msvc => detected.installs.as_ref().err(),
        CompilerKind::Clang | CompilerKind::ClangCl => detected.llvm.as_ref().err(),
        CompilerKind::Gcc => detected.gcc.as_ref().and_then(|g| g.as_ref().err()),
    }
}

/// The list's lines for what `detected` found, with the compiler `choices`
/// selects marked, and the error when no layer's name is one cuno can
/// select (the list is made all the same).
fn rows(
    choices: &Choices,
    constraints: &Constraints,
    detected: &Detected,
    msvc_path: Option<&Path>,
    host: HostOs,
) -> (Vec<Row>, Option<ToolchainError>) {
    let (wanted, unknown) = match choices.kind(host) {
        Ok(wanted) => (Some(wanted), None),
        Err(error) => (None, Some(error)),
    };
    let found = detected.found(msvc_path);
    // Every kind, none excepted; and a Visual Studio in `[toolchain.paths]
    // msvc` that cannot be used is listed as such on cl's line rather than
    // left out, since nothing else here reports it.
    let survey = select::survey(detected, found, None, false, host);

    let mut rows = Vec::new();
    for kind in CompilerKind::ALL {
        let mut detail = match survey.iter().find(|(k, _)| *k == kind) {
            Some((_, Availability::Selectable { path, version })) => Detail::copy(version, path),
            Some((_, Availability::NotFound)) => Detail::not_found(),
            Some((_, Availability::Unusable(error) | Availability::ConfiguredUnusable(error))) => {
                Detail::unusable(error)
            }
            None => match search_error(detected, kind) {
                Some(error) => Detail::unusable(error),
                None => Detail::not_found(),
            },
        };
        let mut note = None;
        if let Some((wanted, source)) = wanted
            && wanted == kind
        {
            // The survey judges with cuno's minimum alone; the compiler in
            // use is also held to `[toolchain.version]`, so its line is
            // selection's own answer (commands.md, "`cuno toolchain
            // list`").
            match select::select(choices, found, constraints, host) {
                Ok(selected) => {
                    detail = Detail::copy(&selected.version, &selected.executable);
                    note = Some(format!("selected: {source}"));
                }
                Err(error) => {
                    detail = match error {
                        ToolchainError::CompilerNotFound { .. } => Detail::not_found(),
                        // For cl, no Visual Studio is no cl; a Clang that
                        // borrows the toolset is there and cannot be used.
                        ToolchainError::NoVisualStudio if !kind.borrows_msvc_toolset() => {
                            Detail::not_found()
                        }
                        error => Detail::unusable(&error),
                    };
                    note = Some(format!("would be selected: {source}"));
                }
            }
        }
        rows.push(Row {
            name: kind.name(),
            detail,
            note,
        });
    }
    for tool in [Tool::ClangFormat, Tool::ClangTidy] {
        let detail = match &detected.llvm {
            Err(error) => Detail::unusable(error),
            Ok(llvm) => match select::select_tool(tool, llvm, host) {
                Ok(chosen) => Detail::copy(&chosen.version, &chosen.path),
                Err(ToolchainError::CompilerNotFound { .. }) => Detail::not_found(),
                Err(error) => Detail::unusable(&error),
            },
        };
        rows.push(Row {
            name: tool.name(),
            detail,
            note: None,
        });
    }
    (rows, unknown)
}

/// `cuno toolchain list`: one line per compiler and tool to `out`. A
/// compiler name no layer can select is warned about, and the list is
/// printed all the same.
pub fn list(
    settings: &Settings,
    env: &Env,
    host: HostOs,
    out: &mut dyn Write,
    shell: &mut Shell,
) -> Result<i32, Box<dyn CunoError>> {
    let runner = SystemRunner::new();
    // Every compiler is searched for, GCC too, since the list shows each.
    let detected = ready::detect(Some(settings), env, host, &runner, true);
    let (rows, unknown) = rows(
        &settings.compiler,
        &settings.constraints,
        &detected,
        settings.msvc_path(),
        host,
    );
    if let Some(error) = unknown {
        shell.warning(&error);
    }
    // A list nobody reads (a closed pipe) is no failure of cuno's.
    let _ = out.write_all(render(&rows).as_bytes());
    let _ = out.flush();
    Ok(0)
}

/// `cuno toolchain default <compiler>`: writes `name` as the user's default
/// compiler, then warns when a layer above it chooses another here, or
/// when the compiler cannot be used on this machine.
pub fn default(
    name: &str,
    settings: &Settings,
    env: &Env,
    host: HostOs,
    shell: &mut Shell,
) -> Result<i32, Box<dyn CunoError>> {
    // The name is judged as `--compiler` would judge it, before anything is
    // written.
    let asked = Choices {
        cli: Some(name.to_owned()),
        ..Choices::default()
    };
    let (kind, _) = asked.kind(host).map_err(boxed)?;

    let path = settings.home.join("config.toml");
    cuno_manifest::write_user_compiler(&path, name).map_err(boxed)?;
    shell.status(
        "Updated",
        format!("[toolchain] compiler = \"{name}\" in {}", path.display()),
    );

    // What selects here with the new default in the user's layer: only the
    // layers above it can choose another.
    let mut here = settings.compiler.clone();
    here.user = Some(name.to_owned());
    match here.kind(host) {
        Ok((selected, Source::UserConfig)) if selected == kind => {}
        Ok((selected, source)) => shell.warning(format!(
            "{name} is the default, but here {source} selects {}",
            selected.name()
        )),
        Err(error) => shell.warning(format!("{name} is the default, but here {error}")),
    }

    // Whether this machine can build with it, as a build would judge;
    // GCC is run to be found, so only when it is the default.
    let runner = SystemRunner::new();
    let detected = ready::detect(
        Some(settings),
        env,
        host,
        &runner,
        kind == CompilerKind::Gcc,
    );
    let found = detected.found(settings.msvc_path());
    if let Err(error) = select::select(&asked, found, &settings.constraints, host) {
        shell.warning(&error);
        if let Some(help) = error.help() {
            shell.help(help);
        }
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cuno_toolchain::gcc::{Gcc, Origin as GccOrigin, Probe as GccProbe};
    use cuno_toolchain::llvm::{LlvmTool, Origin, Probe};
    use std::path::PathBuf;

    fn v(text: &str) -> Version {
        Version::find_in(text).unwrap()
    }

    fn llvm(tool: Tool, path: &str, version: &str) -> LlvmTool {
        LlvmTool {
            tool,
            path: PathBuf::from(path),
            probe: Probe::Ran {
                version: v(version),
                prerelease: false,
                target: Some("x86_64-pc-windows-msvc".to_owned()),
            },
            origin: Origin::Path,
        }
    }

    fn gcc(path: &str, version: &str) -> Gcc {
        Gcc {
            path: PathBuf::from(path),
            probe: GccProbe::Ran {
                version: v(version),
                prerelease: false,
                target: Some("x86_64-w64-mingw32".to_owned()),
                ucrt: true,
            },
            origin: GccOrigin::Msys2,
        }
    }

    /// No Visual Studio, LLVM 22 for the compilers and other versions for
    /// the two tools, and `gcc` as given.
    fn detected(gcc: Option<Vec<Gcc>>) -> Detected {
        Detected {
            installs: Ok(Vec::new()),
            llvm: Ok(vec![
                llvm(Tool::Clang, "C:/llvm/clang.exe", "22.1.8"),
                llvm(Tool::ClangCl, "C:/llvm/clang-cl.exe", "22.1.8"),
                llvm(Tool::ClangFormat, "C:/llvm/clang-format.exe", "21.1.0"),
                llvm(Tool::ClangTidy, "C:/llvm/clang-tidy.exe", "20.1.0"),
            ]),
            gcc: gcc.map(Ok),
        }
    }

    fn by_env(name: &str) -> Choices {
        Choices {
            env: Some(name.to_owned()),
            ..Choices::default()
        }
    }

    fn listed(choices: &Choices, detected: &Detected) -> (String, Option<ToolchainError>) {
        let (rows, unknown) = rows(
            choices,
            &Constraints::default(),
            detected,
            None,
            HostOs::Windows,
        );
        (render(&rows), unknown)
    }

    /// Each line gives its own copy's version and path, each tool its own,
    /// and only the compiler selection picks is marked, with why.
    ///
    /// Mutation: give a line another copy's path or version (clang-tidy
    /// clang-format's, say), drop the path, mark another line, or search
    /// GCC's copies for nothing; this test fails.
    #[test]
    fn each_line_names_its_own_copy_and_the_selected_is_marked() {
        let detected = detected(Some(vec![gcc("C:/msys64/ucrt64/bin/gcc.exe", "16.2.0")]));
        let (list, unknown) = listed(&by_env("gcc"), &detected);
        assert!(unknown.is_none());
        assert_eq!(
            list,
            "msvc          not found\n\
             clang         22.1.8  C:/llvm/clang.exe\n\
             clang-cl      22.1.8  C:/llvm/clang-cl.exe\n\
             gcc           16.2.0  C:/msys64/ucrt64/bin/gcc.exe  (selected: CUNO_COMPILER)\n\
             clang-format  21.1.0  C:/llvm/clang-format.exe\n\
             clang-tidy    20.1.0  C:/llvm/clang-tidy.exe\n"
        );
    }

    /// The compiler selection would pick, when it cannot be used, says why
    /// on its line and that it is the one that would be selected; GCC not
    /// searched at all is not found.
    ///
    /// Mutation: mark it `selected`, leave the note out, or call a missing
    /// GCC unusable; this test fails.
    #[test]
    fn a_compiler_that_would_be_selected_and_cannot_says_so() {
        let (list, _) = listed(&by_env("gcc"), &detected(Some(Vec::new())));
        assert!(
            list.contains("gcc           not found  (would be selected: CUNO_COMPILER)\n"),
            "{list}"
        );
        let (list, _) = listed(&by_env("msvc"), &detected(None));
        assert!(
            list.contains("msvc          not found  (would be selected: CUNO_COMPILER)\n"),
            "{list}"
        );
        assert!(list.contains("gcc           not found\n"), "{list}");
    }

    /// A search that failed (a configured path with no file there) makes
    /// every line it covers `not usable`, with the reason, the two tools
    /// included.
    ///
    /// Mutation: call them `not found`, drop the reason, or leave the tools
    /// out of it; this test fails.
    #[test]
    fn a_failed_search_makes_its_lines_not_usable_with_the_reason() {
        let mut detected = detected(None);
        detected.llvm = Err(ToolchainError::ConfiguredToolMissing {
            setting: "[toolchain.paths] clang".to_owned(),
            path: PathBuf::from("C:/nope/clang.exe"),
        });
        let (list, _) = listed(&by_env("gcc"), &detected);
        for name in ["clang", "clang-cl", "clang-format", "clang-tidy"] {
            let line = list
                .lines()
                .find(|l| l.split_whitespace().next() == Some(name))
                .unwrap();
            assert!(
                line.ends_with(
                    "not usable: [toolchain.paths] clang is C:/nope/clang.exe, but there is no file there"
                ),
                "{line}"
            );
        }
    }

    /// A name no layer can select is returned to be warned about, and the
    /// list is made all the same, with no line marked.
    ///
    /// Mutation: drop the error, or make no list; this test fails.
    #[test]
    fn an_unknown_name_is_reported_and_the_list_still_made() {
        let (list, unknown) = listed(&by_env("cc"), &detected(None));
        assert!(
            matches!(unknown, Some(ToolchainError::UnknownCompiler { .. })),
            "{unknown:?}"
        );
        assert_eq!(list.lines().count(), 6, "{list}");
        assert!(!list.contains('('), "{list}");
    }

    /// Names and versions are each padded to the longest, two spaces after
    /// each column, and the note last.
    ///
    /// Mutation: drop the padding, pad to a fixed width, or pad the
    /// versions of lines that have none; this test fails.
    #[test]
    fn render_pads_each_column() {
        let copy = |name, version: &str, path: &str| Row {
            name,
            detail: Detail::Copy {
                version: version.to_owned(),
                path: path.to_owned(),
            },
            note: None,
        };
        let mut selected = copy("msvc", "19.52.37029", "C:/cl.exe");
        selected.note = Some("selected: automatic selection".to_owned());
        let rows = [
            selected,
            copy("clang", "22.1.8", "C:/clang.exe"),
            Row {
                name: "gcc",
                detail: Detail::Unavailable("not found".to_owned()),
                note: None,
            },
            copy("clang-format", "22.1.8", "C:/clang-format.exe"),
        ];
        assert_eq!(
            render(&rows),
            "msvc          19.52.37029  C:/cl.exe  (selected: automatic selection)\n\
             clang         22.1.8       C:/clang.exe\n\
             gcc           not found\n\
             clang-format  22.1.8       C:/clang-format.exe\n"
        );
    }
}
