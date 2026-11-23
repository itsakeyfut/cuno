use super::*;
use cuno_toolchain::HostOs;

/// A project at a temporary root with `files` (each empty) and a
/// `cuno.toml` of `[package]` plus `extra`.
fn project(lang: &str, extra: &str, files: &[&str]) -> (tempfile::TempDir, PathBuf, Manifest) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_owned();
    for file in files {
        let path = root.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "").unwrap();
    }
    let toml =
        format!("[package]\nname = \"app\"\nversion = \"0.1.0\"\nlang = \"{lang}\"\n{extra}");
    let manifest = cuno_manifest::parse_manifest(&toml, &root.join("cuno.toml"), &root)
        .unwrap()
        .value;
    (dir, root, manifest)
}

fn names(sources: &Sources) -> Vec<&str> {
    sources.bins.iter().map(|b| b.name.as_str()).collect()
}

fn body(root: &Path, sources: &Sources) -> Vec<(String, Lang)> {
    sources
        .body
        .iter()
        .map(|s| (shown(root, &s.path), s.lang))
        .collect()
}

/// Without `[[bin]]`: the default entry file named after the package, each
/// source directly in `src/bin/` named after its stem, and the body is the
/// rest of `src/`, each source in the language of its extension.
///
/// Mutation: leave `src/main.*` or anything under `src/bin/` in the body,
/// detect `src/bin/` recursively, name a bin after its directory, or give
/// a source the package's language; this test fails.
#[test]
fn without_bin_the_default_and_src_bin_are_the_executables() {
    let (_d, root, m) = project(
        "c++",
        "",
        &[
            "src/main.cpp",
            "src/main.c",
            "src/bin/tool.cpp",
            "src/bin/nested/helper.cpp",
            "src/util.cpp",
            "src/a/legacy.c",
            "src/notes.txt",
        ],
    );
    let s = classify(&root, &m, &root.join("target"), HostOs::Windows).unwrap();
    assert_eq!(names(&s), ["app", "tool"]);
    assert_eq!(s.bins[0].entry.path, root.join("src/main.cpp"));
    assert_eq!(s.bins[1].entry.lang, Lang::Cxx);
    assert_eq!(
        body(&root, &s),
        [
            ("src/a/legacy.c".to_owned(), Lang::C),
            ("src/util.cpp".to_owned(), Lang::Cxx)
        ]
    );
    assert!(s.warnings.is_empty());
}

/// A C package's default entry file is `src/main.c`.
///
/// Mutation: look for `src/main.cpp` whatever the package's language; this
/// test fails.
#[test]
fn a_c_package_starts_at_main_c() {
    let (_d, root, m) = project("c", "", &["src/main.c", "src/util.c"]);
    let s = classify(&root, &m, &root.join("target"), HostOs::Windows).unwrap();
    assert_eq!(
        s.bins[0].entry,
        Source {
            path: root.join("src/main.c"),
            lang: Lang::C
        }
    );
    assert_eq!(body(&root, &s), [("src/util.c".to_owned(), Lang::C)]);
}

/// With `[[bin]]`: only what it lists is built, its entry files leave the
/// body, and each unlisted `src/main.*` / `src/bin/` source is warned about.
///
/// Mutation: keep detecting with `[[bin]]` written, leave a listed entry
/// in the body, or warn about nothing; this test fails.
#[test]
fn bin_lists_the_executables_and_warns_about_the_rest() {
    let (_d, root, m) = project(
        "c++",
        "[[bin]]\nname = \"game\"\npath = \"src/game.cpp\"\nsubsystem = \"windows\"\n",
        &[
            "src/main.cpp",
            "src/bin/tool.cpp",
            "src/game.cpp",
            "src/util.cpp",
        ],
    );
    let s = classify(&root, &m, &root.join("target"), HostOs::Windows).unwrap();
    assert_eq!(names(&s), ["game"]);
    assert_eq!(s.bins[0].subsystem, Subsystem::Windows);
    assert_eq!(body(&root, &s), [("src/util.cpp".to_owned(), Lang::Cxx)]);
    let warned: Vec<String> = s.warnings.iter().map(ToString::to_string).collect();
    assert_eq!(
        warned,
        [
            "src/bin/tool.cpp is not built: [[bin]] is written and does not list it",
            "src/main.cpp is not built: [[bin]] is written and does not list it"
        ]
    );
}

