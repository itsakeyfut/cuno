//! Which files of a package are compiled, and into what: its executables'
//! entry files and the package body, each source with its language
//! (`docs/en/specs/project-layout.md`, "Source collection and linking").

use std::fmt;
use std::path::{Component, Path, PathBuf};

use cuno_manifest::Manifest;
use cuno_toolchain::flags::{Lang, Subsystem};

use crate::BuildError;

/// A source to compile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    /// Its absolute path.
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
    /// What is not built and why.
    pub warnings: Vec<SourceWarning>,
}

/// A source under `src/main.*` or `src/bin/` that `[[bin]]` does not list,
/// and so is not built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceWarning {
    /// The source, as shown (`src/bin/tool.cpp`).
    pub path: String,
}

impl fmt::Display for SourceWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} is not built: [[bin]] is written and does not list it",
            self.path
        )
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

/// Classifies the sources of the package at `root`, skipping `target_dir`
/// when it lies under `src/`.
pub fn classify(
    root: &Path,
    manifest: &Manifest,
    target_dir: &Path,
) -> Result<Sources, BuildError> {
    let root = normalize(root);
    let src = root.join("src");
    let bin_dir = src.join("bin");
    let mut found = Vec::new();
    if src.is_dir() {
        walk(&root, &src, &normalize(target_dir), &mut found)?;
    }
    found.sort();
    let is_main =
        |p: &Path| p.parent() == Some(src.as_path()) && p.file_stem().is_some_and(|s| s == "main");
    let is_in_bin = |p: &Path| p.starts_with(&bin_dir);

    let mut warnings = Vec::new();
    let bins = match &manifest.bins {
        Some(listed) => {
            let mut bins = Vec::new();
            for bin in listed {
                let path = normalize(&bin.path);
                if !path.starts_with(&root) {
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
                bins.push(Bin {
                    name: bin.name.clone(),
                    entry: Source {
                        lang: language(&path).unwrap_or(manifest.package.lang),
                        path,
                    },
                    subsystem: bin.subsystem,
                });
            }
            for path in &found {
                let listed = bins.iter().any(|b| &b.entry.path == path);
                if (is_main(path) || is_in_bin(path)) && !listed {
                    warnings.push(SourceWarning {
                        path: shown(&root, path),
                    });
                }
            }
            bins
        }
        None => {
            let mut bins = Vec::new();
            let default = src.join(default_entry(manifest.package.lang));
            if default.is_file() {
                bins.push(Bin {
                    name: manifest.package.name.clone(),
                    entry: Source {
                        lang: manifest.package.lang,
                        path: default,
                    },
                    subsystem: Subsystem::Console,
                });
            }
            for path in found
                .iter()
                .filter(|p| p.parent() == Some(bin_dir.as_path()))
            {
                let name = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                bins.push(Bin {
                    name,
                    entry: Source {
                        lang: language(path).unwrap_or(manifest.package.lang),
                        path: path.clone(),
                    },
                    subsystem: Subsystem::Console,
                });
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
        if let Some(other) = bins[..i].iter().find(|b| b.name == bin.name) {
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
        .filter(|p| !is_main(p) && !is_in_bin(p) && !bins.iter().any(|b| &b.entry.path == p))
        .filter_map(|path| {
            let lang = language(&path)?;
            Some(Source { path, lang })
        })
        .collect();
    Ok(Sources {
        bins,
        body,
        warnings,
    })
}

/// The default entry file's name for a package in `lang`.
fn default_entry(lang: Lang) -> &'static str {
    match lang {
        Lang::Cxx => "main.cpp",
        Lang::C => "main.c",
    }
}

/// Every C or C++ source under `dir`, skipping `target_dir`. Directory
/// links are not followed, so a link cycle cannot make this run forever.
fn walk(
    root: &Path,
    dir: &Path,
    target_dir: &Path,
    out: &mut Vec<PathBuf>,
) -> Result<(), BuildError> {
    if dir == target_dir {
        return Ok(());
    }
    let unreadable = |e: std::io::Error| BuildError::Unreadable {
        dir: shown(root, dir),
        reason: e.to_string(),
    };
    for entry in std::fs::read_dir(dir).map_err(unreadable)? {
        let entry = entry.map_err(unreadable)?;
        let path = entry.path();
        let kind = entry.file_type().map_err(unreadable)?;
        if kind.is_dir() {
            walk(root, &path, target_dir, out)?;
        } else if path.is_file() && language(&path).is_some() {
            out.push(path);
        }
    }
    Ok(())
}

/// `path` with `.` and `..` resolved by name, so a `[[bin]] path` written
/// with `..` compares equal to the file it names.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
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
mod tests;
