//! Which files of a package are compiled, and into what: its executables'
//! entry files, the package body, and its integration tests and their
//! helpers under `tests/`, each source with its language
//! (`docs/en/specs/project-layout.md`, "Source collection and linking"), and
//! the test executables made from them.

use std::fmt;
use std::path::{Path, PathBuf};

use cuno_core::paths::{self, Place};

use cuno_core::NameCase;
use cuno_manifest::Manifest;
use cuno_toolchain::HostOs;
use cuno_toolchain::flags::{Lang, Subsystem};

use crate::BuildError;

/// A source to compile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    /// Its absolute path, spelled as found under `src/` or `tests/`.
    pub path: PathBuf,
    /// The language its extension gives.
    pub lang: Lang,
}

/// An executable: its name, its entry file and how it starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bin {
    /// The executable's name, without an extension.
    pub name: String,
    /// The file with its `main`.
    pub entry: Source,
    /// `[[bin]] subsystem`.
    pub subsystem: Subsystem,
}

/// A package's sources, classified.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sources {
    /// The executables, in the order `[[bin]]` lists them, or the default
    /// one first and then `src/bin/` in name order.
    pub bins: Vec<Bin>,
    /// The package body: every source under `src/` that is not an entry
    /// file, `src/main.*` or under `src/bin/`, in path order.
    pub body: Vec<Source>,
    /// The integration tests: the sources directly under `tests/`, each a
    /// test executable of its own, in path order
    /// (`docs/en/specs/testing.md`, "Execution").
    pub integration_tests: Vec<Source>,
    /// The sources in `tests/`'s subdirectories: helpers linked into every
    /// executable made from `tests/`, in path order.
    pub test_helpers: Vec<Source>,
    /// What is not built and why.
    pub warnings: Vec<SourceWarning>,
}

/// Something under `src/` that is not built, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceWarning {
    /// The file or directory, as shown (`src/bin/tool.cpp`).
    pub path: String,
    /// Why it is not built.
    pub why: NotBuilt,
}

/// Why something under `src/` is not built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotBuilt {
    /// A `src/main.*` or `src/bin/` source a written `[[bin]]` does not list.
    NotListed,
    /// A link to a directory, which is not read.
    DirectoryLink,
}

impl fmt::Display for SourceWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.why {
            NotBuilt::NotListed => write!(
                f,
                "{} is not built: [[bin]] is written and does not list it",
                self.path
            ),
            NotBuilt::DirectoryLink => {
                write!(f, "{} is not read: it is a link to a directory", self.path)
            }
        }
    }
}

/// The language of a source with this extension, or `None` for a file that
/// is not a C or C++ source.
pub fn language(path: &Path) -> Option<Lang> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "c" => Some(Lang::C),
        "cpp" | "cc" | "cxx" => Some(Lang::Cxx),
        _ => None,
    }
}

