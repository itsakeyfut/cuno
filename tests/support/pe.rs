//! Reading what a linked Windows executable (PE) says about itself: its
//! subsystem, and which DLLs it names. Enough to check that a setting in
//! `Cuno.toml` reached the link, not a PE parser.

use std::path::Path;

use super::compilers::Compiler;

/// The PE subsystem an executable was linked for: 2 for Windows GUI, 3 for
/// console.
pub fn subsystem(exe: &Path) -> u16 {
    let bytes = std::fs::read(exe).unwrap_or_else(|e| panic!("{}: {e}", exe.display()));
    let pe = u32::from_le_bytes(bytes[0x3c..0x40].try_into().unwrap()) as usize;
    // The signature (4 bytes), the file header (20), then the optional
    // header, whose Subsystem field is at 68 in PE32+.
    let at = pe + 4 + 20 + 68;
    u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap())
}

/// Whether the executable names `dll` (compared without case), as an
/// import does. A search of the bytes, not of the import table: a DLL's name
/// appears in an executable only to be imported.
pub fn imports(exe: &Path, dll: &str) -> bool {
    let bytes = std::fs::read(exe)
        .unwrap_or_else(|e| panic!("{}: {e}", exe.display()))
        .to_ascii_lowercase();
    let name = dll.to_ascii_lowercase().into_bytes();
    bytes.windows(name.len()).any(|w| w == name.as_slice())
}

/// The DLLs of GCC's own runtime, which cuno links statically under either
/// `runtime` (`docs/en/specs/build.md`, "Rationale").
pub const GCC_RUNTIME_DLLS: [&str; 3] = ["libstdc++-6.dll", "libwinpthread-1.dll", "libgcc_s"];

/// Checks `exe`, built by `compiler`, against `expectations`, one per line:
///
/// - `subsystem console` / `subsystem windows`: the PE subsystem.
/// - `runtime static` / `runtime dynamic`: for MSVC, clang and clang-cl,
///   whether the executable imports `vcruntime140.dll`; for GCC, whose
///   runtime is static either way, that it imports none of
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
