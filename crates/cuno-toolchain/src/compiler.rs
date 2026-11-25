//! The compilers cuno can build with, and every fact about one that the rest
//! of the crate branches on. Each fact is answered here by a match that
//! names every compiler, so a new one does not compile until each is
//! answered for it (`docs/en/specs/toolchain.md`, "Compiler abstraction").
//! Elsewhere, code asks these questions rather than comparing kinds.

use crate::llvm::Tool;
use crate::select::Source;
use crate::{ToolchainError, Version};

/// A compiler cuno can build with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompilerKind {
    /// cl.exe.
    Msvc,
    /// clang with GCC-style flags.
    Clang,
    /// clang-cl, Clang with MSVC-style flags.
    ClangCl,
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
    pub const ALL: [CompilerKind; 3] = [
        CompilerKind::Msvc,
        CompilerKind::Clang,
        CompilerKind::ClangCl,
    ];

    /// The name used in settings (`msvc`, `clang`, `clang-cl`).
    pub fn name(self) -> &'static str {
        match self {
            CompilerKind::Msvc => "msvc",
            CompilerKind::Clang => "clang",
            CompilerKind::ClangCl => "clang-cl",
        }
    }

    /// How its command line is written.
    pub fn syntax(self) -> Syntax {
        match self {
            CompilerKind::Msvc | CompilerKind::ClangCl => Syntax::Cl,
            CompilerKind::Clang => Syntax::Gnu,
        }
    }

    /// Whose compiler it is.
    pub fn family(self) -> Family {
        match self {
            CompilerKind::Msvc => Family::Msvc,
            CompilerKind::Clang | CompilerKind::ClangCl => Family::Llvm,
        }
    }

    /// Whether it prints the source's name on stdout for each compile, which
    /// the build leaves out of what it shows: cl does, the LLVM drivers do
    /// not.
    pub fn echoes_source_name(self) -> bool {
        match self {
            CompilerKind::Msvc => true,
            CompilerKind::Clang | CompilerKind::ClangCl => false,
        }
    }

    /// Whether `cuno check` compiles as the build does, without linking,
    /// rather than checking syntax alone: cl's `/Zs` misses diagnostics
    /// raised during code generation (C4716, and C4700, C4701 and C4715
    /// under `warnings-as-errors`), which clang and clang-cl report when
    /// checking (`docs/en/specs/build.md`, "Always-on flags").
    pub fn checks_by_compiling(self) -> bool {
        match self {
            CompilerKind::Msvc => true,
            CompilerKind::Clang | CompilerKind::ClangCl => false,
        }
    }

    /// How it says which headers a compile read.
    pub fn dependencies(self) -> Dependencies {
        match self {
            CompilerKind::Msvc => Dependencies::SourceDependencies,
            CompilerKind::Clang | CompilerKind::ClangCl => Dependencies::DepFile,
        }
    }

    /// The LLVM tool it is found as, for the LLVM family.
    pub fn llvm_tool(self) -> Option<Tool> {
        match self {
            CompilerKind::Msvc => None,
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
        }
    }

    /// The compiler `name` names, or an error naming where it was given.
    pub(crate) fn parse(name: &str, source: Source) -> Result<CompilerKind, ToolchainError> {
        if let Some(kind) = CompilerKind::ALL.into_iter().find(|k| k.name() == name) {
            return Ok(kind);
        }
        match name {
            "gcc" | "safec" => Err(ToolchainError::UnsupportedCompiler {
                name: name.to_owned(),
                source: source.to_string(),
            }),
            _ => Err(ToolchainError::UnknownCompiler {
                name: name.to_owned(),
                source: source.to_string(),
            }),
        }
    }

    /// The names of every compiler, as a sentence: `msvc, clang or clang-cl`.
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

    /// Mutation: answer true for clang or clang-cl, or false for MSVC; this
    /// test fails.
    #[test]
    fn only_cl_echoes_the_source_name() {
        assert!(CompilerKind::Msvc.echoes_source_name());
        assert!(!CompilerKind::Clang.echoes_source_name());
        assert!(!CompilerKind::ClangCl.echoes_source_name());
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
        };
        let positions: Vec<usize> = CompilerKind::ALL.iter().map(|k| position(*k)).collect();
        assert_eq!(positions, (0..positions.len()).collect::<Vec<_>>());
        assert_eq!(CompilerKind::ALL.len(), position(CompilerKind::ClangCl) + 1);
    }

    /// Mutation: change any answer for any compiler; this test fails.
    #[test]
    fn each_compiler_answers_its_properties() {
        let answers: Vec<_> = CompilerKind::ALL
            .iter()
            .map(|k| (k.name(), k.syntax(), k.family(), k.llvm_tool()))
            .collect();
        assert_eq!(
            answers,
            [
                ("msvc", Syntax::Cl, Family::Msvc, None),
                ("clang", Syntax::Gnu, Family::Llvm, Some(Tool::Clang)),
                ("clang-cl", Syntax::Cl, Family::Llvm, Some(Tool::ClangCl)),
            ]
        );
        assert_eq!(CompilerKind::names(), "msvc, clang or clang-cl");
    }
}
