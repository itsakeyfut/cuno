//! The compilers cuno can build with, and every fact about one that the rest
//! of the crate branches on. Each fact is answered here by a match that
//! names every compiler, so a new one does not compile until each is
//! answered for it (`docs/en/specs/toolchain.md`, "Compiler abstraction").
//! Elsewhere, code asks these questions rather than comparing kinds.

use std::path::{Path, PathBuf};

use crate::llvm::Tool;
use crate::select::Source;
use crate::{HostOs, ToolchainError, Version};

/// A compiler cuno can build with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompilerKind {
    /// cl.exe.
    Msvc,
    /// clang with GCC-style flags.
    Clang,
    /// clang-cl, Clang with MSVC-style flags.
    ClangCl,
    /// MinGW-w64's GCC (`docs/en/specs/toolchain.md`, "MinGW (GCC)").
    Gcc,
}

/// How a compiler's command line is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Syntax {
    /// cl's: `/c`, `/Fo<obj>`, `/D`, linker options after `/link`.
    Cl,
    /// GCC's: `-c`, `-o <obj>`, `-D`, `-l`.
    Gnu,
}

/// Whose compiler it is, which decides its linker, its link-time
/// optimisation and whether it needs a target and the MSVC standard library
/// check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// Microsoft's cl.exe with link.exe.
    Msvc,
    /// An LLVM driver with lld, targeting the MSVC ABI.
    Llvm,
    /// GCC with its own linker and runtime, targeting MinGW-w64.
    Gnu,
}

/// How a compiler says which headers a compile read
/// (`docs/en/specs/build.md`, "Incremental and parallel builds").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dependencies {
    /// cl's `/sourceDependencies` JSON file, whose paths do not depend on
    /// the language Visual Studio is installed in.
    SourceDependencies,
    /// A `-MD` dependency file in make's syntax, from clang and, through
    /// `/clang:`, clang-cl, whose `/showIncludes` leaves out files read by
    /// `#embed`.
    DepFile,
}

impl CompilerKind {
    /// Every compiler, in the order cuno lists them. A new variant is added
    /// here as well as to the matches below.
    pub const ALL: [CompilerKind; 4] = [
        CompilerKind::Msvc,
        CompilerKind::Clang,
        CompilerKind::ClangCl,
        CompilerKind::Gcc,
    ];

