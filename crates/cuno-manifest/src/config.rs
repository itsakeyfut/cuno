//! Reading `config.toml`, project-level (`.cuno/config.toml`) or user-level
//! (`<cuno home>/config.toml`). See `docs/en/specs/manifest.md`,
//! ".cuno/config.toml".

use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use cuno_core::ColorChoice;
use serde::Deserialize;

use crate::read::{Doc, key};
use crate::{ConfigError, Loaded, Location, resolve_path};

/// A directory a file names, such as `[build] target-dir`, kept with how
/// and where it was written: it is judged where the layers are merged,
/// since every layer's value is checked, winning or not
/// (`docs/en/specs/build.md`, "Output directory").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocatedDir {
    /// The value as written, kept because an empty one resolves to the
    /// file's base and could no longer be told apart from it.
    pub written: PathBuf,
    /// Resolved against the file's base.
    pub path: PathBuf,
    /// Where it is written, for an error about it.
    pub location: Location,
}

/// One `config.toml`; `None` where it does not say.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Config {
    /// `[build] target-dir`.
    pub target_dir: Option<LocatedDir>,
    /// `[build] jobs`.
    pub jobs: Option<NonZeroU32>,
    /// `[toolchain] compiler`, as written; judged where the compiler is
    /// selected.
    pub compiler: Option<String>,
    /// `[toolchain.paths]`, each resolved against the file's base.
    pub paths: BTreeMap<Tool, PathBuf>,
    /// `[term] color`.
    pub color: Option<ColorChoice>,
    /// `[term] verbose`.
    pub verbose: Option<bool>,
}

/// A tool `[toolchain.paths]` can name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tool {
    /// A Visual Studio installation directory.
    Msvc,
    /// clang.
    Clang,
    /// clang-cl.
    ClangCl,
    /// GCC.
    Gcc,
    /// safec.
    Safec,
    /// clang-format.
    ClangFormat,
    /// clang-tidy.
    ClangTidy,
    /// sccache.
    Sccache,
    /// ccache.
    Ccache,
}

impl Tool {
    /// Every tool, as written in the file.
    pub const ALL: &[(Tool, &str)] = &[
        (Tool::Msvc, "msvc"),
        (Tool::Clang, "clang"),
        (Tool::ClangCl, "clang-cl"),
        (Tool::Gcc, "gcc"),
        (Tool::Safec, "safec"),
        (Tool::ClangFormat, "clang-format"),
        (Tool::ClangTidy, "clang-tidy"),
        (Tool::Sccache, "sccache"),
        (Tool::Ccache, "ccache"),
    ];

    fn parse(name: &str) -> Option<Tool> {
        Tool::ALL.iter().find(|(_, n)| *n == name).map(|(t, _)| *t)
    }
}

#[derive(Deserialize, Default)]
#[serde(expecting = "a table")]
struct RawConfig {
    build: Option<RawBuild>,
    toolchain: Option<RawToolchain>,
    target: Option<toml::Value>,
    term: Option<RawTerm>,
    alias: Option<toml::Table>,
}

#[derive(Deserialize, Default)]
#[serde(expecting = "a table", rename_all = "kebab-case")]
struct RawBuild {
    target_dir: Option<PathBuf>,
    jobs: Option<u32>,
    cache: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(expecting = "a table")]
struct RawToolchain {
    compiler: Option<String>,
    paths: Option<BTreeMap<String, PathBuf>>,
}

#[derive(Deserialize, Default)]
#[serde(expecting = "a table")]
struct RawTerm {
    color: Option<String>,
    verbose: Option<bool>,
}

/// Reads the `config.toml` at `path`, shown in messages as `shown`, with
/// relative paths resolved against `base`. A file that is not there is an
/// empty configuration.
pub fn load_config(path: &Path, shown: &Path, base: &Path) -> Result<Loaded<Config>, ConfigError> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse_config(&text, shown, base),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Loaded {
            value: Config::default(),
            warnings: Vec::new(),
        }),
        Err(e) => Err(ConfigError {
            message: format!("cannot read {}: {e}", path.display()),
            location: None,
            help: None,
        }),
    }
}