/// Classifies the sources of the package at `root`: the entry files and the
/// body under `src/`, and the integration tests and their helpers under
/// `tests/`, both walked alike, skipping `target_dir` when it lies under
/// either and warning about links to directories. On a Windows `host`, paths
/// and names are compared without regard to case, as its file systems do
/// (`project-layout.md`, "Entry files and the package body").
pub fn classify(
    root: &Path,
    manifest: &Manifest,
    target_dir: &Path,
    host: HostOs,
) -> Result<Sources, BuildError> {
    let case = host.name_case();
    let root = paths::lexical(root);
    let src = root.join("src");
    let bin_dir = src.join("bin");
    let (root_place, src_place) = (Place::of(&root, case), Place::of(&src, case));
    let bin_place = Place::of(&bin_dir, case);
    // `src/` is the project's own even when it is a link that leads out of
    // it (`project-layout.md`, "Source collection and linking").
    let in_project = |p: &Place| p.within(&root_place) || p.within(&src_place);
    let mut found = Vec::new();
    let mut warnings = Vec::new();
    if src.is_dir() {
        Walk {
            root: &root,
            target_dir: Place::of(target_dir, case),
            case,
            found: &mut found,
            warnings: &mut warnings,
        }
        .dir(&src)?;
    }
    let tests_dir = root.join("tests");
    let mut found_tests = Vec::new();
    if tests_dir.is_dir() {
        Walk {
            root: &root,
            target_dir: Place::of(target_dir, case),
            case,
            found: &mut found_tests,
            warnings: &mut warnings,
        }
        .dir(&tests_dir)?;
    }
    found_tests.sort_by_cached_key(|p| case.fold(p.as_os_str()));
    let tests_place = Place::of(&tests_dir, case);
    let (integration_tests, test_helpers): (Vec<Source>, Vec<Source>) = {
        let mut direct = Vec::new();
        let mut helpers = Vec::new();
        for path in found_tests {
            let lang = language(&path).expect("the walk keeps C and C++ sources only");
            let place = Place::of(&path, case);
            let source = Source { path, lang };
            if place
                .parent()
                .is_some_and(|parent| parent.same(&tests_place))
            {
                direct.push(source);
            } else {
                helpers.push(source);
            }
        }
        (direct, helpers)
    };

    // Each path is resolved once: resolving asks the file system.
    found.sort_by_cached_key(|p| case.fold(p.as_os_str()));
    let found: Vec<(PathBuf, Place)> = found
        .into_iter()
        .map(|p| {
            let place = Place::of(&p, case);
            (p, place)
        })
        .collect();
    let in_dir = |p: &Place, dir: &Place| p.parent().is_some_and(|parent| parent.same(dir));
    // A source the file system names `src/main.*`, compared as a place so
    // that a name it holds is spelled as it spells it.
    let is_main = |path: &Path, place: &Place| {
        in_dir(place, &src_place)
            && path.extension().is_some_and(|ext| {
                place.same(&Place::of(&src.join("main").with_extension(ext), case))
            })
    };
    let is_in_bin = |p: &Place| p.within(&bin_place);
    // The spelling the search found, so one file is one path everywhere.
    let as_found = |p: &Place| {
        found
            .iter()
            .find(|(_, f)| f.same(p))
            .map(|(f, _)| f.clone())
    };

    let mut entries: Vec<Place> = Vec::new();
    let bins = match &manifest.bins {
        Some(listed) => {
            let mut bins = Vec::new();
            for bin in listed {
                let path = paths::lexical(&bin.path);
                let place = Place::of(&path, case);
                if !in_project(&place) {
                    return Err(BuildError::OutsideProject {
                        name: bin.name.clone(),
                        path,
                    });
                }
                if !path.is_file() {
                    return Err(BuildError::BinPathMissing {
                        name: bin.name.clone(),
                        path: shown(&root, &path),
                    });
                }
                let Some(lang) = language(&path) else {
                    return Err(BuildError::NotASource {
                        name: bin.name.clone(),
                        path: shown(&root, &path),
                    });
                };
                bins.push(Bin {
                    name: bin.name.clone(),
                    entry: Source {
                        path: as_found(&place).unwrap_or(path),
                        lang,
                    },
                    subsystem: bin.subsystem,
                });
                entries.push(place);
            }
            for (path, place) in &found {
                let listed = entries.iter().any(|e| e.same(place));
                if (is_main(path, place) || is_in_bin(place)) && !listed {
                    warnings.push(SourceWarning {
                        path: shown(&root, path),
                        why: NotBuilt::NotListed,
                    });
                }
            }
            bins
        }
        None => {
            let mut bins = Vec::new();
            let lang = manifest.package.lang;
            let default = Place::of(&src.join(default_entry(lang)), case);
            if let Some(path) = as_found(&default) {
                bins.push(Bin {
                    name: manifest.package.name.clone(),
                    entry: Source { path, lang },
                    subsystem: Subsystem::Console,
                });
                entries.push(default);
            }
            for (path, place) in found.iter().filter(|(_, p)| in_dir(p, &bin_place)) {
                let name = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                bins.push(Bin {
                    name,
                    entry: Source {
                        lang: language(path).unwrap_or(lang),
                        path: path.clone(),
                    },
                    subsystem: Subsystem::Console,
                });
                entries.push(place.clone());
            }
            bins
        }
    };
    if bins.is_empty() {
        return Err(BuildError::NoExecutable {
            default: format!("src/{}", default_entry(manifest.package.lang)),
        });
    }
    for (i, bin) in bins.iter().enumerate() {
        if let Some(other) = bins[..i].iter().find(|b| case.same(&b.name, &bin.name)) {
            return Err(BuildError::DuplicateName {
                name: bin.name.clone(),
                entries: [
                    shown(&root, &other.entry.path),
                    shown(&root, &bin.entry.path),
                ],
            });
        }
    }

    let body = found
        .into_iter()
        .filter(|(path, place)| {
            !is_main(path, place) && !is_in_bin(place) && !entries.iter().any(|e| e.same(place))
        })
        .filter_map(|(path, _)| {
            let lang = language(&path)?;
            Some(Source { path, lang })
        })
        .collect();
    Ok(Sources {
        bins,
        body,
        integration_tests,
        test_helpers,
        warnings,
    })
}

