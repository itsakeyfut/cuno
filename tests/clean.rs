//! `cuno clean` run as a process: what it removes, what it says and how it
//! exits (`docs/en/specs/commands.md`, "`cuno clean`"). It needs no
//! compiler, so the output is laid down by hand.

mod support;

use support::Project;

const MANIFEST: &str = "[package]\nname = \"app\"\nversion = \"0.1.0\"\n";

/// A project with three files of `dev` output and two of `release`.
fn built() -> Project {
    Project::new()
        .file("cuno.toml", MANIFEST)
        .file("src/main.cpp", "int main() {}\n")
        .file("target/debug/app.exe", "")
        .file("target/debug/obj/app/src/main.cpp.obj", "")
        .file("target/debug/obj/app/src/main.cpp.obj.cuno-state", "")
        .file("target/release/app.exe", "")
        .file("target/release/obj/app/src/main.cpp.obj", "")
}

/// `cuno clean` removes the whole output directory and counts the files;
/// a second run finds none; `-q` says nothing.
///
/// Mutation: leave the directory, count directories or only the top level,
/// fail on a directory that is not there, or print under `-q`; this test
/// fails.
#[test]
fn clean_removes_the_output_directory() {
    let project = built();
    project
        .cuno(&["clean"])
        .assert(0, "", "     Removed 5 files\n");
    assert!(!project.path().join("target").exists());
    assert!(project.path().join("src/main.cpp").is_file());
    project
        .cuno(&["clean"])
        .assert(0, "", "     Removed 0 files\n");
    built().cuno(&["-q", "clean"]).assert(0, "", "");
}

/// `--release` and `--profile` remove one profile's directory and leave
/// the other's; one file is a `file`.
///
/// Mutation: remove the other profile, the whole output directory, or say
/// `files` for one; this test fails.
#[test]
fn a_profile_removes_only_its_own_output() {
    let project = built();
    project
        .cuno(&["clean", "--release"])
        .assert(0, "", "     Removed 2 files\n");
    assert!(!project.path().join("target/release").exists());
    assert!(project.path().join("target/debug/app.exe").is_file());

    let project = Project::new()
        .file("cuno.toml", MANIFEST)
        .file("target/debug/app.exe", "")
        .file("target/release/app.exe", "");
    project
        .cuno(&["clean", "--profile", "dev"])
        .assert(0, "", "     Removed 1 file\n");
    assert!(!project.path().join("target/debug").exists());
    assert!(project.path().join("target/release/app.exe").is_file());
}

/// `--target-dir` names what is removed.
///
/// Mutation: ignore `--target-dir`; this test fails.
#[test]
fn target_dir_names_what_is_removed() {
    let project = built().file("out/debug/app.exe", "");
    project
        .cuno(&["clean", "--target-dir", "out"])
        .assert(0, "", "     Removed 1 file\n");
    assert!(!project.path().join("out").exists());
    assert!(project.path().join("target/debug/app.exe").is_file());
}

/// A project whose `cuno.toml` puts the output outside it, or at its root,
/// is refused with exit 3 before anything is removed, even when the
/// command line names another directory.
///
/// Mutation: remove before the settings are checked, or check only the
/// winning layer; this test fails.
#[test]
fn an_output_directory_that_could_hold_the_project_removes_nothing() {
    let project = built().file(
        "cuno.toml",
        &format!("{MANIFEST}[build]\ntarget-dir = \"../out\"\n"),
    );
    let outside = project.path().parent().unwrap().join("out");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("keep"), "").unwrap();
    let run = project.cuno(&["clean"]);
    assert_eq!(run.code, Some(3), "{}", run.stderr);
    assert!(
        run.stderr.contains("points outside the project"),
        "{}",
        run.stderr
    );
    assert!(outside.join("keep").is_file());
    assert!(project.path().join("target/debug/app.exe").is_file());

    let project = built().file(
        "cuno.toml",
        &format!("{MANIFEST}[build]\ntarget-dir = \".\"\n"),
    );
    let run = project.cuno(&["clean", "--target-dir", "target"]);
    assert_eq!(run.code, Some(3), "{}", run.stderr);
    assert!(project.path().join("cuno.toml").is_file());
    assert!(project.path().join("target/debug/app.exe").is_file());
}

/// Outside a project there is nothing to clean: the same error as `cuno
/// build`, exit 3, and nothing removed; an unknown profile is exit 3 too.
///
/// Mutation: clean the current directory's `target` outside a project, or
/// accept another profile; this test fails.
#[test]
fn clean_outside_a_project_or_with_an_unknown_profile_exits_3() {
    let outside = Project::new().file("target/x", "");
    outside.cuno(&["clean"]).assert(
        3,
        "",
        &format!(
            "error: could not find cuno.toml in {} or any parent directory\n\
             help: run it in a directory that holds a cuno.toml, or below one\n",
            outside.path().display()
        ),
    );
    assert!(outside.path().join("target/x").is_file());
    built().cuno(&["clean", "--profile", "bench"]).assert(
        3,
        "",
        "error: no profile named `bench`\nhelp: use dev or release\n",
    );
}
