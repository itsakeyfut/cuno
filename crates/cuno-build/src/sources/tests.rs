use super::*;

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
    let s = classify(&root, &m, &root.join("target")).unwrap();
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
    let s = classify(&root, &m, &root.join("target")).unwrap();
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
    let s = classify(&root, &m, &root.join("target")).unwrap();
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
    let s = classify(&root, &m, &root.join("src/out")).unwrap();
    assert!(s.body.is_empty(), "{:?}", s.body);
}

/// Mutation: accept any of these layouts, or report them as another
/// error; this test fails.
#[test]
fn layouts_that_cannot_be_built_are_errors() {
    let message = |lang: &str, extra: &str, files: &[&str]| {
        let (_d, root, m) = project(lang, extra, files);
        let error = classify(&root, &m, &root.join("target")).unwrap_err();
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
