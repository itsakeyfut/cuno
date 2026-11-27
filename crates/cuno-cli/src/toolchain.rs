//! `cuno toolchain list` and `cuno toolchain default <compiler>`: the
//! compilers cuno detects, and the user's default among them. Selection's
//! answers come from `cuno_toolchain::select`, as `cuno doctor`'s do; this
//! module only lays them out and writes the default. See
//! `docs/en/specs/commands.md`, "`cuno toolchain list`" and "`cuno toolchain
//! default <compiler>`".

use std::io::Write;

use cuno_core::{CunoError, Env, Shell};
use cuno_manifest::Settings;
use cuno_toolchain::compiler::CompilerKind;
use cuno_toolchain::llvm::Tool;
use cuno_toolchain::select::{self, Availability, Choices, Source};
use cuno_toolchain::{HostOs, SystemRunner, ToolchainError, Version};

use cuno_build::toolchain;

use crate::build::boxed;

/// One line of the list: a compiler or tool, and what selecting it would
/// use, or why nothing.
#[derive(Debug, PartialEq, Eq)]
struct Row {
    name: &'static str,
    found: Found,
    /// `(selected: …)` or `(asked for: …)`, on the wanted compiler's line.
    note: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
enum Found {
    /// The copy it would use: its version and executable.
    Copy { version: String, path: String },
    /// Nothing usable, and why: `not found` or `not usable: …`.
    Nothing(String),
}

impl Found {
    fn copy(version: &Version, path: &std::path::Path) -> Found {
        Found::Copy {
            version: version.to_string(),
            path: path.display().to_string(),
        }
    }

    fn unusable(error: &ToolchainError) -> Found {
        Found::Nothing(format!("not usable: {error}"))
    }

    fn not_found() -> Found {
        Found::Nothing("not found".to_owned())
    }
}

/// The list as printed: the names padded to the longest, then the versions
/// padded to the longest, each column followed by two spaces.
fn render(rows: &[Row]) -> String {
    let name_width = rows.iter().map(|r| r.name.len()).max().unwrap_or(0);
    let version_width = rows
        .iter()
        .filter_map(|r| match &r.found {
            Found::Copy { version, .. } => Some(version.len()),
            Found::Nothing(_) => None,
        })
        .max()
        .unwrap_or(0);
    let mut out = String::new();
    for row in rows {
        let mut line = format!("{:name_width$}  ", row.name);
        match &row.found {
            Found::Copy { version, path } => {
                line += &format!("{version:version_width$}  {path}");
            }
            Found::Nothing(why) => line += why,
        }
        if let Some(note) = &row.note {
            line += &format!("  ({note})");
        }
        out += line.trim_end();
        out.push('\n');
    }
    out
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
    let wanted = match settings.compiler.kind(host) {
        Ok(wanted) => Some(wanted),
        Err(error) => {
            shell.warning(&error);
            None
        }
    };
    let detected = toolchain::detect(Some(settings), env, host, &runner, true);
    let found = detected.found(settings.msvc_path());
    let survey = select::survey(&detected, found, None, false, host);

    let mut rows = Vec::new();
    for kind in CompilerKind::ALL {
        let mut found_here = match survey.iter().find(|(k, _)| *k == kind) {
            Some((_, Availability::Selectable { path, version })) => Found::copy(version, path),
            Some((_, Availability::NotFound)) => Found::not_found(),
            Some((_, Availability::Unusable(error) | Availability::ConfiguredUnusable(error))) => {
                Found::unusable(error)
            }
            // Not searched: its search failed, and says why.
            None => match search_error(&detected, kind) {
                Some(error) => Found::unusable(error),
                None => Found::not_found(),
            },
        };
        let mut note = None;
        if let Some((wanted, source)) = wanted
            && wanted == kind
        {
            // The compiler builds use, judged as selection judges it, with
            // `[toolchain.version]`.
            match select::select(&settings.compiler, found, &settings.constraints, host) {
                Ok(selected) => {
                    found_here = Found::copy(&selected.version, &selected.executable);
                    note = Some(format!("selected: {source}"));
                }
                Err(error) => {
                    found_here = match error {
                        ToolchainError::CompilerNotFound { .. } => Found::not_found(),
                        ToolchainError::NoVisualStudio if !kind.borrows_msvc_toolset() => {
                            Found::not_found()
                        }
                        error => Found::unusable(&error),
                    };
                    note = Some(format!("asked for: {source}"));
                }
            }
        }
        rows.push(Row {
            name: kind.name(),
            found: found_here,
            note,
        });
    }
    for tool in [Tool::ClangFormat, Tool::ClangTidy] {
        let found_here = match &detected.llvm {
            Err(error) => Found::unusable(error),
            Ok(llvm) => match select::select_tool(tool, llvm, host) {
                Ok(chosen) => Found::copy(&chosen.version, &chosen.path),
                Err(ToolchainError::CompilerNotFound { .. }) => Found::not_found(),
                Err(error) => Found::unusable(&error),
            },
        };
        rows.push(Row {
            name: tool.name(),
            found: found_here,
            note: None,
        });
    }
    // A list nobody reads (a closed pipe) is no failure of cuno's.
    let _ = out.write_all(render(&rows).as_bytes());
    let _ = out.flush();
    Ok(0)
}

/// Why `kind` was not searched for, when its search failed.
fn search_error(
    detected: &cuno_toolchain::prepare::Detected,
    kind: CompilerKind,
) -> Option<&ToolchainError> {
    match kind {
        CompilerKind::Msvc => detected.installs.as_ref().err(),
        CompilerKind::Clang | CompilerKind::ClangCl => detected.llvm.as_ref().err(),
        CompilerKind::Gcc => detected.gcc.as_ref().and_then(|g| g.as_ref().err()),
    }
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

    let runner = SystemRunner::new();
    let detected = toolchain::detect(
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

    fn copy(name: &'static str, version: &str, path: &str) -> Row {
        Row {
            name,
            found: Found::Copy {
                version: version.to_owned(),
                path: path.to_owned(),
            },
            note: None,
        }
    }

    /// Names and versions are each padded to the longest, two spaces after
    /// each column, the note last, and no line ends in spaces.
    ///
    /// Mutation: drop the padding, pad to a fixed width, or pad the
    /// versions of lines that have none; this test fails.
    #[test]
    fn render_pads_each_column() {
        let mut selected = copy("msvc", "19.52.37029", "C:/cl.exe");
        selected.note = Some("selected: automatic selection".to_owned());
        let rows = [
            selected,
            copy("clang", "22.1.8", "C:/clang.exe"),
            Row {
                name: "gcc",
                found: Found::Nothing("not found".to_owned()),
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