/// A test executable: the package's in-source tests, or one source
/// directly under `tests/` (`docs/en/specs/testing.md`, "Execution").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestExecutable {
    /// Its name, without an extension: the package's for its in-source
    /// tests, `tests-<stem>` for a source under `tests/`
    /// (`docs/en/specs/build.md`, "Output directory").
    pub name: String,
    /// The source under `tests/` it is made from; `None` for the in-source
    /// tests.
    pub file: Option<Source>,
}

/// The test executables of `package`, whose sources at `root` are
/// `sources`: one for its in-source tests, then one per source directly
/// under `tests/`. Two of one name, compared without case on a Windows
/// `host`, are an error naming both: one link would overwrite the other
/// and its tests never run (`build.md`, "Output directory").
pub fn test_executables(
    root: &Path,
    package: &str,
    sources: &Sources,
    host: HostOs,
) -> Result<Vec<TestExecutable>, BuildError> {
    let case = host.name_case();
    let mut exes = vec![TestExecutable {
        name: package.to_owned(),
        file: None,
    }];
    for source in &sources.integration_tests {
        let stem = source
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        exes.push(TestExecutable {
            name: format!("tests-{stem}"),
            file: Some(source.clone()),
        });
    }
    let made_from = |exe: &TestExecutable| match &exe.file {
        Some(file) => shown(&paths::lexical(root), &file.path),
        None => "src/".to_owned(),
    };
    for (i, exe) in exes.iter().enumerate() {
        if let Some(other) = exes[..i].iter().find(|e| case.same(&e.name, &exe.name)) {
            return Err(BuildError::DuplicateTestName {
                name: exe.name.clone(),
                sources: [made_from(other), made_from(exe)],
            });
        }
    }
    Ok(exes)
}

/// The default entry file's name for a package in `lang`.
fn default_entry(lang: Lang) -> &'static str {
    match lang {
        Lang::Cxx => "main.cpp",
        Lang::C => "main.c",
    }
}

/// The search of `src/` for sources.
struct Walk<'a> {
    root: &'a Path,
    target_dir: Place,
    case: NameCase,
    found: &'a mut Vec<PathBuf>,
    warnings: &'a mut Vec<SourceWarning>,
}

impl Walk<'_> {
    /// Every C or C++ source under `dir`, skipping the target directory. A
    /// link to a directory is warned about and not followed, so a link
    /// cycle cannot make this run forever and no file outside the project
    /// joins the build unannounced.
    fn dir(&mut self, dir: &Path) -> Result<(), BuildError> {
        if Place::of(dir, self.case).same(&self.target_dir) {
            return Ok(());
        }
        let unreadable = |e: std::io::Error| BuildError::Unreadable {
            dir: shown(self.root, dir),
            reason: e.to_string(),
        };
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(dir).map_err(unreadable)? {
            let entry = entry.map_err(unreadable)?;
            entries.push((entry.path(), entry.file_type().map_err(unreadable)?));
        }
        for (path, kind) in entries {
            if kind.is_symlink() && path.is_dir() {
                self.warnings.push(SourceWarning {
                    path: shown(self.root, &path),
                    why: NotBuilt::DirectoryLink,
                });
            } else if kind.is_dir() {
                self.dir(&path)?;
            } else if path.is_file() && language(&path).is_some() {
                self.found.push(path);
            }
        }
        Ok(())
    }
}

