//! `cuno new`: a new package, ready to build, test and run
//! (`docs/en/specs/commands.md`, "`cuno new <path>`").

use std::fmt;
use std::path::{Path, PathBuf};

use cuno_core::{CunoError, ExitClass, Shell};
use cuno_toolchain::flags::{Edition, Lang};

/// The options of `cuno new`.
#[derive(Debug)]
pub struct NewArgs {
    /// Where to make the package.
    pub path: PathBuf,
    /// `--lang`.
    pub lang: Option<Lang>,
    /// `--edition`.
    pub edition: Option<Edition>,
    /// `--name`.
    pub name: Option<String>,
    /// `--vcs`.
    pub vcs: Vcs,
}

/// `--vcs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vcs {
    /// A Git repository with a `.gitignore`.
    Git,
    /// No version control.
    None,
}

/// What stops `cuno new`. Every one is exit code 2 but a file that cannot
/// be written, which is the setup's (`commands.md`, "`cuno new <path>`").
#[derive(Debug)]
pub enum NewError {
    /// A package name the rule does not allow.
    InvalidName {
        /// The name.
        name: String,
        /// Whether it came from the directory's name rather than `--name`.
        from_directory: bool,
    },
    /// The destination is already there.
    Exists {
        /// It, as given.
        path: PathBuf,
    },
    /// `--edition` is not a standard of `--lang`.
    EditionMismatch {
        /// The edition.
        edition: Edition,
        /// The language.
        lang: Lang,
    },
    /// A file or directory that could not be made.
    Write {
        /// It.
        path: PathBuf,
        /// Why.
        reason: String,
    },
}

fn lang_name(lang: Lang) -> &'static str {
    match lang {
        Lang::C => "C",
        Lang::Cxx => "C++",
    }
}

impl fmt::Display for NewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NewError::InvalidName { name, .. } => {
                write!(f, "`{name}` is not a valid package name")
            }
            NewError::Exists { path } => write!(f, "`{}` already exists", path.display()),
            NewError::EditionMismatch { edition, lang } => write!(
                f,
                "`{}` is not a standard of {}",
                edition.as_str(),
                lang_name(*lang)
            ),
            NewError::Write { path, reason } => {
                write!(f, "cannot write {}: {reason}", path.display())
            }
        }
    }
}

impl std::error::Error for NewError {}

impl CunoError for NewError {
    fn exit_class(&self) -> ExitClass {
        match self {
            NewError::InvalidName { .. }
            | NewError::Exists { .. }
            | NewError::EditionMismatch { .. } => ExitClass::Usage,
            NewError::Write { .. } => ExitClass::Setup,
        }
    }

    fn help(&self) -> Option<String> {
        match self {
            NewError::InvalidName { from_directory, .. } => {
                let rule =
                    "a package name starts with a letter and has only letters, digits, - and _";
                Some(if *from_directory {
                    format!("{rule}\nhelp: name the package with --name")
                } else {
                    rule.to_owned()
                })
            }
            NewError::EditionMismatch { edition, lang } => {
                let editions: Vec<&str> = Edition::ALL
                    .iter()
                    .filter(|(e, _)| e.lang() == *lang)
                    .map(|(_, text)| *text)
                    .collect();
                Some(format!(
                    "use --lang {}, or an edition of {}: {}",
                    edition.lang().as_str(),
                    lang_name(*lang),
                    editions.join(", ")
                ))
            }
            NewError::Exists { .. } | NewError::Write { .. } => None,
        }
    }
}

/// Whether `name` is a package name: a letter first, then letters, digits,
/// `-` and `_`.
pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The language and the edition: `--lang` (C++ by default) and its default
/// edition, or the edition given, whose language `--lang` must be when it
/// is given too.
pub fn resolve(lang: Option<Lang>, edition: Option<Edition>) -> Result<(Lang, Edition), NewError> {
    match (lang, edition) {
        (Some(lang), Some(edition)) if edition.lang() != lang => {
            Err(NewError::EditionMismatch { edition, lang })
        }
        (_, Some(edition)) => Ok((edition.lang(), edition)),
        (lang, None) => {
            let lang = lang.unwrap_or(Lang::Cxx);
            Ok((lang, lang.default_edition()))
        }
    }
}

