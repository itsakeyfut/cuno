//! `cuno new` and `cuno init`: a package ready to build, test and run, in
//! a new directory or an existing one (`docs/en/specs/commands.md`, "`cuno
//! new <path>`" and "`cuno init [path]`").

use std::fmt;
use std::path::{Path, PathBuf};

use cuno_core::{CunoError, ExitClass, Shell};
use cuno_manifest::valid_package_name;
use cuno_toolchain::flags::{Edition, Lang};

/// The options of `cuno new`.
#[derive(Debug)]
pub struct NewArgs {
    /// Where to make the package, as given: relative to the current
    /// directory, or absolute.
    pub path: PathBuf,
    /// `--lang`; C++ when neither it nor `--edition` is given.
    pub lang: Option<Lang>,
    /// `--edition`; the language's default when not given.
    pub edition: Option<Edition>,
    /// `--name`; the directory's name when not given.
    pub name: Option<String>,
    /// `--vcs`; Git by default.
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
    /// A name Windows keeps for devices (`con`, `nul`, ...).
    ReservedName {
        /// The name.
        name: String,
    },
    /// A package name the rule does not allow.
    InvalidName {
        /// The name.
        name: String,
        /// Whether it came from the directory's name rather than `--name`.
        from_directory: bool,
    },
    /// The destination is already there.
    Exists {
        /// The destination as the user gave it, which the message shows.
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
        /// The file or directory, in full.
        path: PathBuf,
        /// Why.
        reason: String,
    },
}

/// The language's name as messages write it (`C++`), where the manifest
/// writes `c++`.
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
            NewError::ReservedName { name } => {
                write!(f, "`{name}` is a reserved name on Windows")
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
            | NewError::ReservedName { .. }
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
            NewError::ReservedName { .. } => Some("choose another name".to_owned()),
            NewError::Exists { .. } | NewError::Write { .. } => None,
        }
    }
}

/// Whether `name` is one Windows keeps for devices, in any case, which
/// cannot name a file or directory there (`commands.md`, "`cuno new
/// <path>`").
pub fn reserved_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let numbered = |prefix: &str| {
        lower
            .strip_prefix(prefix)
            .is_some_and(|n| n.len() == 1 && n.as_bytes()[0].is_ascii_digit() && n != "0")
    };
    ["con", "prn", "aux", "nul"].contains(&lower.as_str()) || numbered("com") || numbered("lpt")
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
/// their path in the package (`commands.md`, "Generated files"). The
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
            // Every edition named, so a new one has to choose its sample.
            let calc = match edition {
                Edition::Cxx17 => include_str!("../templates/cxx/calc17.cpp"),
                Edition::Cxx20 | Edition::Cxx23 => include_str!("../templates/cxx/calc.cpp"),
                Edition::C11 | Edition::C17 | Edition::C23 => {
                    unreachable!("a C edition with the C++ language")
                }
            };
            files.push((
                lang.default_entry(),
                include_str!("../templates/cxx/main.cpp").into(),
            ));
            files.push(("src/calc.h", include_str!("../templates/cxx/calc.h").into()));
            files.push(("src/calc.cpp", calc.into()));
        }
        Lang::C => {
            files.push((
                lang.default_entry(),
                include_str!("../templates/c/main.c").into(),
            ));
            files.push(("src/calc.h", include_str!("../templates/c/calc.h").into()));
            files.push(("src/calc.c", include_str!("../templates/c/calc.c").into()));
        }
    }
    files
}

/// The `.gitignore` of a package under Git.
const GITIGNORE: &str = include_str!("../templates/gitignore");