/// A target directory under `src/` is not read as sources.
///
/// Mutation: walk into the target directory; this test fails.
#[test]
fn the_target_directory_is_not_a_source() {
    let (_d, root, m) = project("c++", "", &["src/main.cpp", "src/out/stale.cpp"]);
    let s = classify(&root, &m, &root.join("src/out"), HostOs::Windows).unwrap();
    assert!(s.body.is_empty(), "{:?}", s.body);
}

/// Mutation: accept any of these layouts, or report them as another
/// error; this test fails.
#[test]
fn layouts_that_cannot_be_built_are_errors() {
    let message = |lang: &str, extra: &str, files: &[&str]| {
        let (_d, root, m) = project(lang, extra, files);
        let error = classify(&root, &m, &root.join("target"), HostOs::Windows).unwrap_err();
        (error.to_string(), cuno_core::CunoError::help(&error))
    };
    assert_eq!(
        message("c++", "", &["src/util.cpp"]),
        (
            "no executable to build".to_owned(),
            Some("add src/main.cpp, or list one in [[bin]]".to_owned())
        )
    );
    assert_eq!(
        message("c", "", &["src/util.c"]).1,
        Some("add src/main.c, or list one in [[bin]]".to_owned())
    );
    assert_eq!(
        message("c++", "[[bin]]\nname = \"x\"\npath = \"src/x.cpp\"\n", &[]).0,
        "[[bin]] `x` is src/x.cpp, but there is no file there"
    );
    assert!(
        message("c++", "[[bin]]\nname = \"x\"\npath = \"../x.cpp\"\n", &[])
            .0
            .starts_with("the entry file of `x` is outside the project: ")
    );
    assert_eq!(
        message("c++", "", &["src/main.cpp", "src/bin/app.cpp"]).0,
        "two executables are named `app`: src/main.cpp and src/bin/app.cpp"
    );
}

/// Mutation: change the language any extension gives, or accept a
/// non-source extension; this test fails.
#[test]
fn the_extension_gives_the_language() {
    for (file, lang) in [
        ("a.c", Some(Lang::C)),
        ("a.cpp", Some(Lang::Cxx)),
        ("a.cc", Some(Lang::Cxx)),
        ("a.cxx", Some(Lang::Cxx)),
        ("a.CPP", Some(Lang::Cxx)),
        ("a.h", None),
        ("a.hpp", None),
        ("a", None),
    ] {
        assert_eq!(language(Path::new(file)), lang, "{file}");
    }
}

/// On Windows one file is one source whatever its spelling: `src/Main.cpp`
/// is the default entry and leaves the body, a `[[bin]] path` written in
/// another case is the file found and leaves the body, and a target
/// directory spelled in another case is still skipped.
///
/// Mutation: compare any of these paths with case; this test fails.
#[test]
fn one_file_is_one_source_in_any_case() {
    let (_d, root, m) = project("c++", "", &["src/Main.cpp", "src/util.cpp"]);
    let s = classify(&root, &m, &root.join("target"), HostOs::Windows).unwrap();
    assert_eq!(s.bins[0].entry.path, root.join("src/Main.cpp"));
    assert_eq!(body(&root, &s), [("src/util.cpp".to_owned(), Lang::Cxx)]);

    let (_d, root, m) = project(
        "c++",
        "[[bin]]\nname = \"tool\"\npath = \"SRC/Tool.cpp\"\n[[bin]]\nname = \"app\"\npath = \"SRC/MAIN.cpp\"\n",
        &["src/tool.cpp", "src/main.cpp", "src/util.cpp"],
    );
    let s = classify(&root, &m, &root.join("target"), HostOs::Windows).unwrap();
    assert_eq!(s.bins[0].entry.path, root.join("src/tool.cpp"));
    assert_eq!(body(&root, &s), [("src/util.cpp".to_owned(), Lang::Cxx)]);
    assert!(s.warnings.is_empty(), "{:?}", s.warnings);

    let (_d, root, m) = project("c++", "", &["src/main.cpp", "src/out/stale.cpp"]);
    let s = classify(&root, &m, &root.join("SRC/OUT"), HostOs::Windows).unwrap();
    assert!(s.body.is_empty(), "{:?}", s.body);
}