/// Whether `dir` lies in a Git repository: a `.git` in it or above it.
pub fn inside_repository(dir: &Path) -> bool {
    dir.ancestors().any(|d| d.join(".git").exists())
}

/// The files of a new package named `name` in `lang` and `edition`, by
/// their path in the package (`commands.md`, "What is generated"). The
/// sample's tests use the entry its standard selects
/// (`testing.md`, "Writing tests: two entry points").
pub fn files(name: &str, lang: Lang, edition: Edition) -> Vec<(&'static str, String)> {
    let manifest = format!(
        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nlang = \"{}\"\nedition = \"{}\"\n",
        lang.as_str(),
        edition.as_str()
    );
    let mut files = vec![("cuno.toml", manifest)];
    match lang {
        Lang::Cxx => {
            let calc = match edition {
                Edition::Cxx17 => include_str!("../templates/cxx/calc17.cpp"),
                _ => include_str!("../templates/cxx/calc.cpp"),
            };
            files.push((
                "src/main.cpp",
                include_str!("../templates/cxx/main.cpp").into(),
            ));
            files.push(("src/calc.h", include_str!("../templates/cxx/calc.h").into()));
            files.push(("src/calc.cpp", calc.into()));
        }
        Lang::C => {
            files.push(("src/main.c", include_str!("../templates/c/main.c").into()));
            files.push(("src/calc.h", include_str!("../templates/c/calc.h").into()));
            files.push(("src/calc.c", include_str!("../templates/c/calc.c").into()));
        }
    }
    files
}

/// The `.gitignore` of a package under Git.
const GITIGNORE: &str = include_str!("../templates/gitignore");