/// Makes the package `args` describe, `args.path` taken from `cwd`. Every
/// refusal comes before anything is written; a file that cannot be written
/// half way stops it there, leaving what was made for the user to see.
pub fn new(args: NewArgs, cwd: &Path, shell: &mut Shell) -> Result<(), NewError> {
    // `..` resolved, so the directories above it are the ones it will be in.
    let dir = cuno_core::paths::lexical(&cwd.join(&args.path));
    let (name, from_directory) = match &args.name {
        Some(name) => (name.clone(), false),
        None => (
            dir.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            true,
        ),
    };
    if !valid_package_name(&name) {
        return Err(NewError::InvalidName {
            name,
            from_directory,
        });
    }
    if reserved_name(&name) {
        return Err(NewError::ReservedName { name });
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
    // `commands.md`, "`cuno new <path>`": a `.gitignore` goes with a
    // repository, the package's own or the one it is in; with no repository (no
    // git, or `git init` failing), neither is made, and a warning says why.
    if args.vcs == Vcs::Git {
        if in_repository {
            // No repository of its own: it is part of the one it is in.
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
    shell.status("Created", format!("`{name}` package"));
    Ok(())
}

/// The options of `cuno init`: those of `cuno new`, the path being an
/// existing directory, the current one by default.
#[derive(Debug)]
pub struct InitArgs {
    /// The directory, as given: relative to the current directory, or
    /// absolute.
    pub path: PathBuf,
    /// `--lang`; from the sources in `src/` when neither it nor `--edition`
    /// is given.
    pub lang: Option<Lang>,
    /// `--edition`; the language's default when not given.
    pub edition: Option<Edition>,
    /// `--name`; the directory's name when not given.
    pub name: Option<String>,
    /// `--vcs`; Git by default.
    pub vcs: Vcs,
}

/// What stops `cuno init`, besides what stops `cuno new`. Every one is exit
/// code 2: nothing has been written (`commands.md`, "`cuno init [path]`").
#[derive(Debug)]
pub enum InitError {
    /// The directory is not there.
    NoDirectory {
        /// As the user gave it.
        path: PathBuf,
    },
    /// The directory is a package already.
    AlreadyPackage {
        /// As the user gave it.
        path: PathBuf,
    },
    /// Sources, none of them in `src/`.
    SourcesElsewhere {
        /// The first few, from the directory, with `/`.
        found: Vec<String>,
        /// How many more there are.
        more: usize,
    },
    /// Sources in `src/` and no entry file.
    NoEntry,
    /// A file it would write is there.
    FileExists {
        /// From the directory, with `/`.
        path: String,
    },
}

impl fmt::Display for InitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InitError::NoDirectory { path } => write!(f, "`{}` does not exist", path.display()),
            InitError::AlreadyPackage { path } => {
                write!(f, "`{}` already has a cuno.toml", path.display())
            }
            InitError::SourcesElsewhere { found, more } => {
                write!(
                    f,
                    "the sources are not where cuno looks for them: {}",
                    found.join(", ")
                )?;
                if *more > 0 {
                    write!(f, " and {more} more")?;
                }
                Ok(())
            }
            InitError::NoEntry => f.write_str("src/ has no entry file"),
            InitError::FileExists { path } => write!(f, "{path} already exists"),
        }
    }
}

impl std::error::Error for InitError {}

impl CunoError for InitError {
    fn exit_class(&self) -> ExitClass {
        ExitClass::Usage
    }

    fn help(&self) -> Option<String> {
        Some(
            match self {
                InitError::NoDirectory { .. } => {
                    "use cuno new to create a package in a new directory"
                }
                InitError::AlreadyPackage { .. } => return None,
                InitError::SourcesElsewhere { .. } => {
                    "move them under src/, with the program's main in src/main.cpp (or src/main.c)"
                }
                InitError::NoEntry => {
                    "put the program's main in src/main.cpp (or src/main.c), or each program in src/bin/"
                }
                InitError::FileExists { .. } => {
                    "cuno init writes no file that is there; move it aside, or add cuno.toml by hand"
                }
            }
            .to_owned(),
        )
    }
}

/// How many sources a message names before it counts the rest.
const SHOWN_SOURCES: usize = 5;