/// Sets `[toolchain] compiler = name` in the user-level `config.toml` at
/// `path`, keeping its other keys, its comments, its order, its line
/// endings and a byte order mark, and making the file (and its directory)
/// when there is none. Nothing is written when the file cannot be read or
/// `[toolchain]` is not a table, and a failed write never leaves half a
/// configuration in its place (`docs/en/specs/commands.md`, "`cuno
/// toolchain default <compiler>`").
pub fn write_user_compiler(path: &Path, name: &str) -> Result<(), ConfigError> {
    let error = |message: String| ConfigError {
        message,
        location: None,
        help: None,
    };
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(error(format!("cannot read {}: {e}", path.display()))),
    };
    // The reader's own parse first, so a broken file is reported as every
    // command reports it, with its line and column.
    Doc::parse(&text, path)?;
    // toml_edit writes every line ending as `\n` and drops the mark, which
    // would change every line of a file written on Windows: both are put
    // back as they were.
    let (bom, body) = match text.strip_prefix('\u{feff}') {
        Some(body) => ("\u{feff}", body),
        None => ("", text.as_str()),
    };
    let crlf = body.contains("\r\n");
    let mut doc: toml_edit::DocumentMut = body
        .parse()
        .map_err(|e| error(format!("cannot read {}: {e}", path.display())))?;
    let toolchain = doc
        .entry("toolchain")
        .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()));
    let Some(table) = toolchain.as_table_like_mut() else {
        return Err(error(format!(
            "`[toolchain]` in {} is not a table",
            path.display()
        )));
    };
    table.insert("compiler", toml_edit::value(name));

    let written = |e: std::io::Error| error(format!("cannot write {}: {e}", path.display()));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(written)?;
    }
    let mut beside = path.as_os_str().to_owned();
    beside.push(".tmp");
    let beside = PathBuf::from(beside);
    let mut out = doc.to_string().replace("\r\n", "\n");
    if crlf {
        out = out.replace('\n', "\r\n");
    }
    std::fs::write(&beside, format!("{bom}{out}")).map_err(written)?;
    std::fs::rename(&beside, path).map_err(|e| {
        let _ = std::fs::remove_file(&beside);
        written(e)
    })
}

