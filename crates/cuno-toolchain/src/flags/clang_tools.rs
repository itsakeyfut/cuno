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
    /// The resolved `INCLUDE`, in order (`Prepared::include_dirs`).
    pub include: &'a [PathBuf],
    /// The cl version of the MSVC toolset, whose `_MSC_VER` the build sees;
    /// `None` without a toolset, when no version is written.
    pub msvc: Option<&'a Version>,
    /// GCC's version, whose `__GNUC__` the build sees, for GCC; `None`
    /// otherwise.
    pub gnuc: Option<&'a Version>,
}

impl<'a> ToolContext<'a> {
    /// What a tool is told about `kind`, at `version`, whose environment
    /// gives `include` and, for a compiler that builds with an MSVC
    /// toolset, the toolset's cl version `msvc`. Which version macro the
    /// build sees is decided here, once for every caller: a tool told the
    /// wrong one analyses other code than the build compiles.
    pub fn new(
        kind: CompilerKind,
        version: &'a Version,
        msvc: Option<&'a Version>,
        include: &'a [PathBuf],
    ) -> ToolContext<'a> {
        ToolContext {
            kind,
            include,
            msvc,
            gnuc: match kind.family() {
                Family::Gnu => Some(version),
                Family::Msvc | Family::Llvm => None,
            },
        }
    }
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
            // GCC's is in the command itself (`for_clang_tools`).
            Family::Llvm | Family::Gnu => {}
        }
        out.extend(self.args.iter().cloned());
        out
    }
}

/// `args`, a compile's arguments for `context.kind` from
/// [`compile_args`](super::compile_args), as the Clang-based tools must see
/// them to analyse what the compiler builds: `/c` written `-c`, and for cl
/// a standard switch clang-cl does not know written as its own and `/GL`
/// left out, as clang-cl reads a cl command; `INCLUDE` after the tool's own
/// headers (`/imsvc`, or for clang `-nostdlibinc` and then `-idirafter`, as
/// clang would otherwise search the environment's headers first), written
/// before the compile's arguments so a user's own `-idirafter` is searched
/// after it, as in the build; and the MSVC version, so a tool run anywhere
/// sees what the build saw. Why each is written so is in
/// `docs/en/specs/build.md`, "Output".
pub fn for_clang_tools(context: ToolContext<'_>, args: &[OsString]) -> ToolCommand {
    let kind = context.kind;
    let mut include: Vec<OsString> = Vec::new();
    match kind.syntax() {
        Syntax::Gnu if !context.include.is_empty() => include.push("-nostdlibinc".into()),
        Syntax::Gnu | Syntax::Cl => {}
    }
    for dir in context.include {
        match kind.syntax() {
            Syntax::Cl => {
                let mut arg = OsString::from("/imsvc");
                arg.push(dir);
                include.push(arg);
            }
            Syntax::Gnu => {
                include.push("-idirafter".into());
                include.push(dir.into());
            }
        }
    }
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
        CompilerKind::ClangCl | CompilerKind::Clang | CompilerKind::Gcc => {}
    }
    // A tool reading `gcc`'s command learns no target from its name, and
    // reads MinGW's headers as the MSVC ABI's otherwise (build.md, "Output").
    match kind.family() {
        Family::Gnu => include.insert(0, format!("--target={}", crate::select::GCC_TARGET).into()),
        Family::Msvc | Family::Llvm => {}
    }
    out.splice(0..0, include);
    if let Some(version) = context.msvc {
        out.push(format!("-fms-compatibility-version={version}").into());
    }
    // Clang otherwise claims GCC 4.2.1, and `#if __GNUC__ >= 10` would read
    // otherwise than the build.
    if let Some(version) = context.gnuc {
        out.push(format!("-fgnuc-version={version}").into());
    }
    ToolCommand { args: out, kind }
}