/// The sources in `dir`, by their path from it with `/`: those under `src/`
/// and the rest. What cuno makes (`target/`, `.cuno/`), hidden directories,
/// links and other packages (a directory with its own `cuno.toml`) are not
/// looked in.
fn sources_in(dir: &Path) -> (Vec<String>, Vec<String>) {
    fn walk(dir: &Path, from: &str, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            let relative = if from.is_empty() {
                name.clone()
            } else {
                format!("{from}/{name}")
            };
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                let skipped = name.starts_with('.')
                    || (from.is_empty() && name == "target")
                    || entry.path().join("cuno.toml").exists();
                if !skipped {
                    walk(&entry.path(), &relative, out);
                }
            } else if kind.is_file() && cuno_build::sources::language(&entry.path()).is_some() {
                out.push(relative);
            }
        }
    }
    let mut all = Vec::new();
    walk(dir, "", &mut all);
    all.into_iter().partition(|s| s.starts_with("src/"))
}

/// Makes the existing directory `args` describe into a package, `args.path`
/// taken from `cwd`: `cuno.toml`, the sample when there are no sources, and
/// the `.gitignore` lines cuno needs. Every refusal comes before anything
/// is written (`docs/en/specs/commands.md`, "`cuno init [path]`").
pub fn init(
    args: InitArgs,
    cwd: &Path,
    host: cuno_toolchain::HostOs,
    shell: &mut Shell,
) -> Result<(), Box<dyn CunoError>> {
    let boxed = |e: InitError| Box::new(e) as Box<dyn CunoError>;
    let dir = cuno_core::paths::lexical(&cwd.join(&args.path));
    if !dir.is_dir() {
        return Err(boxed(InitError::NoDirectory { path: args.path }));
    }
    if dir.join("cuno.toml").exists() {
        return Err(boxed(InitError::AlreadyPackage { path: args.path }));
    }
    let (name, from_directory) = match &args.name {
        Some(name) => (name.clone(), false),
        None => (
            dir.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            true,
        ),
    };
    if !valid_package_name(&name) {
        return Err(Box::new(NewError::InvalidName {
            name,
            from_directory,
        }));
    }
    if reserved_name(&name) {
        return Err(Box::new(NewError::ReservedName { name }));
    }

    let (in_src, elsewhere) = sources_in(&dir);
    if in_src.is_empty() && !elsewhere.is_empty() {
        return Err(boxed(InitError::SourcesElsewhere {
            more: elsewhere.len().saturating_sub(SHOWN_SOURCES),
            found: elsewhere.into_iter().take(SHOWN_SOURCES).collect(),
        }));
    }
    // The language of the sources there are, when not given: C++ if any is
    // C++, C if all are C, and C++ with none, as for `cuno new`.
    let lang = args.lang.or_else(|| {
        (args.edition.is_none() && !in_src.is_empty()).then(|| {
            let cxx = in_src
                .iter()
                .any(|s| cuno_build::sources::language(Path::new(s)) == Some(Lang::Cxx));
            if cxx { Lang::Cxx } else { Lang::C }
        })
    });
    let (lang, edition) =
        resolve(lang, args.edition).map_err(|e| Box::new(e) as Box<dyn CunoError>)?;
    let mut written = files(&name, lang, edition);
    if !in_src.is_empty() {
        // Only the manifest: the sources are the user's. Whether they make
        // an executable is the build's own rule, asked of the manifest to be.
        written.truncate(1);
        let manifest = cuno_manifest::parse_manifest(&written[0].1, &dir.join("cuno.toml"), &dir)
            .map_err(|e| Box::new(e) as Box<dyn CunoError>)?
            .value;
        match cuno_build::sources::classify(&dir, &manifest, &dir.join("target"), host) {
            Ok(_) => {}
            Err(cuno_build::BuildError::NoExecutable { .. }) => {
                return Err(boxed(InitError::NoEntry));
            }
            Err(error) => return Err(Box::new(error)),
        }
    }
    for (relative, _) in &written {
        if cuno_tools::write::absent(&dir.join(relative)).is_err() {
            return Err(boxed(InitError::FileExists {
                path: (*relative).to_owned(),
            }));
        }
    }

    let failed = |path: PathBuf, e: std::io::Error| {
        Box::new(NewError::Write {
            path,
            reason: e.to_string(),
        }) as Box<dyn CunoError>
    };
    let in_repository = inside_repository(&dir);
    for (relative, content) in &written {
        let path = dir.join(relative);
        let made = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| {
                use std::io::Write;
                std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)?
                    .write_all(content.as_bytes())
            });
        made.map_err(|e| failed(path, e))?;
    }
    if args.vcs == Vcs::Git {
        let repository = in_repository || {
            match std::process::Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(&dir)
                .output()
            {
                Ok(output) if output.status.success() => true,
                Ok(output) => {
                    let said = String::from_utf8_lossy(&output.stderr);
                    shell.warning(format!("git init failed: {}", said.trim()));
                    false
                }
                Err(_) => {
                    shell.warning("git was not found; the package is not under version control");
                    false
                }
            }
        };
        if repository {
            let path = dir.join(".gitignore");
            gitignore(&path).map_err(|e| failed(path, e))?;
        }
    }
    shell.status("Created", format!("`{name}` package"));
    Ok(())
}