/// `path` relative to `root` with `/` separators, as messages show it.
fn shown(root: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
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
    /// Mutation: compare names with case, or without case on every host, or
    /// check only auto-detected names; this test fails.
    #[test]
    fn names_that_are_one_file_are_duplicates() {
        let (_d, root, m) = project("c++", "", &["src/main.cpp", "src/bin/APP.cpp"]);
        let error = classify(&root, &m, &root.join("target"), HostOs::Windows).unwrap_err();
        assert_eq!(
            error.to_string(),
            "two executables are named `APP`: src/main.cpp and src/bin/APP.cpp"
        );
        // Elsewhere, `app` and `APP` are two executables.
        let s = classify(&root, &m, &root.join("target"), HostOs::Other).unwrap();
        assert_eq!(names(&s), ["app", "APP"]);
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
                Some(
                    "move it into the project, or point [[bin]] path at a file inside it"
                        .to_owned()
                ),
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

    /// On Windows, `src/Main.cpp` is a `main` entry file too: with `[[bin]]`
    /// written and not listing it, it is warned about rather than put in the
    /// body (`project-layout.md`, "Source collection and linking").
    ///
    /// Mutation: compare the entry file's name with case; this test fails.
    #[test]
    fn an_unlisted_main_in_another_case_is_warned_about() {
        let (_d, root, m) = project(
            "c++",
            "[[bin]]
name = \"game\"
path = \"src/game.cpp\"
",
            &["src/Main.cpp", "src/game.cpp"],
        );
        let s = classify(&root, &m, &root.join("target"), HostOs::Windows).unwrap();
        assert!(s.body.is_empty(), "{:?}", s.body);
        assert_eq!(s.warnings.len(), 1, "{:?}", s.warnings);
    }

    /// A `src/` that is a junction leading out of the project is still the
    /// project's: its sources are found, a `[[bin]] path` into it is inside
    /// (`project-layout.md`, "Source collection and linking").
    ///
    /// Mutation: judge `src/` by where it leads like any other directory; this
    /// test fails.
    #[cfg(windows)]
    #[test]
    fn a_src_linked_from_outside_is_the_projects() {
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("main.cpp"), "").unwrap();
        std::fs::write(outside.path().join("util.cpp"), "").unwrap();
        let (_d, root, m) = project("c++", "", &[]);
        junction(&root.join("src"), outside.path());
        let s = classify(&root, &m, &root.join("target"), HostOs::Windows).unwrap();
        assert_eq!(names(&s), ["app"]);
        assert_eq!(body(&root, &s), [("src/util.cpp".to_owned(), Lang::Cxx)]);
        let (_e, root, m) = project(
            "c++",
            "[[bin]]
name = \"x\"
path = \"src/main.cpp\"
",
            &[],
        );
        junction(&root.join("src"), outside.path());
        let s = classify(&root, &m, &root.join("target"), HostOs::Windows).unwrap();
        assert_eq!(names(&s), ["x"]);
    }

    /// Sources directly under `tests/` are tests, each its own executable;
    /// those in its subdirectories are helpers; anything else there is not
    /// a source (`testing.md`, "Execution").
    ///
    /// Mutation: take `tests/` recursively as tests, drop the helpers, or
    /// read files that are not sources; this test fails.
    #[test]
    fn tests_and_their_helpers_are_told_apart() {
        let (_dir, root, manifest) = project(
            "c++",
            "",
            &[
                "src/main.cpp",
                "tests/b.c",
                "tests/a.cpp",
                "tests/README.md",
                "tests/common/util.cpp",
                "tests/common/deep/x.c",
            ],
        );
        let sources = classify(&root, &manifest, &root.join("target"), HostOs::Windows).unwrap();
        let shown_all = |list: &[Source]| -> Vec<(String, Lang)> {
            list.iter()
                .map(|s| (shown(&root, &s.path), s.lang))
                .collect()
        };
        assert_eq!(
            shown_all(&sources.integration_tests),
            [
                ("tests/a.cpp".to_owned(), Lang::Cxx),
                ("tests/b.c".to_owned(), Lang::C)
            ]
        );
        assert_eq!(
            shown_all(&sources.test_helpers),
            [
                ("tests/common/deep/x.c".to_owned(), Lang::C),
                ("tests/common/util.cpp".to_owned(), Lang::Cxx)
            ]
        );
        assert!(sources.body.is_empty());
    }

    fn with_tests(files: &[&str]) -> Sources {
        Sources {
            bins: Vec::new(),
            body: Vec::new(),
            integration_tests: files
                .iter()
                .map(|f| Source {
                    path: Path::new("C:/p").join(f),
                    lang: language(Path::new(f)).unwrap(),
                })
                .collect(),
            test_helpers: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn names_of(exes: &[TestExecutable]) -> Vec<&str> {
        exes.iter().map(|e| e.name.as_str()).collect()
    }

    /// The in-source tests are named after the package and each source
    /// under `tests/` after its stem behind `tests-`; two of one name are
    /// an error naming both, without regard to case on Windows (`build.md`,
    /// "Output directory").
    ///
    /// Mutation: drop the `tests-` prefix, skip the duplicate check, compare
    /// names with case on Windows or without it elsewhere, or show the
    /// in-source tests as anything but `src/`; this test fails.
    #[test]
    fn test_executables_are_named_and_never_share_a_name() {
        let root = Path::new("C:/p");
        let exes =
            test_executables(root, "app", &with_tests(&["tests/a.cpp"]), HostOs::Windows).unwrap();
        assert_eq!(names_of(&exes), ["app", "tests-a"]);
        assert_eq!(exes[0].file, None);
        assert_eq!(
            exes[1].file.as_ref().map(|f| f.path.clone()),
            Some(PathBuf::from("C:/p/tests/a.cpp"))
        );

        let refused = |package: &str, files: &[&str], host| match test_executables(
            root,
            package,
            &with_tests(files),
            host,
        ) {
            Err(BuildError::DuplicateTestName { name, sources }) => Some((name, sources)),
            Err(other) => panic!("{other}"),
            Ok(_) => None,
        };
        assert_eq!(
            refused("app", &["tests/foo.c", "tests/foo.cpp"], HostOs::Windows),
            Some((
                "tests-foo".to_owned(),
                ["tests/foo.c".to_owned(), "tests/foo.cpp".to_owned()]
            ))
        );
        assert_eq!(
            refused("tests-foo", &["tests/foo.cpp"], HostOs::Windows),
            Some((
                "tests-foo".to_owned(),
                ["src/".to_owned(), "tests/foo.cpp".to_owned()]
            ))
        );
        assert!(refused("app", &["tests/Foo.c", "tests/foo.cpp"], HostOs::Windows).is_some());
        assert_eq!(
            refused("app", &["tests/Foo.c", "tests/foo.cpp"], HostOs::Other),
            None
        );
    }

    /// The integration tests are in path order without regard to case, as
    /// the body is, whatever order the file system lists them in.
    ///
    /// Mutation: sort `tests/` with case, or not at all; this test fails.
    #[test]
    fn integration_tests_are_in_path_order_without_case() {
        let (_dir, root, manifest) = project(
            "c++",
            "",
            &["src/main.cpp", "tests/B.c", "tests/a.cpp", "tests/c.cpp"],
        );
        let sources = classify(&root, &manifest, &root.join("target"), HostOs::Windows).unwrap();
        let order: Vec<String> = sources
            .integration_tests
            .iter()
            .map(|s| shown(&root, &s.path))
            .collect();
        assert_eq!(order, ["tests/a.cpp", "tests/B.c", "tests/c.cpp"]);
    }

    /// A link to a directory inside `tests/` is warned about and not read,
    /// as one in `src/` is (`project-layout.md`, "Source collection and
    /// linking").
    ///
    /// Mutation: drop the warnings of the `tests/` walk; this test fails.
    #[cfg(windows)]
    #[test]
    fn a_directory_link_in_tests_is_warned_about() {
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("shared.cpp"), "").unwrap();
        let (_d, root, m) = project("c++", "", &["src/main.cpp", "tests/t.cpp"]);
        junction(&root.join("tests").join("shared"), outside.path());
        let s = classify(&root, &m, &root.join("target"), HostOs::Windows).unwrap();
        assert!(s.test_helpers.is_empty(), "{:?}", s.test_helpers);
        assert_eq!(
            s.warnings,
            [SourceWarning {
                path: "tests/shared".to_owned(),
                why: NotBuilt::DirectoryLink,
            }]
        );
    }
}
