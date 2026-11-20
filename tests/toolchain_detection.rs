//! cuno's own compiler detection checked against the test support's
//! independent reference (`tests/support/compilers.rs`), on a real machine:
//! the two must find the same compilers.

mod support;

#[cfg(windows)]
use support::compilers::{Compiler, Language, require};

/// cuno finds the Visual Studio whose cl.exe the reference uses, and reads a
/// version from it.
///
/// Mutation: read the cl.exe version from the toolset directory name (whose
/// major part is 14, not 19), or look for cl.exe under a host directory other
/// than `Hostx64\x64`; this test fails.
#[cfg(windows)]
#[test]
fn finds_the_visual_studio_the_reference_uses() {
    let Some(reference) = require(Compiler::Msvc, Language::C, "x.c", "x.exe") else {
        return; // skipped, reported by `require`
    };
    let reference_cl = std::path::PathBuf::from(reference.get_program());

    let env = cuno_core::Env::from_process();
    let found = cuno_toolchain::msvc::find_installations(&env, &cuno_toolchain::SystemRunner)
        .expect("cuno should find Visual Studio where the reference does");

    let toolset = found
        .iter()
        .flat_map(|install| &install.toolsets)
        .find(|toolset| same_file(&toolset.cl(), &reference_cl))
        .unwrap_or_else(|| panic!("{} is not among {found:#?}", reference_cl.display()));
    assert_eq!(toolset.cl_version.major, 19, "{toolset:?}");
}

#[cfg(windows)]
fn same_file(a: &std::path::Path, b: &std::path::Path) -> bool {
    std::fs::canonicalize(a).ok() == std::fs::canonicalize(b).ok()
}