/// `.gitignore` at `path` made, or, when there is one, given the lines of
/// cuno's that it lacks, at its end, in its own line ending; its lines and
/// a byte order mark are left as they are.
fn gitignore(path: &Path) -> std::io::Result<()> {
    use std::io::Write;
    if cuno_tools::write::absent(path).is_ok() {
        return std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?
            .write_all(GITIGNORE.as_bytes());
    }
    if let Err(refusal) = cuno_tools::write::appendable(path) {
        return Err(std::io::Error::other(format!(
            "{} {}",
            path.display(),
            refusal.which()
        )));
    }
    let held = std::fs::read_to_string(path)?;
    let text = held.strip_prefix('\u{feff}').unwrap_or(&held);
    let have: Vec<&str> = text.lines().map(str::trim).collect();
    let missing: Vec<&str> = GITIGNORE
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !have.contains(l))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut added = String::new();
    if !text.is_empty() && !text.ends_with('\n') {
        added += newline;
    }
    for line in missing {
        added += line;
        added += newline;
    }
    std::fs::OpenOptions::new()
        .append(true)
        .open(path)?
        .write_all(added.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let c = resolve(Some(Lang::C), Some(Edition::Cxx17)).unwrap_err();
        assert_eq!(c.to_string(), "`c++17` is not a standard of C");
        assert_eq!(
            c.help().as_deref(),
            Some("use --lang c++, or an edition of C: c11, c17, c23")
        );
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
    }

    /// Windows' device names are refused in any case; names that only start
    /// like one are not.
    ///
    /// Mutation: compare with case, drop a family, or refuse `com0` or
    /// `console`; this test fails.
    #[test]
    fn device_names_are_reserved() {
        for name in ["con", "PRN", "Aux", "nul", "com1", "COM9", "lpt5"] {
            assert!(reserved_name(name), "{name}");
        }
        for name in ["console", "com0", "com10", "lpt", "nulls", "auxiliary"] {
            assert!(!reserved_name(name), "{name}");
        }
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
        let reserved = NewError::ReservedName { name: "con".into() };
        assert_eq!(reserved.to_string(), "`con` is a reserved name on Windows");
        assert_eq!(reserved.help().as_deref(), Some("choose another name"));
        assert_eq!(reserved.exit_class(), ExitClass::Usage);
        let write = NewError::Write {
            path: PathBuf::from("x"),
            reason: "denied".into(),
        };
        assert_eq!(write.to_string(), "cannot write x: denied");
        assert_eq!(write.exit_class(), ExitClass::Setup);
    }
}
