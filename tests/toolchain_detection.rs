//! cuno's own compiler detection checked against the test support's
//! independent reference (`tests/support/compilers.rs`), on a real machine:
//! the two must find the same compilers.

mod support;

#[cfg(windows)]
use support::compilers::{Compiler, Language, require};

/// cuno finds the Visual Studio whose cl.exe the reference uses, and reads a
/// version from it.
///
/// Mutation: read the cl.exe version from the toolset directory name, pass
/// the user's `CL` through to cl.exe (run with `CL=/nologo` to see it), or
/// look for cl.exe under a host directory other than `Hostx64\x64`; this test
/// fails.
#[cfg(windows)]
#[test]
fn finds_the_visual_studio_the_reference_uses() {
    let Some(reference) = require(Compiler::Msvc, Language::C, "x.c", "x.exe") else {
        return; // skipped, reported by `require`
    };
    let reference_cl = std::path::PathBuf::from(reference.get_program());
    let reference_version = banner_version(&reference_cl);

    let env = cuno_core::Env::from_process();
    let found = cuno_toolchain::msvc::find_installations(&env, &cuno_toolchain::SystemRunner)
        .expect("cuno should find Visual Studio where the reference does");

    let toolset = found
        .iter()
        .flat_map(|install| &install.toolsets)
        .find(|toolset| same_file(&toolset.cl(), &reference_cl))
        .unwrap_or_else(|| panic!("{} is not among {found:#?}", reference_cl.display()));
    assert_eq!(
        toolset.cl_version.to_string(),
        reference_version,
        "{toolset:?}"
    );
}

/// The `<n>.<n>.<n>` in the reference cl.exe's banner, read here independently
/// of cuno's parser: the banner line is `... Version 19.52.37029 for x64`.
#[cfg(windows)]
fn banner_version(cl: &std::path::Path) -> String {
    let output = std::process::Command::new(cl)
        .env_remove("CL")
        .env_remove("_CL_")
        .output()
        .expect("the reference cl.exe should run");
    let stderr = String::from_utf8_lossy(&output.stderr);
    stderr
        .split_whitespace()
        .find(|word| {
            word.split('.').count() == 3 && word.split('.').all(|p| p.parse::<u32>().is_ok())
        })
        .unwrap_or_else(|| panic!("no version in the banner: {stderr}"))
        .to_owned()
}

#[cfg(windows)]
fn same_file(a: &std::path::Path, b: &std::path::Path) -> bool {
    std::fs::canonicalize(a).ok() == std::fs::canonicalize(b).ok()
}
