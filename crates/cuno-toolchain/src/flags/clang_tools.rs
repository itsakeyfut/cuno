//! A compile as the Clang-based tools read it: compile_commands.json, which
//! clangd and `cuno lint` read, and `cuno doctor`'s clang-tidy check, which
//! is to judge what `cuno lint` will run. Both build the command here: only
//! [`for_clang_tools`] makes a [`ToolCommand`], so the two cannot come to
//! give a tool different arguments, as they did before (`docs/en/specs/
//! build.md`, "Output"; `docs/en/specs/toolchain.md`, "Minimum compiler
//! versions").

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use super::Edition;
use super::compile::{TARGET_SWITCH, standard};
use crate::Version;
use crate::compiler::{CompilerKind, Family, Syntax};

/// What a Clang-based tool is told about the compiler a build ran with,
/// besides its arguments.
#[derive(Clone, Copy, Debug)]
pub struct ToolContext<'a> {
    /// The compiler.
    pub kind: CompilerKind,
    /// The resolved `INCLUDE`, in order ([`include_dirs`]).
    pub include: &'a [PathBuf],
    /// The cl version of the MSVC toolset, whose `_MSC_VER` the build sees;
    /// `None` without a toolset, when no version is written.
    pub msvc: Option<&'a Version>,
}

/// One compile as a Clang-based tool reads it. Only [`for_clang_tools`]
/// makes one, so compile_commands.json and doctor's check cannot come to
/// build it differently.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolCommand {
    args: Vec<OsString>,
    kind: CompilerKind,
}

impl ToolCommand {
    /// The arguments after the compiler's program, as compile_commands.json
    /// writes them: a tool there learns from the program which driver and
    /// target to read them as. Given after `--` instead, they need
    /// [`ToolCommand::after_double_dash`].
    pub fn args(&self) -> &[OsString] {
        &self.args
    }

    /// The arguments as given after `--`, where there is no program: a
    /// cl-style command is read as clang-cl's only with `--driver-mode=cl`,
    /// and cl's own flags name no target, so MSVC's gets the one it builds
    /// for. An LLVM driver's flags already name it. The source is among
    /// them; a tool given its file before `--` is given it once only if the
    /// caller leaves it out here.
    pub fn after_double_dash(&self) -> Vec<OsString> {
        let mut out = Vec::new();
        match self.kind.syntax() {
            Syntax::Cl => out.push("--driver-mode=cl".into()),
            Syntax::Gnu => {}
        }
        match self.kind.family() {
            Family::Msvc => out.push(TARGET_SWITCH.into()),
            Family::Llvm => {}
        }
        out.extend(self.args.iter().cloned());
        out
    }
}

/// `args`, a compile's arguments for `context.kind` from
/// [`compile_args`](super::compile_args), as the Clang-based tools must see
/// them to analyse what the compiler builds: `/c` written `-c`, and for cl
/// a standard switch clang-cl does not know written as its own and `/GL`
/// left out, as clang-cl reads a cl command; then `INCLUDE` after the
/// tool's own headers (`/imsvc`, or for clang `-nostdlibinc` and then
/// `-idirafter`, as clang would otherwise search the environment's headers
/// first) and the MSVC version, so a tool run anywhere sees what the build
/// saw. Why each is written so is in `docs/en/specs/build.md`, "Output".
pub fn for_clang_tools(context: ToolContext<'_>, args: &[OsString]) -> ToolCommand {
    let kind = context.kind;
    let mut out = args.to_vec();
    match kind.syntax() {
        Syntax::Cl => {
            for arg in &mut out {
                if arg == "/c" {
                    *arg = "-c".into();
                }
            }
        }
        Syntax::Gnu => {}
    }
    match kind {
        CompilerKind::Msvc => {
            out.retain(|arg| arg != "/GL");
            for arg in &mut out {
                let same = Edition::ALL
                    .iter()
                    .find(|(edition, _)| *arg == standard(CompilerKind::Msvc, *edition));
                if let Some((edition, _)) = same {
                    *arg = standard(CompilerKind::ClangCl, *edition).into();
                }
            }
        }
        CompilerKind::ClangCl | CompilerKind::Clang => {}
    }
    match kind.syntax() {
        Syntax::Gnu if !context.include.is_empty() => out.push("-nostdlibinc".into()),
        Syntax::Gnu | Syntax::Cl => {}
    }
    for dir in context.include {
        match kind.syntax() {
            Syntax::Cl => {
                let mut arg = OsString::from("/imsvc");
                arg.push(dir);
                out.push(arg);
            }
            Syntax::Gnu => {
                out.push("-idirafter".into());
                out.push(dir.into());
            }
        }
    }
    if let Some(version) = context.msvc {
        out.push(format!("-fms-compatibility-version={version}").into());
    }
    ToolCommand { args: out, kind }
}