/// The directories of an `INCLUDE` value, in order. An empty entry (`a;;b`,
/// or a trailing `;`) names no directory and is left out, rather than
/// written as an empty include directory.
pub(crate) fn include_dirs(include: Option<&OsStr>) -> Vec<PathBuf> {
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

    /// Each compiler's tools are told the version macro its builds see:
    /// GCC's version as `__GNUC__`, and the toolset's cl version as
    /// `_MSC_VER` for the compilers that have one. Every kind is asked.
    ///
    /// Mutation: give GCC no version, or give a compiler other than GCC
    /// one, or drop the toolset's version; this test fails.
    #[test]
    fn each_compiler_s_tools_are_told_the_version_its_builds_see() {
        let version = Version::find_in("16.2.0").unwrap();
        let cl = Version::find_in("19.44.35207").unwrap();
        for kind in CompilerKind::ALL {
            let msvc = (kind.family() != Family::Gnu).then_some(&cl);
            let context = ToolContext::new(kind, &version, msvc, &[]);
            let gnuc = context.gnuc.map(ToString::to_string);
            let expected = (kind == CompilerKind::Gcc).then(|| "16.2.0".to_owned());
            assert_eq!(gnuc, expected, "{}", kind.name());
            assert_eq!(context.msvc, msvc, "{}", kind.name());
        }
    }

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
    /// order, write them after the compile's arguments (a user's
    /// `-idirafter` is then searched first), or leave out the version or
    /// add one without a toolset; this test fails.
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
            gnuc: None,
        };
        let args = os(&["-c", "a.cpp"]);
        for kind in [CompilerKind::Msvc, CompilerKind::ClangCl] {
            assert_eq!(
                strings(for_clang_tools(context(kind, Some(&version)), &args).args()),
                [
                    "/imsvcC:/sdk/ucrt",
                    "/imsvcC:/msvc/include",
                    "-c",
                    "a.cpp",
                    "-fms-compatibility-version=19.52.37029"
                ],
                "{kind:?}"
            );
        }
        assert_eq!(
            strings(for_clang_tools(context(CompilerKind::Clang, None), &args).args()),
            [
                "-nostdlibinc",
                "-idirafter",
                "C:/sdk/ucrt",
                "-idirafter",
                "C:/msvc/include",
                "-c",
                "a.cpp"
            ]
        );
        // A user's own `-idirafter` in the compile's arguments stays after
        // the toolset's, as the build searches it.
        let user = os(&["-idirafter", "extra", "-c", "a.cpp"]);
        assert_eq!(
            strings(for_clang_tools(context(CompilerKind::Clang, None), &user).args()),
            [
                "-nostdlibinc",
                "-idirafter",
                "C:/sdk/ucrt",
                "-idirafter",
                "C:/msvc/include",
                "-idirafter",
                "extra",
                "-c",
                "a.cpp"
            ]
        );
        let none = ToolContext {
            kind: CompilerKind::Clang,
            include: &[],
            msvc: None,
            gnuc: None,
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
            gnuc: None,
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
        let expected: [(CompilerKind, &[&str]); 4] = [
            (
                CompilerKind::Msvc,
                &["--driver-mode=cl", TARGET_SWITCH, "-c"],
            ),
            (CompilerKind::ClangCl, &["--driver-mode=cl", "-c"]),
            (CompilerKind::Clang, &["-c"]),
            // GCC's target is in its command, once.
            (CompilerKind::Gcc, &["--target=x86_64-w64-mingw32", "-c"]),
        ];
        for (kind, want) in expected {
            let context = ToolContext {
                kind,
                include: &[],
                msvc: None,
                gnuc: None,
            };
            let command = for_clang_tools(context, &os(&["-c"]));
            assert_eq!(strings(&command.after_double_dash()), want, "{kind:?}");
        }
    }

    /// GCC's command names its target, MinGW-w64, before its include
    /// directories, which are written as clang's are, and GCC's version
    /// after: a tool reading `gcc` learns no target from the name and would
    /// read MinGW's headers as the MSVC ABI's, and would claim GCC 4.2.1
    /// (`docs/en/specs/build.md`, "Output").
    ///
    /// Mutation: leave the target or the version out, write the target for
    /// another driver, or write GCC's directories otherwise than clang's;
    /// this test fails.
    #[test]
    fn gcc_s_command_names_its_target() {
        let include = [PathBuf::from("C:/m/include")];
        let gcc = Version::find_in("16.2.0").unwrap();
        let context = |kind| ToolContext {
            kind,
            include: &include,
            msvc: None,
            gnuc: (kind == CompilerKind::Gcc).then_some(&gcc),
        };
        let args = os(&["-c", "a.c"]);
        assert_eq!(
            strings(for_clang_tools(context(CompilerKind::Gcc), &args).args()),
            [
                "--target=x86_64-w64-mingw32",
                "-nostdlibinc",
                "-idirafter",
                "C:/m/include",
                "-c",
                "a.c",
                "-fgnuc-version=16.2.0"
            ]
        );
        for kind in [
            CompilerKind::Msvc,
            CompilerKind::ClangCl,
            CompilerKind::Clang,
        ] {
            let args = strings(for_clang_tools(context(kind), &args).args());
            assert!(
                !args.iter().any(|a| a.contains("mingw32")),
                "{kind:?} {args:?}"
            );
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