/// Makes the package `args` describe, `args.path` taken from `cwd`.
pub fn new(args: NewArgs, cwd: &Path, quiet: bool, shell: &mut Shell) -> Result<(), NewError> {
    let dir = cwd.join(&args.path);
    let (name, from_directory) = match &args.name {
        Some(name) => (name.clone(), false),
        None => (
            cuno_core::paths::lexical(&dir)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            true,
        ),
    };
    if !valid_name(&name) {
        return Err(NewError::InvalidName {
            name,
            from_directory,
        });
    }
    let (lang, edition) = resolve(args.lang, args.edition)?;
    if dir.exists() {
        return Err(NewError::Exists { path: args.path });
    }
    let write = |relative: &str, content: &str| -> Result<(), NewError> {
        let path = dir.join(relative);
        let made = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&path, content));
        made.map_err(|e| NewError::Write {
            path,
            reason: e.to_string(),
        })
    };
    // Looked for before the package's own directory exists, so it never
    // counts as the repository it is in.
    let in_repository = inside_repository(&dir);
    for (relative, content) in files(&name, lang, edition) {
        write(relative, &content)?;
    }
    if args.vcs == Vcs::Git {
        if in_repository {
            // Part of the repository it is in (`commands.md`, "`--vcs`").
            write(".gitignore", GITIGNORE)?;
        } else {
            match std::process::Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(&dir)
                .output()
            {
                Ok(output) if output.status.success() => write(".gitignore", GITIGNORE)?,
                Ok(output) => {
                    let said = String::from_utf8_lossy(&output.stderr);
                    shell.warning(format!("git init failed: {}", said.trim()));
                }
                Err(_) => {
                    shell.warning("git was not found; the package is not under version control")
                }
            }
        }
    }
    if !quiet {
        shell.status("Created", format!("`{name}` package"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mutation: allow a leading digit, `-` or `_`, another character, or
    /// an empty name; or refuse `-` or `_` after the first; this test fails.
    #[test]
    fn a_package_name_follows_the_rule() {
        for good in ["app", "my-game", "a_1", "A"] {
            assert!(valid_name(good), "{good}");
        }
        for bad in ["", "1app", "-app", "_app", "my game", "app.x", "日本"] {
            assert!(!valid_name(bad), "{bad}");
        }
    }

    /// The language decides the edition, an edition alone decides the
    /// language, and two that disagree are an error saying what to do.
    ///
    /// Mutation: default to another language or edition, ignore an edition
    /// given alone, or accept a mismatch; this test fails.
    #[test]
    fn language_and_edition_agree() {
        assert_eq!(resolve(None, None).unwrap(), (Lang::Cxx, Edition::Cxx20));
        assert_eq!(
            resolve(Some(Lang::C), None).unwrap(),
            (Lang::C, Edition::C17)
        );
        assert_eq!(
            resolve(None, Some(Edition::C11)).unwrap(),
            (Lang::C, Edition::C11)
        );
        assert_eq!(
            resolve(Some(Lang::Cxx), Some(Edition::Cxx17)).unwrap(),
            (Lang::Cxx, Edition::Cxx17)
        );
        let err = resolve(Some(Lang::Cxx), Some(Edition::C17)).unwrap_err();
        assert_eq!(err.to_string(), "`c17` is not a standard of C++");
        assert_eq!(
            err.help().as_deref(),
            Some("use --lang c, or an edition of C++: c++17, c++20, c++23")
        );
        assert_eq!(err.exit_class(), ExitClass::Usage);
    }

    /// The manifest names the package, version 0.1.0, its language and
    /// edition, and nothing else; C++17 gets the macro API's sample and
    /// C++20 the modern one.
    ///
    /// Mutation: write anything else into `cuno.toml`, or give C++17 the
    /// sample that needs C++20; this test fails.
    #[test]
    fn the_files_fit_the_language_and_edition() {
        let cxx = files("app", Lang::Cxx, Edition::Cxx20);
        assert_eq!(
            cxx[0],
            (
                "cuno.toml",
                "[package]\nname = \"app\"\nversion = \"0.1.0\"\nlang = \"c++\"\nedition = \"c++20\"\n"
                    .to_owned()
            )
        );
        let names: Vec<&str> = cxx.iter().map(|(n, _)| *n).collect();
        assert_eq!(
            names,
            ["cuno.toml", "src/main.cpp", "src/calc.h", "src/calc.cpp"]
        );
        assert!(cxx[3].1.contains("<cuno/test.hpp>"));
        let cxx17 = files("app", Lang::Cxx, Edition::Cxx17);
        assert!(cxx17[3].1.contains("<cuno/test.h>"));
        let c = files("app", Lang::C, Edition::C17);
        let names: Vec<&str> = c.iter().map(|(n, _)| *n).collect();
        assert_eq!(
            names,
            ["cuno.toml", "src/main.c", "src/calc.h", "src/calc.c"]
        );
    }

    /// Mutation: look only in the directory itself, or not at all; this
    /// test fails.
    #[test]
    fn a_repository_above_counts() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        assert!(inside_repository(&dir.path().join("a").join("app")));
        let bare = tempfile::tempdir().unwrap();
        assert!(!inside_repository(&bare.path().join("app")) || inside_repository(bare.path()));
    }

    /// Mutation: change any message, help or exit class; this test fails.
    #[test]
    fn the_errors_read_as_the_spec_says() {
        let named = NewError::InvalidName {
            name: "1app".into(),
            from_directory: false,
        };
        assert_eq!(named.to_string(), "`1app` is not a valid package name");
        assert_eq!(
            named.help().as_deref(),
            Some("a package name starts with a letter and has only letters, digits, - and _")
        );
        let from_dir = NewError::InvalidName {
            name: "my game".into(),
            from_directory: true,
        };
        assert!(
            from_dir
                .help()
                .unwrap()
                .ends_with("\nhelp: name the package with --name")
        );
        let exists = NewError::Exists {
            path: PathBuf::from("app"),
        };
        assert_eq!(exists.to_string(), "`app` already exists");
        for e in [named, from_dir, exists] {
            assert_eq!(e.exit_class(), ExitClass::Usage);
        }
        let write = NewError::Write {
            path: PathBuf::from("x"),
            reason: "denied".into(),
        };
        assert_eq!(write.exit_class(), ExitClass::Setup);
    }
}