    /// The name used in settings (`msvc`, `clang`, `clang-cl`).
    pub const fn name(self) -> &'static str {
        match self {
            CompilerKind::Msvc => "msvc",
            CompilerKind::Clang => "clang",
            CompilerKind::ClangCl => "clang-cl",
            CompilerKind::Gcc => "gcc",
        }
    }

    /// How its command line is written.
    pub fn syntax(self) -> Syntax {
        match self {
            CompilerKind::Msvc | CompilerKind::ClangCl => Syntax::Cl,
            CompilerKind::Clang | CompilerKind::Gcc => Syntax::Gnu,
        }
    }

    /// Whose compiler it is.
    pub fn family(self) -> Family {
        match self {
            CompilerKind::Msvc => Family::Msvc,
            CompilerKind::Clang | CompilerKind::ClangCl => Family::Llvm,
            CompilerKind::Gcc => Family::Gnu,
        }
    }

    /// Whether it builds with an MSVC toolset (its standard library, the
    /// Windows SDK and its environment) on `host`, so selecting it needs
    /// Visual Studio (`docs/en/specs/toolchain.md`, "Environment resolution
    /// on Windows"). On Windows cl does, and Clang and clang-cl target the
    /// MSVC ABI and borrow its toolset; elsewhere none does (cl does not run
    /// there, and is refused as not found). MinGW's GCC brings its own
    /// headers, libraries and runtime. A new compiler must answer here
    /// before it can be selected.
    pub fn uses_msvc_toolset(self, host: HostOs) -> bool {
        match self {
            CompilerKind::Msvc | CompilerKind::Clang | CompilerKind::ClangCl => {
                host == HostOs::Windows
            }
            CompilerKind::Gcc => false,
        }
    }

    /// Whether it builds against the MSVC standard library, whose switches
    /// (`_HAS_EXCEPTIONS`) cuno sets: cl and the Clangs do, MinGW's GCC uses
    /// libstdc++ (`docs/en/specs/build.md`, "Settings and flags").
    pub fn uses_msvc_stl(self) -> bool {
        match self {
            CompilerKind::Msvc | CompilerKind::Clang | CompilerKind::ClangCl => true,
            CompilerKind::Gcc => false,
        }
    }

    /// Whether the tools it runs open paths in the ANSI code page, so that
    /// what they create or read must be named in ASCII: MinGW's assembler
    /// and linker do (`docs/en/specs/toolchain.md`, "MinGW (GCC)"); cl,
    /// the Clangs and lld open them as Unicode.
    pub fn needs_ascii_paths(self) -> bool {
        match self {
            CompilerKind::Gcc => true,
            CompilerKind::Msvc | CompilerKind::Clang | CompilerKind::ClangCl => false,
        }
    }

    /// The driver that links objects compiled by `program`, when any is C++
    /// (`cxx`): GCC's `gcc` adds no C++ standard library, so C++ links with
    /// the `g++` beside it (`docs/en/specs/build.md`, "Linking"); every other
    /// driver links both languages.
    pub fn link_driver(self, program: &Path, cxx: bool) -> PathBuf {
        match self {
            CompilerKind::Gcc if cxx => {
                let mut name = std::ffi::OsString::from("g++");
                if let Some(ext) = program.extension() {
                    name.push(".");
                    name.push(ext);
                }
                program.with_file_name(name)
            }
            CompilerKind::Gcc
            | CompilerKind::Msvc
            | CompilerKind::Clang
            | CompilerKind::ClangCl => program.to_owned(),
        }
    }

    /// Whether it prints the source's name on stdout for each compile, which
    /// the build leaves out of what it shows: cl does, the LLVM drivers and
    /// GCC do not.
    pub fn echoes_source_name(self) -> bool {
        match self {
            CompilerKind::Msvc => true,
            CompilerKind::Clang | CompilerKind::ClangCl | CompilerKind::Gcc => false,
        }
    }

    /// Whether `cuno check` compiles as the build does, without linking,
    /// rather than checking syntax alone: cl's `/Zs` misses diagnostics
    /// raised during code generation (C4716, and C4700, C4701 and C4715
    /// under `warnings-as-errors`), which clang and clang-cl report when
    /// checking; GCC's `-fsyntax-only` misses those its optimiser raises
    /// (`-Warray-bounds`, `-Wstringop-overflow`) (`docs/en/specs/build.md`,
    /// "Always-on flags").
    pub fn checks_by_compiling(self) -> bool {
        match self {
            CompilerKind::Msvc | CompilerKind::Gcc => true,
            CompilerKind::Clang | CompilerKind::ClangCl => false,
        }
    }

    /// How it says which headers a compile read.
    pub fn dependencies(self) -> Dependencies {
        match self {
            CompilerKind::Msvc => Dependencies::SourceDependencies,
            CompilerKind::Clang | CompilerKind::ClangCl | CompilerKind::Gcc => {
                Dependencies::DepFile
            }
        }
    }

    /// The LLVM tool it is found as, for the LLVM family.
    pub fn llvm_tool(self) -> Option<Tool> {
        match self {
            CompilerKind::Msvc | CompilerKind::Gcc => None,
            CompilerKind::Clang => Some(Tool::Clang),
            CompilerKind::ClangCl => Some(Tool::ClangCl),
        }
    }

    /// The oldest version cuno supports (`toolchain.md`, "Minimum compiler
    /// versions").
    pub(crate) fn minimum(self) -> Version {
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
            CompilerKind::Gcc => Version {
                major: 10,
                minor: 0,
                patch: 0,
            },
        }
    }

    /// The compiler `name` names, or an error naming where it was given.
    pub(crate) fn parse(name: &str, source: Source) -> Result<CompilerKind, ToolchainError> {
        if let Some(kind) = CompilerKind::ALL.into_iter().find(|k| k.name() == name) {
            return Ok(kind);
        }
        match name {
            "safec" => Err(ToolchainError::UnsupportedCompiler {
                name: name.to_owned(),
                source: source.to_string(),
            }),
            _ => Err(ToolchainError::UnknownCompiler {
                name: name.to_owned(),
                source: source.to_string(),
            }),
        }
    }

    /// The names of every compiler, as a sentence: `msvc, clang, clang-cl or
    /// gcc`.
    pub(crate) fn names() -> String {
        let names: Vec<&str> = CompilerKind::ALL.iter().map(|k| k.name()).collect();
        match names.split_last() {
            Some((last, [])) => (*last).to_owned(),
            Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
            None => String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// On Windows every kind but GCC uses an MSVC toolset, and none does
    /// elsewhere. Every kind is asked, so a new one is too.
    ///
    /// Mutation: answer false for clang on Windows, true elsewhere, or true
    /// for GCC; this test fails.
    #[test]
    fn which_kinds_use_an_msvc_toolset() {
        for kind in CompilerKind::ALL {
            let windows = kind.uses_msvc_toolset(HostOs::Windows);
            let other = kind.uses_msvc_toolset(HostOs::Other);
            let want = match kind {
                CompilerKind::Msvc | CompilerKind::Clang | CompilerKind::ClangCl => (true, false),
                CompilerKind::Gcc => (false, false),
            };
            assert_eq!((windows, other), want, "{}", kind.name());
        }
    }

    /// Mutation: answer true for clang, clang-cl or GCC, or false for MSVC;
    /// this test fails.
    #[test]
    fn only_cl_echoes_the_source_name() {
        assert!(CompilerKind::Msvc.echoes_source_name());
        assert!(!CompilerKind::Clang.echoes_source_name());
        assert!(!CompilerKind::ClangCl.echoes_source_name());
        assert!(!CompilerKind::Gcc.echoes_source_name());
    }

    /// cl and GCC check by compiling, as their checks alone miss what code
    /// generation raises; the LLVM drivers check syntax.
    ///
    /// Mutation: change the answer for any compiler; this test fails.
    #[test]
    fn cl_and_gcc_check_by_compiling() {
        let by_compiling: Vec<bool> = CompilerKind::ALL
            .iter()
            .map(|k| k.checks_by_compiling())
            .collect();
        assert_eq!(by_compiling, [true, false, false, true]);
    }

    /// Mutation: leave a compiler out of `ALL`, list it twice, or reorder it;
    /// this test fails. (A new variant does not compile here until this
    /// match names it, which is the reminder to add it to `ALL`.)
    #[test]
    fn all_lists_every_compiler_once_in_order() {
        let position = |kind: CompilerKind| match kind {
            CompilerKind::Msvc => 0,
            CompilerKind::Clang => 1,
            CompilerKind::ClangCl => 2,
            CompilerKind::Gcc => 3,
        };
        let positions: Vec<usize> = CompilerKind::ALL.iter().map(|k| position(*k)).collect();
        assert_eq!(positions, (0..positions.len()).collect::<Vec<_>>());
        assert_eq!(CompilerKind::ALL.len(), position(CompilerKind::Gcc) + 1);
    }

    /// GCC links C++ with the `g++` beside it, keeping the extension; every
    /// other driver, and GCC for C, links with itself.
    ///
    /// Mutation: link GCC's C++ with `gcc`, its C with `g++`, or another
    /// driver with `g++`; this test fails.
    #[test]
    fn gcc_links_cxx_with_g_plus_plus() {
        let gcc = Path::new("C:/msys64/ucrt64/bin/gcc.exe");
        assert_eq!(
            CompilerKind::Gcc.link_driver(gcc, true),
            Path::new("C:/msys64/ucrt64/bin/g++.exe")
        );
        assert_eq!(CompilerKind::Gcc.link_driver(gcc, false), gcc);
        assert_eq!(
            CompilerKind::Gcc.link_driver(Path::new("/usr/bin/gcc"), true),
            Path::new("/usr/bin/g++")
        );
        for kind in [
            CompilerKind::Msvc,
            CompilerKind::Clang,
            CompilerKind::ClangCl,
        ] {
            assert_eq!(
                kind.link_driver(Path::new("x/cc.exe"), true),
                Path::new("x/cc.exe")
            );
        }
    }

    /// Only GCC's tools open paths in the ANSI code page.
    ///
    /// Mutation: change the answer for any compiler; this test fails.
    #[test]
    fn only_gcc_needs_ascii_paths() {
        let ascii: Vec<bool> = CompilerKind::ALL
            .iter()
            .map(|k| k.needs_ascii_paths())
            .collect();
        assert_eq!(ascii, [false, false, false, true]);
    }

    /// Mutation: say any compiler but GCC does not use the MSVC standard
    /// library, or that GCC does; this test fails.
    #[test]
    fn only_gcc_brings_its_own_standard_library() {
        let msvc_stl: Vec<bool> = CompilerKind::ALL
            .iter()
            .map(|k| k.uses_msvc_stl())
            .collect();
        assert_eq!(msvc_stl, [true, true, true, false]);
    }

    /// Mutation: change any answer for any compiler; this test fails.
    #[test]
    fn each_compiler_answers_its_properties() {
        let answers: Vec<_> = CompilerKind::ALL
            .iter()
            .map(|k| {
                (
                    k.name(),
                    k.syntax(),
                    k.family(),
                    k.llvm_tool(),
                    k.dependencies(),
                    k.minimum().to_string(),
                )
            })
            .collect();
        let dep = Dependencies::DepFile;
        let deps = Dependencies::SourceDependencies;
        assert_eq!(
            answers,
            [
                (
                    "msvc",
                    Syntax::Cl,
                    Family::Msvc,
                    None,
                    deps,
                    "19.29.0".to_owned()
                ),
                (
                    "clang",
                    Syntax::Gnu,
                    Family::Llvm,
                    Some(Tool::Clang),
                    dep,
                    "17.0.0".to_owned()
                ),
                (
                    "clang-cl",
                    Syntax::Cl,
                    Family::Llvm,
                    Some(Tool::ClangCl),
                    dep,
                    "17.0.0".to_owned()
                ),
                (
                    "gcc",
                    Syntax::Gnu,
                    Family::Gnu,
                    None,
                    dep,
                    "10.0.0".to_owned()
                ),
            ]
        );
        assert_eq!(CompilerKind::names(), "msvc, clang, clang-cl or gcc");
    }
}
