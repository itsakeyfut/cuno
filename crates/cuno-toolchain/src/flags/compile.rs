//! The compile step's arguments, row by row from `docs/en/specs/build.md`,
//! "Settings and flags".

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::{BuildSettings, Edition, Lang, OptLevel, ProfileSettings, Runtime, Warnings};
use crate::compiler::{CompilerKind, Dependencies, Family, Syntax};
use crate::{ToolchainError, Version};

/// Whether a source is compiled to an object or only checked (`cuno check`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Compile to an object.
    Compile,
    /// Check the syntax and types only.
    Check,
}

/// What one compile of the build is made from.
#[derive(Clone, Copy, Debug)]
pub struct CompileInput<'a> {
    /// The driver.
    pub kind: CompilerKind,
    /// This source's language, from its extension (`docs/en/specs/project-layout.md`,
    /// "Target files").
    pub lang: Lang,
    /// The standard for this source: the package's edition when `lang` is the
    /// package's language, otherwise that language's default.
    pub edition: Edition,
    /// The resolved build settings.
    pub build: &'a BuildSettings,
    /// The resolved profile.
    pub profile: &'a ProfileSettings,
    /// The include directories cuno always adds (`src/`, the bundled
    /// headers), before `build.include_dirs`.
    pub include_dirs: &'a [PathBuf],
    /// A test build, which defines `CUNO_TEST`.
    pub test: bool,
    /// Compile or check.
    pub mode: Mode,
}

/// The flags every source of the build gets, without the mode, the source
/// and the output: what the standard library probe is given
/// (`select::ProbeInputs::flags`). `[build.<compiler>] flags` come last.
pub fn compile_flags(input: &CompileInput<'_>) -> Vec<OsString> {
    let mut out = Flags::default();
    let msvc_style = match input.kind.syntax() {
        Syntax::Cl => true,
        Syntax::Gnu => false,
    };
    let cxx = input.lang == Lang::Cxx;
    let build = input.build;
    let profile = input.profile;

    // Always on.
    if msvc_style {
        out.push(&["/nologo", "/utf-8"]);
    }
    match input.kind.family() {
        // The MSVC ABI, so the objects link with MSVC-built libraries.
        Family::Llvm => out.push(&["--target=x86_64-pc-windows-msvc"]),
        Family::Msvc => {}
    }
    if msvc_style && cxx {
        out.push(&["/Zc:__cplusplus"]);
    }

    out.push(&[standard(input.kind, input.edition)]);

    out.push(match (build.warnings, input.kind) {
        (Warnings::Default, _) => &[][..],
        (Warnings::All, CompilerKind::Clang) => &["-Wall", "-Wextra"],
        (Warnings::All, CompilerKind::Msvc | CompilerKind::ClangCl) => &["/W4"],
        (Warnings::Pedantic, CompilerKind::Msvc) => &["/W4", "/permissive-"],
        (Warnings::Pedantic, CompilerKind::ClangCl) => &["/W4", "-Wpedantic"],
        (Warnings::Pedantic, CompilerKind::Clang) => &["-Wall", "-Wextra", "-Wpedantic"],
    });
    if build.warnings_as_errors {
        out.push(&[if msvc_style { "/WX" } else { "-Werror" }]);
    }

    out.push(&[match (build.runtime, input.kind) {
        (Runtime::Dynamic, CompilerKind::Msvc | CompilerKind::ClangCl) => "/MD",
        (Runtime::Static, CompilerKind::Msvc | CompilerKind::ClangCl) => "/MT",
        (Runtime::Dynamic, CompilerKind::Clang) => "-fms-runtime-lib=dll",
        (Runtime::Static, CompilerKind::Clang) => "-fms-runtime-lib=static",
    }]);

    if cxx {
        // With exceptions or RTTI off, cl and clang-cl only warn about
        // `try` / `throw` and `typeid` / `dynamic_cast`; these make it an
        // error, as clang does (build.md, "Settings and flags").
        out.push(match (build.exceptions, input.kind) {
            (true, CompilerKind::Clang) => &[][..],
            (true, CompilerKind::Msvc | CompilerKind::ClangCl) => &["/EHsc"],
            (false, CompilerKind::Msvc) => &["/we4530"],
            (false, CompilerKind::ClangCl) => &[],
            (false, CompilerKind::Clang) => &["-fno-exceptions"],
        });
        if !build.rtti {
            out.push(match input.kind {
                CompilerKind::Msvc => &["/GR-", "/we4541"][..],
                CompilerKind::ClangCl => &["/GR-", "-Werror=rtti"],
                CompilerKind::Clang => &["-fno-rtti"],
            });
        }
    }

    out.push(&[match (profile.opt_level, msvc_style) {
        (OptLevel::O0, true) => "/Od",
        (OptLevel::O1 | OptLevel::O2 | OptLevel::O3, true) => "/O2",
        (OptLevel::Size, true) => "/O1",
        (OptLevel::O0, false) => "-O0",
        (OptLevel::O1, false) => "-O1",
        (OptLevel::O2, false) => "-O2",
        (OptLevel::O3, false) => "-O3",
        (OptLevel::Size, false) => "-Os",
    }]);
    if profile.debug_info {
        out.push(&[if msvc_style { "/Z7" } else { "-g" }]);
    }
    if profile.lto {
        out.push(&[match input.kind.family() {
            Family::Msvc => "/GL",
            Family::Llvm => "-flto",
        }]);
    }

    let define = if msvc_style { "/D" } else { "-D" };
    let mut defines: Vec<&str> = build.defines.iter().map(String::as_str).collect();
    if !profile.debug_assertions {
        defines.push("NDEBUG");
    }
    if input.test {
        defines.push("CUNO_TEST");
    }
    if cxx && !build.exceptions {
        defines.push("_HAS_EXCEPTIONS=0");
    }
    for d in defines {
        out.0.push(format!("{define}{d}").into());
    }

    let include = if msvc_style { "/I" } else { "-I" };
    for dir in input.include_dirs.iter().chain(&build.include_dirs) {
        let mut arg = OsString::from(include);
        arg.push(dir);
        out.0.push(arg);
    }

    out.0.extend(build.flags.iter().map(OsString::from));
    out.0
}