/// The directories of an `INCLUDE` value, in order. An empty entry (`a;;b`,
/// or a trailing `;`) names no directory and is left out, rather than
/// written as an empty include directory.
pub fn include_dirs(include: Option<&OsStr>) -> Vec<PathBuf> {
    include
        .map(|dirs| {
            std::env::split_paths(dirs)
                .filter(|dir| !dir.as_os_str().is_empty())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: &[OsString]) -> Vec<String> {
        args.iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    /// The include directories come after the tool's own headers, in the
    /// form each driver reads, clang's alone with `-nostdlibinc` so the
    /// environment's are not searched first, then the MSVC version when
    /// there is a toolset (`docs/en/specs/build.md`, "Output").
    ///
    /// Mutation: give clang `/imsvc` or `-isystem`, give a cl-style driver
    /// `-idirafter` or `-nostdlibinc`, leave `-nostdlibinc` out for clang or
    /// give it without include directories, drop a directory, change their
    /// order, or leave out the version or add one without a toolset; this
    /// test fails.
    #[test]
    fn include_and_version_follow_the_driver() {
        let include = [
            PathBuf::from("C:/sdk/ucrt"),
            PathBuf::from("C:/msvc/include"),
        ];
        let version = Version::find_in("19.52.37029").unwrap();
        let context = |kind, msvc| ToolContext {
            kind,
            include: &include,
            msvc,
        };
        let args = os(&["-c", "a.cpp"]);
        for kind in [CompilerKind::Msvc, CompilerKind::ClangCl] {
            assert_eq!(
                strings(for_clang_tools(context(kind, Some(&version)), &args).args()),
                [
                    "-c",
                    "a.cpp",
                    "/imsvcC:/sdk/ucrt",
                    "/imsvcC:/msvc/include",
                    "-fms-compatibility-version=19.52.37029"
                ],
                "{kind:?}"
            );
        }
        assert_eq!(
            strings(for_clang_tools(context(CompilerKind::Clang, None), &args).args()),
            [
                "-c",
                "a.cpp",
                "-nostdlibinc",
                "-idirafter",
                "C:/sdk/ucrt",
                "-idirafter",
                "C:/msvc/include"
            ]
        );
        let none = ToolContext {
            kind: CompilerKind::Clang,
            include: &[],
            msvc: None,
        };
        assert_eq!(
            strings(for_clang_tools(none, &args).args()),
            ["-c", "a.cpp"]
        );
    }

    /// Mutation: keep cl's `/c`, `/std:c++23`, `/std:clatest` or `/GL`, or
    /// rewrite them for clang-cl, which takes its own; this test fails.
    #[test]
    fn a_cl_command_is_rewritten_as_clang_cl_reads_it() {
        let context = |kind| ToolContext {
            kind,
            include: &[],
            msvc: None,
        };
        for (switch, as_clang_cl) in [
            ("/std:c++23", "/clang:-std=c++23"),
            ("/std:clatest", "/clang:-std=c2x"),
            ("/std:c++20", "/std:c++20"),
        ] {
            let args = os(&["/c", switch, "/GL", "a.cpp"]);
            assert_eq!(
                strings(for_clang_tools(context(CompilerKind::Msvc), &args).args()),
                ["-c", as_clang_cl, "a.cpp"],
                "{switch}"
            );
        }
        let args = os(&["/c", "/clang:-std=c++23", "a.cpp"]);
        assert_eq!(
            strings(for_clang_tools(context(CompilerKind::ClangCl), &args).args()),
            ["-c", "/clang:-std=c++23", "a.cpp"]
        );
    }

    /// After `--`, a cl-style command is read as clang-cl's and MSVC's
    /// names its target; clang's names its own (`docs/en/specs/
    /// toolchain.md`, "Minimum compiler versions").
    ///
    /// Mutation: leave out `--driver-mode=cl` for MSVC or clang-cl, give it
    /// to clang, leave out the target for MSVC or add it for an LLVM
    /// driver, or put them after the arguments; this test fails.
    #[test]
    fn after_double_dash_names_the_driver_and_target_a_program_would() {
        let expected: [(CompilerKind, &[&str]); 3] = [
            (
                CompilerKind::Msvc,
                &["--driver-mode=cl", TARGET_SWITCH, "-c"],
            ),
            (CompilerKind::ClangCl, &["--driver-mode=cl", "-c"]),
            (CompilerKind::Clang, &["-c"]),
        ];
        for (kind, want) in expected {
            let context = ToolContext {
                kind,
                include: &[],
                msvc: None,
            };
            let command = for_clang_tools(context, &os(&["-c"]));
            assert_eq!(strings(&command.after_double_dash()), want, "{kind:?}");
        }
    }

    /// Mutation: keep an empty entry, or change the order; this test fails.
    #[test]
    fn include_dirs_keep_the_order_and_drop_empty_entries() {
        let value = OsString::from("C:\\a;;C:\\b;");
        assert_eq!(
            include_dirs(Some(&value)),
            [PathBuf::from("C:\\a"), PathBuf::from("C:\\b")]
        );
        assert!(include_dirs(None).is_empty());
    }
}
