//! `cuno self update` run as a process, for the cases that refuse before
//! anything is fetched (`docs/en/specs/commands.md`, "`cuno self update`").
//! The update itself is tested in `cuno-self` against files, since a test
//! cannot make a release newer than the cuno it builds.

mod support;

use std::path::Path;
use std::process::Command;

use support::Project;

/// A copy of the built cuno at `dir\cuno.exe`, run from there.
fn copy_into(dir: &Path) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let exe = dir.join("cuno.exe");
    std::fs::copy(env!("CARGO_BIN_EXE_cuno"), &exe).unwrap();
    exe
}

/// A cuno that is neither in the cuno home's `bin` nor cargo's is not
/// rewritten, and says how to install it: exit code 3, so a script does not
/// take it as updated.
///
/// Mutation: let `Method::Other` through to the update, or exit 0 for it;
/// this test fails.
#[test]
fn a_cuno_placed_by_hand_is_pointed_to_the_installer() {
    let p = Project::new();
    let cargo = p.path().join("cargo");
    let exe = copy_into(&p.path().join("tools"));
    let mut cmd = Command::new(&exe);
    cmd.args(["self", "update"]);
    let run = p.run_command_with(cmd, &[("CARGO_HOME", cargo.to_str().unwrap())]);
    run.assert(
        3,
        "",
        &format!(
            "error: {} was not installed by cuno's installer, so cuno cannot update it\n\
             help: install cuno with: powershell -c \"irm https://github.com/itsakeyfut/cuno/releases/latest/download/install.ps1 | iex\"\n",
            exe.display()
        ),
    );
}

/// A cuno in `<CARGO_HOME>\bin` is pointed to cargo, with exit code 3.
///
/// Mutation: treat cargo's `bin` as another place, or exit 0 for it; this
/// test fails.
#[test]
fn a_cargo_installed_cuno_is_pointed_to_cargo() {
    let p = Project::new();
    let cargo = p.path().join("cargo");
    let exe = copy_into(&cargo.join("bin"));
    let mut cmd = Command::new(&exe);
    cmd.args(["self", "update"]);
    let run = p.run_command_with(cmd, &[("CARGO_HOME", cargo.to_str().unwrap())]);
    run.assert(
        3,
        "",
        "error: cuno was installed by cargo install, so cargo updates it\nhelp: run cargo install cuno\n",
    );
}

/// Like every command but `cuno new`, it reads the project's configuration
/// too, and a broken one stops it before anything else
/// (`docs/en/specs/commands.md`, "Available for all commands").
///
/// Mutation: read only the user's configuration, as `cuno new` does; this
/// test fails.
#[test]
fn a_broken_project_configuration_stops_it() {
    let p = Project::new()
        .file(
            "Cuno.toml",
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
        )
        .file(".cuno/config.toml", "this is = = not toml\n");
    let run = p.cuno(&["self", "update"]);
    assert_eq!(run.code, Some(3), "{}", run.stderr);
    assert!(
        run.stderr.contains(".cuno") && run.stderr.contains("config.toml"),
        "{}",
        run.stderr
    );
    assert!(!run.stderr.contains("installer"), "{}", run.stderr);
}