/// The full arguments for compiling, or checking, one `source`: the mode,
/// [`compile_flags`], the language switch and the source, then `object`
/// when compiling.
pub fn compile_args(
    input: &CompileInput<'_>,
    source: &Path,
    object: Option<&Path>,
) -> Vec<OsString> {
    let msvc_style = match input.kind.syntax() {
        Syntax::Cl => true,
        Syntax::Gnu => false,
    };
    let mut args: Vec<OsString> = vec![
        match (input.mode, msvc_style) {
            (Mode::Compile, true) => "/c",
            (Mode::Check, true) => "/Zs",
            (Mode::Compile, false) => "-c",
            (Mode::Check, false) => "-fsyntax-only",
        }
        .into(),
    ];
    args.extend(compile_flags(input));
    // The language switch: stated, so the source's language is never left to
    // the driver's own guess from the extension.
    match (input.lang, msvc_style) {
        (Lang::C, true) => args.push("/TC".into()),
        (Lang::Cxx, true) => args.push("/TP".into()),
        (Lang::C, false) => args.extend(["-x".into(), "c".into()]),
        (Lang::Cxx, false) => args.extend(["-x".into(), "c++".into()]),
    }
    args.push(source.into());
    if let (Mode::Compile, Some(object)) = (input.mode, object) {
        if msvc_style {
            let mut arg = OsString::from("/Fo");
            arg.push(object);
            args.push(arg);
        } else {
            args.extend(["-o".into(), object.into()]);
        }
    }
    args
}

/// The arguments that make `kind` write the headers a compile read into
/// `file` (`build.md`, "Settings and flags"). They come after
/// [`compile_args`], so they follow the user's `flags` and win over a
/// user's `-MF`.
pub fn dependency_args(kind: CompilerKind, file: &Path) -> Vec<OsString> {
    match (kind.dependencies(), kind.syntax()) {
        (Dependencies::SourceDependencies, _) => vec!["/sourceDependencies".into(), file.into()],
        (Dependencies::DepFile, Syntax::Cl) => {
            let mut to = OsString::from("/clang:-MF");
            to.push(file);
            vec!["/clang:-MD".into(), to]
        }
        (Dependencies::DepFile, Syntax::Gnu) => vec!["-MD".into(), "-MF".into(), file.into()],
    }
}