/// Executable names that differ only in case are the same file on Windows,
/// and a duplicate among `[[bin]]` entries is one too.
///
/// Mutation: compare names with case, or check only auto-detected names;
/// this test fails.
#[test]
fn names_that_are_one_file_are_duplicates() {
    let (_d, root, m) = project("c++", "", &["src/main.cpp", "src/bin/APP.cpp"]);
    let error = classify(&root, &m, &root.join("target"), HostOs::Windows).unwrap_err();
    assert_eq!(
        error.to_string(),
        "two executables are named `APP`: src/main.cpp and src/bin/APP.cpp"
    );
    let (_d, root, m) = project(
        "c++",
        "[[bin]]\nname = \"x\"\npath = \"src/a.cpp\"\n[[bin]]\nname = \"x\"\npath = \"src/b.cpp\"\n",
        &["src/a.cpp", "src/b.cpp"],
    );
    let error = classify(&root, &m, &root.join("target"), HostOs::Windows).unwrap_err();
    assert_eq!(
        error.to_string(),
        "two executables are named `x`: src/a.cpp and src/b.cpp"
    );
}

/// Every executable cuno detects starts as a console program, and so does
/// a `[[bin]]` that names no subsystem; a listed `src/main.*` / `src/bin/`
/// entry is not warned about.
///
/// Mutation: default any executable to the windows subsystem, or warn about
/// a listed entry; this test fails.
#[test]
fn detected_and_unspecified_executables_are_console_programs() {
    let (_d, root, m) = project("c++", "", &["src/main.cpp", "src/bin/tool.cpp"]);
    let s = classify(&root, &m, &root.join("target"), HostOs::Windows).unwrap();
    assert!(
        s.bins.iter().all(|b| b.subsystem == Subsystem::Console),
        "{:?}",
        s.bins
    );

    let (_d, root, m) = project(
        "c++",
        "[[bin]]\nname = \"app\"\npath = \"src/main.cpp\"\n[[bin]]\nname = \"tool\"\npath = \"src/bin/tool.cpp\"\n",
        &["src/main.cpp", "src/bin/tool.cpp"],
    );
    let s = classify(&root, &m, &root.join("target"), HostOs::Windows).unwrap();
    assert!(
        s.bins.iter().all(|b| b.subsystem == Subsystem::Console),
        "{:?}",
        s.bins
    );
    assert!(s.warnings.is_empty(), "{:?}", s.warnings);
}

/// The body is in path order whatever order the files were made in.
///
/// Mutation: keep the order the directory listing gives; this test fails
/// where the listing is not sorted.
#[test]
fn the_body_is_in_path_order() {
    let (_d, root, m) = project(
        "c++",
        "",
        &[
            "src/main.cpp",
            "src/z.cpp",
            "src/b/y.cpp",
            "src/a.cpp",
            "src/M.cpp",
        ],
    );
    let s = classify(&root, &m, &root.join("target"), HostOs::Windows).unwrap();
    let order: Vec<String> = body(&root, &s).into_iter().map(|(p, _)| p).collect();
    assert_eq!(
        order,
        ["src/a.cpp", "src/b/y.cpp", "src/M.cpp", "src/z.cpp"]
    );
}

