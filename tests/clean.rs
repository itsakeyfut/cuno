//! `cuno clean` run as a process: what it removes, what it says and how it
//! exits (`docs/en/specs/commands.md`, "`cuno clean`"). It needs no
//! compiler, so the output is laid down by hand.

mod support;

use support::Project;

const MANIFEST: &str = "[package]\nname = \"app\"\nversion = \"0.1.0\"\n";

/// The marker a build leaves in the output directory it made.
const MARKER: &str = "target/.cuno-target";

/// A project with three files of `dev` output, two of `release` and the
/// marker.
fn built() -> Project {
    Project::new()
        .file("cuno.toml", MANIFEST)
        .file("src/main.cpp", "int main() {}\n")
        .file(MARKER, "")
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
        .assert(0, "", "     Removed 6 files\n");
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
        .file(MARKER, "")
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
    let project = built()
        .file("out/.cuno-target", "")
        .file("out/debug/app.exe", "");
    project
        .cuno(&["clean", "--target-dir", "out"])
        .assert(0, "", "     Removed 2 files\n");
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
fn a_refused_output_directory_removes_nothing() {
    let project = built().file(
        "cuno.toml",
        &format!("{MANIFEST}[build]\ntarget-dir = \"../out\"\n"),
    );
    let outside = project.path().parent().unwrap().join("out");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join(".cuno-target"), "").unwrap();
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

/// An output directory without the marker is not cuno's: `cuno clean`
/// removes nothing, says so, and exits 3, whether it is a `target` made by
/// hand or the sources a project's `cuno.toml` names.
///
/// Mutation: remove an unmarked directory; this test fails.
#[test]
fn an_unmarked_output_directory_is_not_removed() {
    let project = Project::new()
        .file("cuno.toml", MANIFEST)
        .file("target/notes.txt", "");
    let run = project.cuno(&["clean"]);
    assert_eq!(run.code, Some(3), "{}", run.stderr);
    assert!(
        run.stderr.ends_with(
            "was not made by cuno; not removing it\n\
             help: remove it yourself, or set target-dir to a directory cuno builds into\n"
        ),
        "{}",
        run.stderr
    );
    assert!(project.path().join("target/notes.txt").is_file());

    let project = Project::new()
        .file(
            "cuno.toml",
            &format!("{MANIFEST}[build]\ntarget-dir = \"src\"\n"),
        )
        .file("src/main.cpp", "int main() {}\n");
    let run = project.cuno(&["clean"]);
    assert_eq!(run.code, Some(3), "{}", run.stderr);
    assert!(project.path().join("src/main.cpp").is_file());
}

/// Outside a project there is nothing to clean: the same error as `cuno
/// build`, exit 3, and nothing removed, even with an output directory that
/// would be refused.
///
/// Mutation: clean the current directory's `target` outside a project, or
/// judge the settings before looking for the project; this test fails.
#[test]
fn clean_outside_a_project_exits_3() {
    let outside = Project::new()
        .file("target/x", "")
        .file("target/.cuno-target", "");
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
    let run = outside.cuno(&["clean", "--target-dir", "."]);
    assert!(
        run.stderr.starts_with("error: could not find cuno.toml"),
        "{}",
        run.stderr
    );
}

/// A profile cuno does not have is exit 3, and `--release` with
/// `--profile` is a usage error, exit 2; neither removes anything.
///
/// Mutation: accept another profile, or let `--release` win over
/// `--profile`; this test fails.
#[test]
fn a_profile_that_is_not_one_is_refused() {
    let project = built();
    let run = project.cuno(&["clean", "--release", "--profile", "bench"]);
    assert_eq!(run.code, Some(2), "{}", run.stderr);
    assert!(project.path().join("target/release/app.exe").is_file());
    project.cuno(&["clean", "--profile", "bench"]).assert(
        3,
        "",
        "error: no profile named `bench`\nhelp: use dev or release\n",
    );
    assert!(project.path().join("target/debug/app.exe").is_file());
}

/// What reading the settings warns about is shown by `cuno clean` too.
///
/// Mutation: drop the settings' warnings; this test fails.
#[test]
fn clean_shows_the_settings_warnings() {
    let project = built().file("cuno.toml", &format!("{MANIFEST}nope = 1\n"));
    let run = project.cuno(&["clean"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert!(
        run.stderr.starts_with("warning: unknown key"),
        "{}",
        run.stderr
    );
}

/// A file another program holds open stops the removal: exit 3, naming
/// what could not be removed.
///
/// Mutation: report a failed removal as success, or as another exit code;
/// this test fails.
#[cfg(windows)]
#[test]
fn a_file_in_use_fails_the_removal_with_exit_3() {
    use std::os::windows::fs::OpenOptionsExt;
    let project = built();
    let held = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(project.path().join("target/debug/app.exe"))
        .unwrap();
    let run = project.cuno(&["clean"]);
    drop(held);
    assert_eq!(run.code, Some(3), "{}", run.stderr);
    assert!(
        run.stderr.starts_with("error: could not remove "),
        "{}",
        run.stderr
    );
}
