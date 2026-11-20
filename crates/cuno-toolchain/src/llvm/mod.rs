//! LLVM: finding clang, clang-cl, clang-format and clang-tidy. See
//! `docs/en/specs/toolchain.md`, "LLVM".

mod detect;

pub use detect::{ExplicitPaths, LlvmTool, Origin, Probe, Tool, find_llvm_tools};