/// `args`, a compile's arguments for `kind` from [`compile_args`], as the
/// clang-based tools that read `compile_commands.json` (clangd, clang-tidy)
/// must see them to analyse what the compiler builds (`build.md`,
/// "Output"). They read a command for cl as one for clang-cl, so for both:
///
/// - `/c` is written `-c`, which cl takes too: clang-cl warns that `/c` is
///   unused when it only analyses, and `/WX` makes that an error;
/// - for cl, a standard switch clang-cl does not know (`/std:c++23`,
///   `/std:clatest`) is written as clang-cl's own for the same standard.
pub fn for_clang_tools(kind: CompilerKind, args: &[OsString]) -> Vec<OsString> {
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
    out
}

/// The language standard switch, naming the standard on every driver:
/// `/std:c++latest` is already C++26 on current cl and clang-cl, and
/// clang-cl ignores `/std:c++23` and `/std:c23`, so it gets clang's own
/// through `/clang:`. C23 is `c2x`, which Clang 17 also accepts.
fn standard(kind: CompilerKind, edition: Edition) -> &'static str {
    match (kind, edition) {
        (CompilerKind::Clang, Edition::Cxx17) => "-std=c++17",
        (CompilerKind::Clang, Edition::Cxx20) => "-std=c++20",
        (CompilerKind::Clang, Edition::Cxx23) => "-std=c++23",
        (CompilerKind::Clang, Edition::C11) => "-std=c11",
        (CompilerKind::Clang, Edition::C17) => "-std=c17",
        (CompilerKind::Clang, Edition::C23) => "-std=c2x",
        (CompilerKind::ClangCl, Edition::Cxx23) => "/clang:-std=c++23",
        (CompilerKind::ClangCl, Edition::C23) => "/clang:-std=c2x",
        (CompilerKind::Msvc, Edition::Cxx23) => "/std:c++23",
        (CompilerKind::Msvc, Edition::C23) => "/std:clatest",
        (CompilerKind::Msvc | CompilerKind::ClangCl, Edition::Cxx17) => "/std:c++17",
        (CompilerKind::Msvc | CompilerKind::ClangCl, Edition::Cxx20) => "/std:c++20",
        (CompilerKind::Msvc | CompilerKind::ClangCl, Edition::C11) => "/std:c11",
        (CompilerKind::Msvc | CompilerKind::ClangCl, Edition::C17) => "/std:c17",
    }
}

/// Whether `kind` at `version` accepts `edition`. cl only warns about a
/// `/std` it does not know and compiles with the old standard, so this is
/// decided from its version (`docs/en/specs/toolchain.md`, "Language
/// standards"): `/std:c++23` from cl 19.52, the oldest checked, and
/// `/std:clatest` from 19.37 (Visual Studio 2022 17.7). Every Clang cuno
/// supports accepts every edition.
pub fn check_edition(
    kind: CompilerKind,
    version: Version,
    edition: Edition,
) -> Result<(), ToolchainError> {
    let needed = match kind {
        // cl ignores a `/std` it does not know, so its version decides.
        CompilerKind::Msvc => match edition {
            Edition::Cxx23 => Version {
                major: 19,
                minor: 52,
                patch: 0,
            },
            Edition::C23 => Version {
                major: 19,
                minor: 37,
                patch: 0,
            },
            _ => return Ok(()),
        },
        // Every supported Clang accepts every edition (as `c2x` for C23).
        CompilerKind::Clang | CompilerKind::ClangCl => return Ok(()),
    };
    if version >= needed {
        return Ok(());
    }
    Err(ToolchainError::EditionNotSupported {
        compiler: kind.name(),
        version,
        edition: edition.as_str(),
        needed,
    })
}

#[derive(Default)]
struct Flags(Vec<OsString>);

impl Flags {
    fn push(&mut self, flags: &[&str]) {
        self.0.extend(flags.iter().map(OsString::from));
    }
}

#[cfg(test)]
mod tests;
