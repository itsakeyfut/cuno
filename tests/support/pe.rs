//! Reading what a linked Windows executable (PE) says about itself: its
//! subsystem, and which DLLs it names. Enough to check that a setting in
//! `Cuno.toml` reached the link, not a PE parser.

use std::path::Path;

use super::compilers::Compiler;

/// The PE subsystem a 64-bit (PE32+) executable was linked for: 2 for
/// Windows GUI, 3 for console. Panics on a file too short to be one, as a
/// test should.
pub fn subsystem(exe: &Path) -> u16 {
    let bytes = std::fs::read(exe).unwrap_or_else(|e| panic!("{}: {e}", exe.display()));
    let pe = u32::from_le_bytes(bytes[0x3c..0x40].try_into().unwrap()) as usize;
    // The signature (4 bytes), the file header (20), then the optional
    // header, whose Subsystem field is at 68 in PE32+.
    let at = pe + 4 + 20 + 68;
    u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap())
}

/// Whether the executable's bytes contain `dll`, compared without case
/// (linkers write import names in either case). A search of the bytes
/// rather than of the import table, to keep a PE parser out of the tests:
/// a DLL's name could also appear in a string or a debug path, so this can
/// only be trusted for names the program under test does not otherwise
/// mention.
pub fn imports(exe: &Path, dll: &str) -> bool {
    let bytes = std::fs::read(exe)
        .unwrap_or_else(|e| panic!("{}: {e}", exe.display()))
        .to_ascii_lowercase();
    let name = dll.to_ascii_lowercase().into_bytes();
    bytes.windows(name.len()).any(|w| w == name.as_slice())
}

/// The DLLs of GCC's own runtime, which cuno links statically under either
/// `runtime` (`docs/en/specs/build.md`, "Settings and flags", its first
/// "Rationale"). `libgcc_s` is a prefix, matching the whole family
/// (`libgcc_s_seh-1.dll` and the like).
pub const GCC_RUNTIME_DLLS: [&str; 3] = ["libstdc++-6.dll", "libwinpthread-1.dll", "libgcc_s"];

/// Checks `exe`, built by `compiler`, against `expectations`, one per line,
/// naming `at` (the example, compiler and profile) in a failure:
///
/// - `subsystem console` / `subsystem windows`: the PE subsystem.
/// - `runtime static` / `runtime dynamic`: for MSVC, clang and clang-cl,
///   whether the executable imports the C runtime's `vcruntime140.dll`. For
///   GCC, whose own runtime is static and whose C runtime (UCRT) is dynamic
///   under either setting, both mean the same: that it imports none of
///   [`GCC_RUNTIME_DLLS`].
///
/// Blank lines are skipped. Any other line fails, so a misspelt
/// expectation is not silently left unchecked.
#[track_caller]
pub fn check(exe: &Path, compiler: Compiler, expectations: &str, at: &str) {
    for line in expectations
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
    {
        match line {
            "subsystem console" => assert_eq!(subsystem(exe), 3, "{at}: not a console program"),
            "subsystem windows" => assert_eq!(subsystem(exe), 2, "{at}: not a GUI program"),
            "runtime static" | "runtime dynamic" => match compiler {
                Compiler::Gcc => {
                    for dll in GCC_RUNTIME_DLLS {
                        assert!(!imports(exe, dll), "{at}: imports {dll}");
                    }
                }
                Compiler::Msvc | Compiler::Clang | Compiler::ClangCl => assert_eq!(
                    imports(exe, "vcruntime140.dll"),
                    line == "runtime dynamic",
                    "{at}: {line}, but vcruntime140.dll is {}imported",
                    if line == "runtime dynamic" {
                        "not "
                    } else {
                        ""
                    }
                ),
            },
            other => panic!("{at}: unknown expectation `{other}`"),
        }
    }
}