/// A `[[bin]] path` that is not a C or C++ source is an error.
///
/// Mutation: give it the package's language instead; this test fails.
#[test]
fn a_bin_path_must_be_a_source() {
    let (_d, root, m) = project(
        "c++",
        "[[bin]]\nname = \"x\"\npath = \"README.md\"\n",
        &["README.md"],
    );
    let error = classify(&root, &m, &root.join("target"), HostOs::Windows).unwrap_err();
    assert_eq!(
        error.to_string(),
        "[[bin]] `x` is README.md, which is not a C or C++ source"
    );
}

/// A link to a directory under `src/` is warned about and not read.
///
/// Mutation: follow it, or skip it without a warning; this test fails.
#[cfg(windows)]
#[test]
fn a_directory_link_is_warned_about_and_not_read() {
    let (dir, root, m) = project("c++", "", &["src/main.cpp", "ext/lib.cpp"]);
    let made = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(root.join("src").join("lib"))
        .arg(root.join("ext"))
        .output()
        .unwrap();
    assert!(made.status.success(), "{made:?}");
    let s = classify(&root, &m, &root.join("target"), HostOs::Windows).unwrap();
    assert!(s.body.is_empty(), "{:?}", s.body);
    let warned: Vec<String> = s.warnings.iter().map(ToString::to_string).collect();
    assert_eq!(warned, ["src/lib is not read: it is a link to a directory"]);
    drop(dir);
}

/// Every layout error is a setup problem with help on how to fix it, except
/// a directory that cannot be read, whose reason is the help.
///
/// Mutation: change any error's exit class, or drop any help; this test
/// fails.
#[test]
fn layout_errors_exit_3_with_help() {
    use cuno_core::{CunoError, ExitClass};
    let errors = [
        BuildError::NoExecutable {
            default: "src/main.cpp".into(),
        },
        BuildError::BinPathMissing {
            name: "x".into(),
            path: "src/x.cpp".into(),
        },
        BuildError::NotASource {
            name: "x".into(),
            path: "x.txt".into(),
        },
        BuildError::OutsideProject {
            name: "x".into(),
            path: "D:/x.cpp".into(),
        },
        BuildError::DuplicateName {
            name: "x".into(),
            entries: ["a".into(), "b".into()],
        },
        BuildError::Unreadable {
            dir: "src".into(),
            reason: "denied".into(),
        },
    ];
    let helps: Vec<Option<String>> = errors.iter().map(|e| e.help()).collect();
    assert_eq!(
        helps,
        [
            Some("add src/main.cpp, or list one in [[bin]]".to_owned()),
            Some("correct the path of [[bin]] `x`".to_owned()),
            Some("correct the path of [[bin]] `x`".to_owned()),
            Some("move it into the project, or point [[bin]] path at a file inside it".to_owned()),
            Some("give one of them another name with [[bin]] name".to_owned()),
            None,
        ]
    );
    for error in &errors {
        assert_eq!(error.exit_class(), ExitClass::Setup, "{error}");
    }
    assert_eq!(errors[5].to_string(), "cannot read src: denied");
}

/// Makes `link` a junction to `target`, which needs no privilege on Windows.
#[cfg(windows)]
fn junction(link: &Path, target: &Path) {
    let made = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .unwrap();
    assert!(made.status.success(), "{made:?}");
}

/// The target directory named through a junction to it is still not read
/// as sources.
///
/// Mutation: compare the directories walked with the target directory as
/// written, without following links; this test fails.
#[cfg(windows)]
#[test]
fn the_target_directory_through_a_junction_is_not_a_source() {
    let (_d, root, m) = project("c++", "", &["src/main.cpp", "src/out/stale.cpp"]);
    junction(&root.join("alias"), &root.join("src").join("out"));
    let s = classify(&root, &m, &root.join("alias"), HostOs::Windows).unwrap();
    assert!(s.body.is_empty(), "{:?}", s.body);
}
