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
    let found =
        cuno_toolchain::msvc::find_installations(&env, &cuno_toolchain::SystemRunner::new())
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

/// cuno finds, on `PATH`, the clang the reference uses, with the version that
/// clang prints. The reference's directory is put first on `PATH` (on CI it
/// is the pinned LLVM, which is not on the runner's `PATH`).
///
/// Mutation: read clang's `--version` from stderr instead of stdout (real
/// clang prints it on stdout); this test fails. Where each place is searched,
/// and which number is the version, are guarded by the unit tests in
/// `cuno-toolchain`, since on a given machine several places may hold the
/// same clang.
#[test]
fn finds_the_clang_the_reference_uses() {
    let Some(reference) = support::compilers::require(
        support::compilers::Compiler::Clang,
        support::compilers::Language::C,
        "x.c",
        "x.exe",
    ) else {
        return; // skipped, reported by `require`
    };
    let reference_clang = std::path::PathBuf::from(reference.get_program());
    let reference_dir = reference_clang.parent().unwrap().to_owned();
    let reference_version = clang_version(&reference_clang);

    let process = cuno_core::Env::from_process();
    let mut path = vec![reference_dir.clone()];
    if let Some(rest) = process.get("PATH") {
        path.extend(std::env::split_paths(rest));
    }
    let pairs = process
        .vars()
        .filter(|(name, _)| !name.eq_ignore_ascii_case("PATH"))
        .map(|(n, v)| (n.to_owned(), v.to_owned()))
        .chain([("PATH".into(), std::env::join_paths(path).unwrap())]);
    let env = cuno_core::Env::from_pairs(pairs, cuno_core::NameCase::host());

    let found = cuno_toolchain::llvm::find_llvm_tools(
        &env,
        cuno_toolchain::HostOs::current(),
        &cuno_toolchain::llvm::ExplicitPaths::default(),
        &[],
        &cuno_toolchain::SystemRunner::new(),
    )
    .expect("finding LLVM tools should not fail without configured paths");
    let first_clang = found
        .iter()
        .find(|t| t.tool == cuno_toolchain::llvm::Tool::Clang)
        .unwrap_or_else(|| panic!("no clang among {found:#?}"));
    assert_eq!(
        std::fs::canonicalize(&first_clang.path).ok(),
        std::fs::canonicalize(&reference_clang).ok()
    );
    assert_eq!(
        first_clang.version().map(|v| v.to_string()),
        Some(reference_version.clone())
    );

    // clang-cl beside it: the same file cuno finds, with the same version.
    let reference_clang_cl = reference_dir.join(
        reference_clang
            .file_name()
            .unwrap()
            .to_string_lossy()
            .replace("clang", "clang-cl"),
    );
    if reference_clang_cl.is_file() {
        let first_clang_cl = found
            .iter()
            .find(|t| t.tool == cuno_toolchain::llvm::Tool::ClangCl)
            .unwrap_or_else(|| panic!("no clang-cl among {found:#?}"));
        assert_eq!(
            std::fs::canonicalize(&first_clang_cl.path).ok(),
            std::fs::canonicalize(&reference_clang_cl).ok()
        );
        assert_eq!(
            first_clang_cl.version().map(|v| v.to_string()),
            Some(clang_version(&reference_clang_cl))
        );
    }
}

/// The version clang prints, read here independently of cuno: the word after
/// `clang version`.
fn clang_version(clang: &std::path::Path) -> String {
    let output = std::process::Command::new(clang)
        .arg("--version")
        .output()
        .expect("the reference clang should run");
    let text = String::from_utf8_lossy(&output.stdout);
    let after = text
        .split("clang version ")
        .nth(1)
        .expect("a clang version line");
    after.split_whitespace().next().unwrap().to_owned()
}

/// A `[toolchain.paths] clang` that names the installed clang-cl is refused
/// before anything is built, naming the setting and what the file is
/// (`docs/en/specs/toolchain.md`, "LLVM (clang / clang-cl / clang-format /
/// clang-tidy)").
///
/// Mutation: take a configured file for the tool its key names without
/// running it with `/?`; this test fails.
#[cfg(windows)]
#[test]
fn a_configured_clang_that_is_clang_cl_is_refused() {
    let Some(driver) = support::compilers::require_driver(Compiler::ClangCl, Language::C) else {
        return;
    };
    let path = driver.get_program().to_string_lossy().replace('\\', "/");
    let project = support::Project::new()
        .file(
            "Cuno.toml",
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nlang = \"c\"\n",
        )
        .file("src/main.c", "int main(void) { return 0; }\n")
        .file(
            ".cuno/config.toml",
            &format!("[toolchain.paths]\nclang = \"{path}\"\n"),
        );
    let run = project.cuno(&["build", "--compiler", "clang"]);
    assert_eq!(run.code, Some(3), "{}", run.stderr);
    assert!(
        run.stderr.starts_with(&format!(
            "error: [toolchain.paths] clang is {path}, but it is clang-cl, not clang\n"
        )),
        "{}",
        run.stderr
    );
}
