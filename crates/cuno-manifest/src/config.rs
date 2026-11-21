//! Reading `config.toml`, project-level (`.cuno/config.toml`) or user-level
//! (`<cuno home>/config.toml`). See `docs/en/specs/manifest.md`,
//! ".cuno/config.toml".

use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use cuno_core::ColorChoice;
use serde::Deserialize;

use crate::read::{Doc, key};
use crate::{ConfigError, Loaded, resolve_path};

/// One `config.toml`; `None` where it does not say.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Config {
    /// `[build] target-dir`, resolved against the file's base.
    pub target_dir: Option<PathBuf>,
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
    // An empty output directory would be the base itself (build.md).
    if build
        .target_dir
        .as_ref()
        .is_some_and(|dir| dir.as_os_str().is_empty())
    {
        return Err(doc.error(
            "build.target-dir",
            format!("`build.target-dir` in {shown} is empty"),
            Some("write the output directory, such as \"target\"".to_owned()),
        ));
    }
    let target_dir = build
        .target_dir
        .map(|dir| path("build.target-dir", &dir))
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
        assert_eq!(c.target_dir, Some(b.join("out")));
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