/// Reads `text` as a `config.toml`; `file` is how messages name it, and
/// relative paths resolve against `base`.
pub fn parse_config(text: &str, file: &Path, base: &Path) -> Result<Loaded<Config>, ConfigError> {
    let doc = Doc::parse(text, file)?;
    let (raw, mut unknown): (RawConfig, Vec<String>) = doc.deserialize()?;
    let shown = file.display();
    let mut warnings = Vec::new();

    if raw.target.is_some() {
        return Err(doc.error(
            "target",
            format!("`[target]` is not available yet ({shown})"),
            Some("remove `target`; it comes in a later release".to_owned()),
        ));
    }
    let path = |name: &str, value: &Path| {
        resolve_path(value, base).map_err(|e| ConfigError {
            location: Some(doc.locate(name)),
            ..e
        })
    };

    let build = raw.build.unwrap_or_default();
    // Left at its default, `cache` ignores nothing (manifest.md, "Implemented
    // in MVP"); MVP uses no build cache.
    if build.cache.as_deref().is_some_and(|cache| cache != "auto") {
        warnings.push(doc.warning(
            "build.cache",
            format!("`build.cache` in {shown} is ignored: it is not supported yet"),
        ));
    }
    if raw.alias.as_ref().is_some_and(|alias| !alias.is_empty()) {
        warnings.push(doc.warning(
            "alias",
            format!("`alias` in {shown} is ignored: it is not supported yet"),
        ));
    }
    let jobs = match build.jobs {
        None => None,
        Some(n) => Some(NonZeroU32::new(n).ok_or_else(|| {
            doc.error(
                "build.jobs",
                format!("`build.jobs` in {shown} is 0, but at least one job is needed"),
                None,
            )
        })?),
    };
    let target_dir = build
        .target_dir
        .map(|dir| {
            Ok(LocatedDir {
                path: path("build.target-dir", &dir)?,
                written: dir,
                location: doc.locate("build.target-dir"),
            })
        })
        .transpose()?;

    let toolchain = raw.toolchain.unwrap_or_default();
    let mut paths = BTreeMap::new();
    for (name, value) in toolchain.paths.unwrap_or_default() {
        let at = format!("toolchain.paths.{}", key(&name));
        match Tool::parse(&name) {
            Some(tool) => {
                paths.insert(tool, path(&at, &value)?);
            }
            None => unknown.push(at),
        }
    }

    let term = raw.term.unwrap_or_default();
    let color = match term.color.as_deref() {
        None => None,
        Some("auto") => Some(ColorChoice::Auto),
        Some("always") => Some(ColorChoice::Always),
        Some("never") => Some(ColorChoice::Never),
        Some(other) => {
            return Err(doc.error(
                "term.color",
                format!("`term.color` is \"{other}\" in {shown}, which is not one of auto, always, never"),
                None,
            ));
        }
    };

    warnings.extend(unknown.iter().map(|path| doc.unknown(path)));
    warnings.sort_by_key(|w| w.location.as_ref().map(|l| (l.line, l.column)));
    Ok(Loaded {
        value: Config {
            target_dir,
            jobs,
            compiler: toolchain.compiler,
            paths,
            color,
            verbose: term.verbose,
        },
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cuno_core::{CunoError, ExitClass};

    /// Writing the default changes `[toolchain] compiler` and nothing else:
    /// a comment, `[toolchain.paths]` and another table stay as they were,
    /// and the file still reads as the same configuration with the new
    /// compiler.
    ///
    /// Mutation: write the file from the new key alone, drop its comments,
    /// set the key at the top level, leave the file written beside it in
    /// place, or not make the directory; this test fails.
    #[test]
    fn writing_the_default_keeps_the_rest_of_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let before = "# my machine\n[build]\njobs = 4  # half the cores\n\n[toolchain]\ncompiler = \"clang\"\n\n[toolchain.paths]\nclang = \"C:/LLVM/bin/clang.exe\"\n";
        std::fs::write(&path, before).unwrap();
        write_user_compiler(&path, "gcc").unwrap();
        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            after,
            before.replace("compiler = \"clang\"", "compiler = \"gcc\"")
        );
        assert!(!dir.path().join("config.toml.tmp").exists());

        // Made, with its directory, when there is none.
        let fresh = dir.path().join("home").join("config.toml");
        write_user_compiler(&fresh, "msvc").unwrap();
        assert_eq!(
            std::fs::read_to_string(&fresh).unwrap(),
            "[toolchain]\ncompiler = \"msvc\"\n"
        );
    }

    /// A file written on Windows keeps its CRLF line endings and its byte
    /// order mark on every line, not only the new one.
    ///
    /// Mutation: write toml_edit's text as it comes, or drop the mark;
    /// this test fails.
    #[test]
    fn writing_the_default_keeps_crlf_and_a_byte_order_mark() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        for before in [
            "\u{feff}# c\r\n[build]\r\njobs = 2\r\n\r\n[toolchain]\r\ncompiler = \"clang\"\r\n",
            "[build]\r\njobs = 2\r\n",
        ] {
            std::fs::write(&path, before).unwrap();
            write_user_compiler(&path, "gcc").unwrap();
            let after = std::fs::read_to_string(&path).unwrap();
            let expected = if before.contains("compiler") {
                before.replace("\"clang\"", "\"gcc\"")
            } else {
                format!("{before}\r\n[toolchain]\r\ncompiler = \"gcc\"\r\n")
            };
            assert_eq!(after, expected);
        }
    }

    /// A `toolchain` written as an inline table is a table: the key goes in
    /// it, and it stays inline.
    ///
    /// Mutation: accept only a `[toolchain]` header table; this test
    /// fails.
    #[test]
    fn an_inline_toolchain_table_takes_the_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "toolchain = { compiler = \"clang\" }\n").unwrap();
        write_user_compiler(&path, "gcc").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "toolchain = { compiler = \"gcc\" }\n"
        );
    }

    /// A `toolchain` that is not a table, and a file that is not TOML, are
    /// errors that name the file, and the file is left as it was.
    ///
    /// Mutation: overwrite `toolchain` with a table, or write what parsed
    /// in part; this test fails.
    #[test]
    fn a_file_the_default_cannot_go_in_is_refused_and_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        for before in [
            "toolchain = \"clang\"\n",
            "[toolchain\ncompiler = \"clang\"\n",
        ] {
            std::fs::write(&path, before).unwrap();
            let err = write_user_compiler(&path, "gcc").unwrap_err();
            assert_eq!(err.exit_class(), ExitClass::Setup);
            assert!(err.to_string().contains("config.toml"), "{err}");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
        }
        // Reported as reading reports it: with where.
        std::fs::write(&path, "[toolchain\n").unwrap();
        let err = write_user_compiler(&path, "gcc").unwrap_err();
        assert!(err.to_string().contains("config.toml:1:"), "{err}");
        std::fs::write(&path, "toolchain = \"clang\"\n").unwrap();
        let err = write_user_compiler(&path, "gcc").unwrap_err();
        assert!(err.to_string().contains("is not a table"), "{err}");
    }

    fn base() -> PathBuf {
        std::env::temp_dir().join("cuno-base")
    }

    fn read(text: &str) -> Result<Loaded<Config>, ConfigError> {
        parse_config(text, Path::new("config.toml"), &base())
    }

    /// Mutation: drop or misread any key, resolve a path against anything
    /// but the file's base, or file a tool's path under another; this test
    /// fails.
    #[test]
    fn every_key_is_read() {
        let text = concat!(
            "[build]\ntarget-dir = \"out\"\njobs = 16\ncache = \"auto\"\n",
            "[toolchain]\ncompiler = \"clang\"\n",
            "[toolchain.paths]\n",
            "msvc = \"vs\"\nclang = \"c\"\nclang-cl = \"cc\"\ngcc = \"g\"\nsafec = \"s\"\n",
            "clang-format = \"f\"\nclang-tidy = \"t\"\nsccache = \"sc\"\nccache = \"cc2\"\n",
            "[term]\ncolor = \"never\"\nverbose = true\n",
        );
        let loaded = read(text).unwrap();
        assert_eq!(loaded.warnings, []);
        let b = base();
        let c = loaded.value;
        assert_eq!(c.target_dir.map(|t| t.path), Some(b.join("out")));
        assert_eq!(c.jobs, NonZeroU32::new(16));
        assert_eq!(c.compiler.as_deref(), Some("clang"));
        let expected: BTreeMap<Tool, PathBuf> = [
            (Tool::Msvc, "vs"),
            (Tool::Clang, "c"),
            (Tool::ClangCl, "cc"),
            (Tool::Gcc, "g"),
            (Tool::Safec, "s"),
            (Tool::ClangFormat, "f"),
            (Tool::ClangTidy, "t"),
            (Tool::Sccache, "sc"),
            (Tool::Ccache, "cc2"),
        ]
        .into_iter()
        .map(|(t, p)| (t, b.join(p)))
        .collect();
        assert_eq!(c.paths, expected);
        assert_eq!((c.color, c.verbose), (Some(ColorChoice::Never), Some(true)));
        for (color, choice) in [
            ("auto", ColorChoice::Auto),
            ("always", ColorChoice::Always),
            ("never", ColorChoice::Never),
        ] {
            let c = read(&format!("[term]\ncolor = \"{color}\"\n"))
                .unwrap()
                .value;
            assert_eq!(c.color, Some(choice));
        }
        assert_eq!(read("").unwrap().value, Config::default());
    }

    /// Mutation: stop warning about a cache or alias that would be ignored,
    /// warn about the default cache, or keep an unknown tool; this test
    /// fails.
    #[test]
    fn what_is_read_past_is_warned_about() {
        let loaded = read(concat!(
            "[build]\ncache = \"sccache\"\n",
            "[toolchain.paths]\nnvcc = \"n\"\n",
            "[alias]\nb = \"build\"\n",
            "[term]\nwide = true\n",
        ))
        .unwrap();
        let shown: Vec<(usize, String)> = loaded
            .warnings
            .iter()
            .map(|w| (w.location.as_ref().unwrap().line, w.message.clone()))
            .collect();
        assert_eq!(
            shown,
            [
                (
                    2,
                    "`build.cache` in config.toml is ignored: it is not supported yet".to_owned()
                ),
                (
                    4,
                    "unknown key `toolchain.paths.nvcc` in config.toml, ignored".to_owned()
                ),
                (
                    5,
                    "`alias` in config.toml is ignored: it is not supported yet".to_owned()
                ),
                (
                    8,
                    "unknown key `term.wide` in config.toml, ignored".to_owned()
                ),
            ]
        );
        assert!(loaded.value.paths.is_empty());
        assert_eq!(
            read("[build]\ncache = \"auto\"\n[alias]\n")
                .unwrap()
                .warnings,
            []
        );
    }

    /// Mutation: accept any of these, or report it elsewhere; this test
    /// fails. Each is a setup problem (exit code 3).
    #[test]
    fn invalid_settings_are_errors_at_their_line() {
        for (text, line, message) in [
            (
                "[target.aarch64-linux-android]\nndk = \"x\"\n",
                1,
                "`[target]` is not available yet",
            ),
            ("\n[build]\njobs = 0\n", 3, "at least one job"),
            (
                "[term]\ncolor = \"sometimes\"\n",
                2,
                "not one of auto, always, never",
            ),
            ("[build]\njobs = \"many\"\n", 2, "invalid type"),
        ] {
            let err = read(text).expect_err(text);
            assert!(err.message.contains(message), "{err}");
            assert_eq!(err.location.as_ref().unwrap().line, line, "{err}");
            assert_eq!(err.exit_class(), ExitClass::Setup);
        }
    }

    /// Mutation: treat a missing file as an error; this test fails.
    #[test]
    fn a_missing_file_is_an_empty_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("config.toml");
        let loaded = load_config(&missing, Path::new("config.toml"), dir.path()).unwrap();
        assert_eq!(loaded.value, Config::default());
    }
}
