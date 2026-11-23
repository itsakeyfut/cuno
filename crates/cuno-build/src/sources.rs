//! Which files of a package are compiled, and into what: its executables'
//! entry files and the package body, each source with its language
//! (`docs/en/specs/project-layout.md`, "Source collection and linking").

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
    /// Its absolute path, spelled as found under `src/`.
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

/// Classifies the sources of the package at `root`, skipping `target_dir`
/// when it lies under `src/`. On a Windows `host`, paths and names are
/// compared without regard to case, as its file systems do
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
    /// joins the body unannounced.
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
mod tests;
