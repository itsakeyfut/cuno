//! Reading and validating `cuno.toml`. See `docs/en/specs/manifest.md`.
//!
//! The file is deserialized into raw structs whose fields are all optional,
//! then checked into a [`Manifest`]. Defaults that depend on other settings
//! (the build layering) are applied where the settings are resolved, not
//! here.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cuno_core::CunoError;
use cuno_toolchain::select::{Constraints, VersionReq};
use serde::Deserialize;

use crate::read::{Doc, key};
use crate::{ConfigError, ConfigWarning, Project, resolve_path, shown_path};

/// A value read from a file, with what cuno read past.
#[derive(Clone, Debug)]
pub struct Loaded<T> {
    /// The value.
    pub value: T,
    /// Warnings, in file order.
    pub warnings: Vec<ConfigWarning>,
}

/// `cuno.toml`.
#[derive(Clone, Debug, PartialEq)]
pub struct Manifest {
    /// `[package]`.
    pub package: Package,
    /// `[toolchain] compiler`, as written; judged where the compiler is
    /// selected, since a name a higher layer overrides is not an error.
    pub compiler: Option<String>,
    /// `[toolchain.version]` for the compilers cuno builds with.
    pub constraints: Constraints,
    /// `[build] target-dir`, resolved against the project root.
    pub target_dir: Option<PathBuf>,
    /// `[build]`.
    pub build: BuildTable,
    /// `[build.<compiler>] flags`.
    pub compiler_flags: BTreeMap<CompilerName, Vec<String>>,
    /// `[platform.<OS>.build]`.
    pub platform: BTreeMap<Os, BuildTable>,
    /// `[profile.<name>.build]`.
    pub profile_build: BTreeMap<Profile, BuildTable>,
    /// `[profile.<name>]`.
    pub profiles: BTreeMap<Profile, ProfileTable>,
    /// `[[bin]]`; `None` when none is written, so executables are found
    /// automatically (`build.md`).
    pub bins: Option<Vec<Bin>>,
    /// `[test]`.
    pub test: Test,
    /// `[fmt]`, read as written; what its keys mean is the formatter's.
    pub fmt: Option<toml::Table>,
    /// `[lint]`, read as written; what its keys mean is the linter's.
    pub lint: Option<toml::Table>,
}

/// `[package]`.
#[derive(Clone, Debug, PartialEq)]
pub struct Package {
    /// `name`.
    pub name: String,
    /// `version`.
    pub version: semver::Version,
    /// `lang`.
    pub lang: Lang,
    /// `edition`, or the default for `lang`.
    pub edition: Edition,
    /// `description`.
    pub description: Option<String>,
    /// `license`.
    pub license: Option<String>,
}

/// The keys `[build]`, `[platform.<OS>.build]` and `[profile.<name>.build]`
/// share; `None` where the table does not say.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BuildTable {
    /// `warnings`.
    pub warnings: Option<Warnings>,
    /// `warnings-as-errors`.
    pub warnings_as_errors: Option<bool>,
    /// `defines`.
    pub defines: Option<Vec<String>>,
    /// `include-dirs`, resolved against the project root.
    pub include_dirs: Option<Vec<PathBuf>>,
    /// `links`.
    pub links: Option<Vec<String>>,
    /// `lib-dirs`, resolved against the project root.
    pub lib_dirs: Option<Vec<PathBuf>>,
    /// `link-flags`.
    pub link_flags: Option<Vec<String>>,
    /// `runtime`.
    pub runtime: Option<Runtime>,
    /// `exceptions`.
    pub exceptions: Option<bool>,
    /// `rtti`.
    pub rtti: Option<bool>,
}

/// `[profile.<name>]`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProfileTable {
    /// `opt-level`.
    pub opt_level: Option<OptLevel>,
    /// `debug-info`.
    pub debug_info: Option<bool>,
    /// `debug-assertions`.
    pub debug_assertions: Option<bool>,
    /// `lto`.
    pub lto: Option<bool>,
}

/// One `[[bin]]`, its defaults filled in.
#[derive(Clone, Debug, PartialEq)]
pub struct Bin {
    /// `name`, or the package name.
    pub name: String,
    /// `path`, or `src/main.cpp` (`src/main.c` for C), resolved against the
    /// project root.
    pub path: PathBuf,
    /// `subsystem`.
    pub subsystem: Subsystem,
}

/// `[test]`.
#[derive(Clone, Debug, PartialEq)]
pub struct Test {
    /// `framework`; only `cuno` exists so far.
    pub framework: Framework,
}

macro_rules! choices {
    ($(#[$doc:meta])* $name:ident { $($(#[$vdoc:meta])* $variant:ident = $text:literal),+ $(,)? }) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
        pub enum $name { $($(#[$vdoc])* $variant),+ }

        impl $name {
            /// Every value, as written in the file.
            pub const ALL: &[($name, &str)] = &[$(($name::$variant, $text)),+];

            /// The value as written in the file.
            pub fn as_str(self) -> &'static str {
                match self { $($name::$variant => $text),+ }
            }

            fn parse(text: &str) -> Option<$name> {
                Self::ALL.iter().find(|(_, t)| *t == text).map(|(v, _)| *v)
            }
        }
    };
}

choices!(
    /// `[package] lang`.
    Lang { /// C.
    C = "c", /// C++.
    Cxx = "c++" }
);
choices!(
    /// `[package] edition`.
    Edition { /// C++17.
    Cxx17 = "c++17", /// C++20.
    Cxx20 = "c++20", /// C++23.
    Cxx23 = "c++23", /// C11.
    C11 = "c11", /// C17.
    C17 = "c17", /// C23.
    C23 = "c23" }
);
choices!(
    /// `warnings`.
    Warnings { /// The compiler's own.
    Default = "default", /// The recommended set.
    All = "all", /// Strict conformance too.
    Pedantic = "pedantic" }
);
choices!(
    /// `runtime`.
    Runtime { /// The C runtime as a DLL.
    Dynamic = "dynamic", /// The C runtime in the executable.
    Static = "static" }
);
choices!(
    /// `[[bin]] subsystem`.
    Subsystem { /// A console program.
    Console = "console", /// A program without a console window.
    Windows = "windows" }
);
choices!(
    /// `<OS>` in `[platform.<OS>.build]`.
    Os { /// Windows.
    Windows = "windows", /// Linux.
    Linux = "linux", /// macOS.
    Macos = "macos", /// Android.
    Android = "android", /// WebAssembly.
    Wasm = "wasm" }
);
choices!(
    /// `<compiler>` in `[build.<compiler>]` and `[toolchain.version]`.
    CompilerName { /// MSVC.
    Msvc = "msvc", /// clang.
    Clang = "clang", /// clang-cl.
    ClangCl = "clang-cl", /// GCC.
    Gcc = "gcc", /// safec.
    Safec = "safec" }
);
choices!(
    /// A profile; only the two built in exist so far.
    Profile { /// The default.
    Dev = "dev", /// `--release`.
    Release = "release" }
);
choices!(
    /// `[test] framework`.
    Framework { /// cuno's own.
    Cuno = "cuno" }
);

/// `opt-level`: `0` to `3`, or `"s"` for size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptLevel {
    /// `0`.
    O0,
    /// `1`.
    O1,
    /// `2`.
    O2,
    /// `3`.
    O3,
    /// `"s"`.
    Size,
}

impl Lang {
    fn default_edition(self) -> Edition {
        match self {
            Lang::C => Edition::C17,
            Lang::Cxx => Edition::Cxx20,
        }
    }
}

impl Edition {
    fn lang(self) -> Lang {
        match self {
            Edition::C11 | Edition::C17 | Edition::C23 => Lang::C,
            _ => Lang::Cxx,
        }
    }
}

impl Default for Test {
    fn default() -> Test {
        Test {
            framework: Framework::Cuno,
        }
    }
}

// ---- the file as written ----

#[derive(Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
#[serde(expecting = "a table")]
struct RawManifest {
    package: Option<RawPackage>,
    toolchain: Option<RawToolchain>,
    build: Option<RawBuild>,
    platform: Option<BTreeMap<String, RawPlatform>>,
    profile: Option<BTreeMap<String, RawProfile>>,
    bin: Option<Vec<RawBin>>,
    lib: Option<toml::Value>,
    dependencies: Option<toml::Value>,
    test: Option<RawTest>,
    fmt: Option<toml::Table>,
    lint: Option<toml::Table>,
}

#[derive(Deserialize, Default)]
#[serde(expecting = "a table")]
struct RawPackage {
    name: Option<String>,
    version: Option<String>,
    lang: Option<String>,
    edition: Option<String>,
    description: Option<String>,
    license: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(expecting = "a table")]
struct RawToolchain {
    compiler: Option<String>,
    version: Option<BTreeMap<String, String>>,
}

/// One struct for every build table: the keys only `[build]` takes are
/// warned about where they appear elsewhere.
#[derive(Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
#[serde(expecting = "a table")]
struct RawBuild {
    warnings: Option<String>,
    warnings_as_errors: Option<bool>,
    defines: Option<Vec<String>>,
    include_dirs: Option<Vec<PathBuf>>,
    links: Option<Vec<String>>,
    lib_dirs: Option<Vec<PathBuf>>,
    link_flags: Option<Vec<String>>,
    runtime: Option<String>,
    exceptions: Option<bool>,
    rtti: Option<bool>,
    precompiled_header: Option<toml::Value>,
    unity: Option<toml::Value>,
    target_dir: Option<PathBuf>,
    rule: Option<toml::Value>,
    msvc: Option<RawFlags>,
    clang: Option<RawFlags>,
    #[serde(rename = "clang-cl")]
    clang_cl: Option<RawFlags>,
    gcc: Option<RawFlags>,
    safec: Option<RawFlags>,
}

#[derive(Deserialize, Default)]
#[serde(expecting = "a table")]
struct RawFlags {
    flags: Option<Vec<String>>,
}

#[derive(Deserialize, Default)]
#[serde(expecting = "a table")]
struct RawPlatform {
    build: Option<RawBuild>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
#[serde(expecting = "a table")]
struct RawProfile {
    opt_level: Option<toml::Value>,
    debug_info: Option<bool>,
    debug_assertions: Option<bool>,
    lto: Option<bool>,
    inherits: Option<toml::Value>,
    build: Option<RawBuild>,
}

#[derive(Deserialize, Default)]
#[serde(expecting = "a table")]
struct RawBin {
    name: Option<String>,
    path: Option<PathBuf>,
    subsystem: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(expecting = "a table")]
struct RawTest {
    framework: Option<String>,
}

// ---- reading ----

/// Reads the project's `cuno.toml`. `cwd` decides how the file is shown in
/// messages.
pub fn load_manifest(project: &Project, cwd: &Path) -> Result<Loaded<Manifest>, ConfigError> {
    let path = project.manifest();
    let text = std::fs::read_to_string(&path).map_err(|e| ConfigError {
        message: format!("cannot read {}: {e}", path.display()),
        location: None,
        help: None,
    })?;
    parse_manifest(&text, &shown_path(&path, cwd), &project.root)
}

/// Reads `text` as `cuno.toml`; `file` is how messages name it, and relative
/// paths resolve against `root`.
pub fn parse_manifest(
    text: &str,
    file: &Path,
    root: &Path,
) -> Result<Loaded<Manifest>, ConfigError> {
    let doc = Doc::parse(text, file)?;
    let (raw, unknown): (RawManifest, _) = doc.deserialize()?;
    let mut check = Check {
        doc: &doc,
        root,
        unknown,
        warnings: Vec::new(),
    };
    let manifest = check.manifest(raw)?;
    let mut warnings = check.warnings;
    warnings.extend(check.unknown.iter().map(|path| doc.unknown(path)));
    warnings.sort_by_key(|w| w.location.as_ref().map(|l| (l.line, l.column)));
    Ok(Loaded {
        value: manifest,
        warnings,
    })
}

struct Check<'a> {
    doc: &'a Doc<'a>,
    root: &'a Path,
    /// The dotted paths of keys nothing read; turned into warnings at the end.
    unknown: Vec<String>,
    warnings: Vec<ConfigWarning>,
}

impl Check<'_> {
    fn error(&self, path: &str, message: String) -> ConfigError {
        self.doc.error(path, message, None)
    }

    fn not_yet(&self, path: &str, what: &str) -> ConfigError {
        self.doc.error(
            path,
            format!("{what} is not available yet ({})", self.doc.file.display()),
            Some(format!("remove `{path}`; it comes in a later release")),
        )
    }

    fn ignored(&mut self, path: &str, why: &str) {
        let message = format!("`{path}` in {} is ignored: {why}", self.doc.file.display());
        self.warnings.push(self.doc.warning(path, message));
    }

    fn choice<T: Copy>(
        &self,
        path: &str,
        value: Option<String>,
        parse: fn(&str) -> Option<T>,
        all: &[(T, &str)],
    ) -> Result<Option<T>, ConfigError> {
        let Some(text) = value else { return Ok(None) };
        parse(&text).map(Some).ok_or_else(|| {
            let allowed: Vec<&str> = all.iter().map(|(_, t)| *t).collect();
            self.error(
                path,
                format!(
                    "`{path}` is \"{text}\" in {}, which is not one of {}",
                    self.doc.file.display(),
                    allowed.join(", ")
                ),
            )
        })
    }

    fn paths(
        &self,
        path: &str,
        values: Option<Vec<PathBuf>>,
    ) -> Result<Option<Vec<PathBuf>>, ConfigError> {
        values
            .map(|values| values.iter().map(|v| self.path(path, v)).collect())
            .transpose()
    }

    fn path(&self, path: &str, value: &Path) -> Result<PathBuf, ConfigError> {
        resolve_path(value, self.root).map_err(|e| ConfigError {
            location: Some(self.doc.locate(path)),
            ..e
        })
    }

    fn manifest(&mut self, raw: RawManifest) -> Result<Manifest, ConfigError> {
        if raw.lib.is_some() {
            return Err(self.not_yet("lib", "`[lib]`"));
        }
        if raw.dependencies.is_some() {
            return Err(self.not_yet("dependencies", "`[dependencies]`"));
        }
        let package = self.package(raw.package)?;
        let toolchain = raw.toolchain.unwrap_or_default();
        let constraints = self.constraints(toolchain.version)?;

        let mut build = raw.build.unwrap_or_default();
        if build.rule.is_some() {
            return Err(self.not_yet("build.rule", "`[[build.rule]]`"));
        }
        let target_dir = build
            .target_dir
            .take()
            .map(|dir| self.path("build.target-dir", &dir))
            .transpose()?;
        let mut compiler_flags = BTreeMap::new();
        for (name, flags) in [
            (CompilerName::Msvc, build.msvc.take()),
            (CompilerName::Clang, build.clang.take()),
            (CompilerName::ClangCl, build.clang_cl.take()),
            (CompilerName::Gcc, build.gcc.take()),
            (CompilerName::Safec, build.safec.take()),
        ] {
            if let Some(flags) = flags.and_then(|f| f.flags) {
                compiler_flags.insert(name, flags);
            }
        }
        let build = self.build_table("build", build)?;

        let mut platform = BTreeMap::new();
        for (os, table) in raw.platform.unwrap_or_default() {
            let prefix = format!("platform.{}", key(&os));
            let Some(parsed) = Os::parse(&os) else {
                // One warning for the table, not one more for each key in it.
                let inside = format!("{prefix}.");
                self.unknown.retain(|path| !path.starts_with(&inside));
                self.unknown.push(prefix);
                continue;
            };
            if let Some(table) = table.build {
                platform.insert(
                    parsed,
                    self.nested_build(&format!("{prefix}.build"), table)?,
                );
            }
        }

        let mut profiles = BTreeMap::new();
        let mut profile_build = BTreeMap::new();
        for (name, table) in raw.profile.unwrap_or_default() {
            let prefix = format!("profile.{}", key(&name));
            let Some(profile) = Profile::parse(&name) else {
                return Err(self.not_yet(&prefix, "a profile other than `dev` and `release`"));
            };
            if table.inherits.is_some() {
                return Err(self.not_yet(&format!("{prefix}.inherits"), "`inherits`"));
            }
            let opt_level = self.opt_level(&format!("{prefix}.opt-level"), table.opt_level)?;
            profiles.insert(
                profile,
                ProfileTable {
                    opt_level,
                    debug_info: table.debug_info,
                    debug_assertions: table.debug_assertions,
                    lto: table.lto,
                },
            );
            if let Some(build) = table.build {
                profile_build.insert(
                    profile,
                    self.nested_build(&format!("{prefix}.build"), build)?,
                );
            }
        }

        let bins = raw
            .bin
            .map(|bins| {
                bins.into_iter()
                    .enumerate()
                    .map(|(i, bin)| self.bin(i, bin, &package))
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?;

        let test = match raw.test.and_then(|t| t.framework) {
            None => Test::default(),
            Some(name) if ["gtest", "catch2", "doctest"].contains(&name.as_str()) => {
                return Err(self.not_yet("test.framework", &format!("the test framework `{name}`")));
            }
            Some(name) => Test {
                framework: self
                    .choice(
                        "test.framework",
                        Some(name),
                        Framework::parse,
                        Framework::ALL,
                    )?
                    .unwrap_or(Framework::Cuno),
            },
        };

        Ok(Manifest {
            package,
            compiler: toolchain.compiler,
            constraints,
            target_dir,
            build,
            compiler_flags,
            platform,
            profile_build,
            profiles,
            bins,
            test,
            fmt: raw.fmt,
            lint: raw.lint,
        })
    }

    fn package(&mut self, raw: Option<RawPackage>) -> Result<Package, ConfigError> {
        let Some(raw) = raw else {
            return Err(ConfigError {
                message: format!("{} has no `[package]` table", self.doc.file.display()),
                location: Some(self.doc.locate("package")),
                help: Some("add `[package]` with `name` and `version`".to_owned()),
            });
        };
        let missing = |key: &str| {
            self.doc.error(
                "package",
                format!("`package.{key}` is missing in {}", self.doc.file.display()),
                None,
            )
        };
        let name = raw.name.ok_or_else(|| missing("name"))?;
        if !valid_name(&name) {
            return Err(self.doc.error(
                "package.name",
                format!(
                    "`package.name` \"{name}\" in {} is not a valid package name",
                    self.doc.file.display()
                ),
                Some("use letters, digits, `-` and `_`, starting with a letter".to_owned()),
            ));
        }
        let version_text = raw.version.ok_or_else(|| missing("version"))?;
        let version = semver::Version::parse(&version_text).map_err(|e| {
            self.doc.error(
                "package.version",
                format!(
                    "`package.version` \"{version_text}\" in {} is not a version: {e}",
                    self.doc.file.display()
                ),
                Some("write a Semantic Versioning version, such as \"0.1.0\"".to_owned()),
            )
        })?;
        let lang = self
            .choice("package.lang", raw.lang, Lang::parse, Lang::ALL)?
            .unwrap_or(Lang::Cxx);
        let edition = self
            .choice("package.edition", raw.edition, Edition::parse, Edition::ALL)?
            .unwrap_or(lang.default_edition());
        if edition.lang() != lang {
            return Err(self.error(
                "package.edition",
                format!(
                    "`package.edition` \"{}\" in {} is not a {} standard",
                    edition.as_str(),
                    self.doc.file.display(),
                    lang.as_str()
                ),
            ));
        }
        Ok(Package {
            name,
            version,
            lang,
            edition,
            description: raw.description,
            license: raw.license,
        })
    }

    fn constraints(
        &mut self,
        raw: Option<BTreeMap<String, String>>,
    ) -> Result<Constraints, ConfigError> {
        let mut constraints = Constraints::default();
        for (name, text) in raw.unwrap_or_default() {
            let path = format!("toolchain.version.{}", key(&name));
            let slot = match CompilerName::parse(&name) {
                Some(CompilerName::Msvc) => &mut constraints.msvc,
                Some(CompilerName::Clang) => &mut constraints.clang,
                Some(CompilerName::ClangCl) => &mut constraints.clang_cl,
                // Harmless until that compiler is used (manifest.md).
                Some(CompilerName::Gcc | CompilerName::Safec) => continue,
                None => {
                    self.unknown.push(path);
                    continue;
                }
            };
            *slot = Some(VersionReq::parse(&name, &text).map_err(|e| ConfigError {
                message: e.to_string(),
                location: Some(self.doc.locate(&path)),
                help: e.help(),
            })?);
        }
        Ok(constraints)
    }

    /// A `[platform.<OS>.build]` or `[profile.<name>.build]`: what only
    /// `[build]` takes is warned about and ignored.
    fn nested_build(&mut self, prefix: &str, mut raw: RawBuild) -> Result<BuildTable, ConfigError> {
        let only_in_build = "it can be written only in `[build]`";
        if raw.target_dir.take().is_some() {
            self.ignored(&format!("{prefix}.target-dir"), only_in_build);
        }
        // Ignoring a rule would build something other than written
        // (manifest.md, "Implemented in MVP"), wherever it is.
        if raw.rule.take().is_some() {
            return Err(self.not_yet(&format!("{prefix}.rule"), "`[[build.rule]]`"));
        }
        for (name, flags) in [
            ("msvc", raw.msvc.take()),
            ("clang", raw.clang.take()),
            ("clang-cl", raw.clang_cl.take()),
            ("gcc", raw.gcc.take()),
            ("safec", raw.safec.take()),
        ] {
            if flags.is_some() {
                self.ignored(&format!("{prefix}.{name}"), only_in_build);
            }
        }
        self.build_table(prefix, raw)
    }

    fn build_table(&mut self, prefix: &str, raw: RawBuild) -> Result<BuildTable, ConfigError> {
        let not_supported = "it is not supported yet";
        // Left at its default, a key ignores nothing, so it is not warned
        // about (manifest.md, "Implemented in MVP").
        let at_default = |value: &Option<toml::Value>, default: &toml::Value| {
            value.as_ref().is_none_or(|v| v == default)
        };
        if !at_default(&raw.precompiled_header, &toml::Value::String(String::new())) {
            self.ignored(&format!("{prefix}.precompiled-header"), not_supported);
        }
        if !at_default(&raw.unity, &toml::Value::Boolean(false)) {
            self.ignored(&format!("{prefix}.unity"), not_supported);
        }
        Ok(BuildTable {
            warnings: self.choice(
                &format!("{prefix}.warnings"),
                raw.warnings,
                Warnings::parse,
                Warnings::ALL,
            )?,
            warnings_as_errors: raw.warnings_as_errors,
            defines: raw.defines,
            include_dirs: self.paths(&format!("{prefix}.include-dirs"), raw.include_dirs)?,
            links: raw.links,
            lib_dirs: self.paths(&format!("{prefix}.lib-dirs"), raw.lib_dirs)?,
            link_flags: raw.link_flags,
            runtime: self.choice(
                &format!("{prefix}.runtime"),
                raw.runtime,
                Runtime::parse,
                Runtime::ALL,
            )?,
            exceptions: raw.exceptions,
            rtti: raw.rtti,
        })
    }

    fn opt_level(
        &self,
        path: &str,
        raw: Option<toml::Value>,
    ) -> Result<Option<OptLevel>, ConfigError> {
        let Some(value) = raw else { return Ok(None) };
        let level = match &value {
            toml::Value::Integer(0) => OptLevel::O0,
            toml::Value::Integer(1) => OptLevel::O1,
            toml::Value::Integer(2) => OptLevel::O2,
            toml::Value::Integer(3) => OptLevel::O3,
            toml::Value::String(s) if s == "s" => OptLevel::Size,
            other => {
                return Err(self.error(
                    path,
                    format!(
                        "`{path}` is {other} in {}, which is not one of 0, 1, 2, 3, \"s\"",
                        self.doc.file.display()
                    ),
                ));
            }
        };
        Ok(Some(level))
    }

    fn bin(&self, index: usize, raw: RawBin, package: &Package) -> Result<Bin, ConfigError> {
        let prefix = format!("bin.{index}");
        let default_path = match package.lang {
            Lang::C => "src/main.c",
            Lang::Cxx => "src/main.cpp",
        };
        Ok(Bin {
            name: raw.name.unwrap_or_else(|| package.name.clone()),
            path: match raw.path {
                Some(path) => self.path(&format!("{prefix}.path"), &path)?,
                None => self.root.join(default_path),
            },
            subsystem: self
                .choice(
                    &format!("{prefix}.subsystem"),
                    raw.subsystem,
                    Subsystem::parse,
                    Subsystem::ALL,
                )?
                .unwrap_or(Subsystem::Console),
        })
    }
}

/// The package name rule `cuno new` applies (`commands.md`): ASCII letters,
/// digits, `-` and `_`, starting with a letter.
fn valid_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_alphabetic())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use cuno_core::ExitClass;

    const MINIMAL: &str = "[package]\nname = \"app\"\nversion = \"0.1.0\"\n";

    fn root() -> PathBuf {
        std::env::temp_dir().join("cuno-project")
    }

    fn read(text: &str) -> Result<Loaded<Manifest>, ConfigError> {
        parse_manifest(text, Path::new("cuno.toml"), &root())
    }

    /// `MINIMAL` and then `rest`.
    fn with(rest: &str) -> String {
        format!("{MINIMAL}{rest}")
    }

    /// Mutation: drop or misread any key of the full example, or give a key
    /// that is not written a value; this test fails.
    #[test]
    fn every_key_is_read_and_the_unwritten_ones_stay_empty() {
        let full = r#"
[package]
name = "mygame"
version = "0.1.0"
lang = "c++"
edition = "c++23"
description = "d"
license = "MIT"

[toolchain]
compiler = "clang"

[toolchain.version]
msvc = ">=19.29"
clang = ">=17, <30"
clang-cl = "=22"

[build]
warnings = "pedantic"
warnings-as-errors = true
defines = ["A=1"]
include-dirs = ["inc"]
links = ["d3d12"]
lib-dirs = ["lib"]
link-flags = ["/X"]
runtime = "static"
exceptions = false
rtti = false
target-dir = "out"

[build.msvc]
flags = ["/Zc:__cplusplus"]

[build.clang-cl]
flags = ["-fcolor-diagnostics"]

[platform.windows.build]
defines = ["NOMINMAX"]

[profile.dev.build]
defines = ["GAME_DEBUG"]

[profile.release]
opt-level = "s"
debug-info = true
debug-assertions = false
lto = true

[[bin]]
name = "game"
path = "src/game.cpp"
subsystem = "windows"

[test]
framework = "cuno"

[fmt]
indent-width = 2
"#;
        let loaded = read(full).unwrap();
        assert_eq!(loaded.warnings, []);
        let m = loaded.value;
        let r = root();
        assert_eq!(m.package.name, "mygame");
        assert_eq!(m.package.version, semver::Version::new(0, 1, 0));
        assert_eq!(
            (m.package.lang, m.package.edition),
            (Lang::Cxx, Edition::Cxx23)
        );
        assert_eq!(m.package.description.as_deref(), Some("d"));
        assert_eq!(m.package.license.as_deref(), Some("MIT"));
        assert_eq!(m.compiler.as_deref(), Some("clang"));
        assert_eq!(m.constraints.msvc.as_ref().unwrap().to_string(), ">=19.29");
        assert_eq!(
            m.constraints.clang.as_ref().unwrap().to_string(),
            ">=17, <30"
        );
        assert_eq!(m.constraints.clang_cl.as_ref().unwrap().to_string(), "=22");
        assert_eq!(m.target_dir, Some(r.join("out")));
        assert_eq!(
            m.build,
            BuildTable {
                warnings: Some(Warnings::Pedantic),
                warnings_as_errors: Some(true),
                defines: Some(vec!["A=1".into()]),
                include_dirs: Some(vec![r.join("inc")]),
                links: Some(vec!["d3d12".into()]),
                lib_dirs: Some(vec![r.join("lib")]),
                link_flags: Some(vec!["/X".into()]),
                runtime: Some(Runtime::Static),
                exceptions: Some(false),
                rtti: Some(false),
            }
        );
        assert_eq!(
            m.compiler_flags,
            BTreeMap::from([
                (CompilerName::Msvc, vec!["/Zc:__cplusplus".to_owned()]),
                (
                    CompilerName::ClangCl,
                    vec!["-fcolor-diagnostics".to_owned()]
                ),
            ])
        );
        assert_eq!(
            m.platform[&Os::Windows].defines,
            Some(vec!["NOMINMAX".into()])
        );
        assert_eq!(
            m.profile_build[&Profile::Dev].defines,
            Some(vec!["GAME_DEBUG".into()])
        );
        assert_eq!(
            m.profiles[&Profile::Release],
            ProfileTable {
                opt_level: Some(OptLevel::Size),
                debug_info: Some(true),
                debug_assertions: Some(false),
                lto: Some(true),
            }
        );
        assert_eq!(
            m.bins,
            Some(vec![Bin {
                name: "game".into(),
                path: r.join("src/game.cpp"),
                subsystem: Subsystem::Windows,
            }])
        );
        assert_eq!(m.test.framework, Framework::Cuno);
        assert_eq!(m.fmt.unwrap()["indent-width"].as_integer(), Some(2));

        let m = read(MINIMAL).unwrap().value;
        assert_eq!(
            (m.package.lang, m.package.edition),
            (Lang::Cxx, Edition::Cxx20)
        );
        assert_eq!(m.compiler, None);
        assert_eq!(m.constraints, Constraints::default());
        assert_eq!(m.target_dir, None);
        assert_eq!(m.build, BuildTable::default());
        assert!(m.compiler_flags.is_empty() && m.platform.is_empty() && m.profiles.is_empty());
        assert_eq!(m.bins, None);
        assert_eq!((m.fmt, m.lint), (None, None));
    }

    /// Mutation: change a default of `[[bin]]` or the edition C gets; this
    /// test fails.
    #[test]
    fn a_bin_and_a_c_package_get_their_defaults() {
        let m = read("[package]\nname = \"tool\"\nversion = \"1.0.0\"\nlang = \"c\"\n\n[[bin]]\n")
            .unwrap()
            .value;
        assert_eq!(m.package.edition, Edition::C17);
        assert_eq!(
            m.bins.unwrap(),
            [Bin {
                name: "tool".into(),
                path: root().join("src/main.c"),
                subsystem: Subsystem::Console,
            }]
        );
    }

    /// Mutation: accept any of these, or report it on another line; this
    /// test fails. Every one is a setup problem (exit code 3).
    #[test]
    fn each_invalid_setting_is_an_error_at_its_line() {
        let whole: &[(&str, usize, &str)] = &[
            (
                "[package]\nname = \"a\"\nversion = \"1.0.0\"\n[build]\nrtti = \"yes\"\n",
                5,
                "invalid type",
            ),
            ("[toolchain]\n", 1, "no `[package]` table"),
            (
                "[package]\nversion = \"1.0.0\"\n",
                1,
                "`package.name` is missing",
            ),
            (
                "[package]\nname = \"a\"\n",
                1,
                "`package.version` is missing",
            ),
            (
                "[package]\nname = \"1app\"\nversion = \"1.0.0\"\n",
                2,
                "not a valid package name",
            ),
            (
                "[package]\nname = \"a b\"\nversion = \"1.0.0\"\n",
                2,
                "not a valid package name",
            ),
            (
                "[package]\nname = \"a\"\nversion = \"1.0\"\n",
                3,
                "not a version",
            ),
            (
                "[package]\nname = \"a\"\nversion = \"1.0.0\"\nlang = \"c\"\nedition = \"c++20\"\n",
                5,
                "is not a c standard",
            ),
            (
                "[package]\nname = \"a\"\nversion = \"1.0.0\"\nlang = \"rust\"\n",
                4,
                "not one of c, c++",
            ),
            (
                "[package]\nname = \"a\"\nversion = \"1.0.0\"\nedition = \"c++98\"\n",
                4,
                "not one of",
            ),
        ];
        let appended: &[(&str, usize, &str)] = &[
            (
                "[build]\nwarnings = \"loud\"\n",
                5,
                "not one of default, all, pedantic",
            ),
            (
                "[build]\nruntime = \"shared\"\n",
                5,
                "not one of dynamic, static",
            ),
            (
                "[[bin]]\nsubsystem = \"gui\"\n",
                5,
                "not one of console, windows",
            ),
            (
                "[profile.dev]\nopt-level = 4\n",
                5,
                "not one of 0, 1, 2, 3, \"s\"",
            ),
            (
                "[toolchain.version]\nclang = \"~17\"\n",
                5,
                "not a version requirement",
            ),
            (
                "[test]\nframework = \"gtest\"\n",
                5,
                "`gtest` is not available yet",
            ),
            ("[test]\nframework = \"unity\"\n", 5, "not one of cuno"),
            (
                "[lib]\nkind = \"static\"\n",
                4,
                "`[lib]` is not available yet",
            ),
            (
                "[dependencies]\nfmt = \"10\"\n",
                4,
                "`[dependencies]` is not available yet",
            ),
            (
                "[[build.rule]]\nx = 1\n",
                4,
                "`[[build.rule]]` is not available yet",
            ),
            (
                "[profile.bench]\nopt-level = 3\n",
                4,
                "other than `dev` and `release`",
            ),
            (
                "[profile.release]\ninherits = \"dev\"\n",
                5,
                "`inherits` is not available yet",
            ),
        ];
        let cases = whole
            .iter()
            .map(|(t, l, m)| (t.to_string(), *l, *m))
            .chain(appended.iter().map(|(t, l, m)| (with(t), *l, *m)));
        for (text, line, message) in cases {
            let err = read(&text).expect_err(&text);
            assert!(err.message.contains(message), "{text}\n{err}");
            assert_eq!(
                err.location.as_ref().map(|l| l.line),
                Some(line),
                "{text}\n{err}"
            );
            assert_eq!(err.exit_class(), ExitClass::Setup);
        }
    }

    /// Mutation: turn any of these into an error, drop its warning, report
    /// it on another line, or keep the value it ignores; this test fails.
    #[test]
    fn what_is_read_past_is_warned_about_at_its_line() {
        let text = with(concat!(
            "[build]\n",                      // 4
            "unity = true\n",                 // 5
            "precompiled-header = \"p.h\"\n", // 6
            "wanings = \"all\"\n",            // 7
            "[toolchain.version]\n",          // 8
            "gcc = \">=13\"\n",               // 9
            "nvcc = \">=12\"\n",              // 10
            "[platform.beos.build]\n",        // 11
            "defines = [\"X\"]\n",            // 12
            "[platform.linux.build]\n",       // 13
            "target-dir = \"t\"\n",           // 14
            "[profile.dev.build.msvc]\n",     // 15
            "flags = [\"/W0\"]\n",            // 16
            "[package2]\n",                   // 17
        ));
        let loaded = read(&text).unwrap();
        let got: Vec<(usize, String)> = loaded
            .warnings
            .iter()
            .map(|w| (w.location.as_ref().unwrap().line, w.message.clone()))
            .collect();
        let expect: Vec<(usize, String)> = [
            (5, "`build.unity` in cuno.toml is ignored: it is not supported yet"),
            (
                6,
                "`build.precompiled-header` in cuno.toml is ignored: it is not supported yet",
            ),
            (7, "unknown key `build.wanings` in cuno.toml, ignored"),
            (10, "unknown key `toolchain.version.nvcc` in cuno.toml, ignored"),
            (11, "unknown key `platform.beos` in cuno.toml, ignored"),
            (
                14,
                "`platform.linux.build.target-dir` in cuno.toml is ignored: it can be written only in `[build]`",
            ),
            (
                15,
                "`profile.dev.build.msvc` in cuno.toml is ignored: it can be written only in `[build]`",
            ),
            (17, "unknown key `package2` in cuno.toml, ignored"),
        ]
        .iter()
        .map(|(l, m)| (*l, m.to_string()))
        .collect();
        assert_eq!(got, expect);
        let m = loaded.value;
        assert_eq!(m.target_dir, None);
        assert_eq!(m.platform.len(), 1);
        assert_eq!(m.platform[&Os::Linux], BuildTable::default());
        assert_eq!(m.profile_build[&Profile::Dev], BuildTable::default());
        assert!(m.compiler_flags.is_empty());
        assert_eq!(m.constraints, Constraints::default());
    }

    /// Mutation: resolve a relative path against anything but the project
    /// root, or let a drive-dependent Windows path through; this test fails.
    #[test]
    fn paths_resolve_against_the_project_root() {
        let m = read(&with(concat!(
            "[build]\ninclude-dirs = [\"a\"]\nlib-dirs = [\"b\"]\ntarget-dir = \"t\"\n",
            "[platform.windows.build]\ninclude-dirs = [\"c\"]\n",
            "[[bin]]\npath = \"x/m.cpp\"\n",
        )))
        .unwrap()
        .value;
        let r = root();
        assert_eq!(m.build.include_dirs, Some(vec![r.join("a")]));
        assert_eq!(m.build.lib_dirs, Some(vec![r.join("b")]));
        assert_eq!(m.target_dir, Some(r.join("t")));
        assert_eq!(
            m.platform[&Os::Windows].include_dirs,
            Some(vec![r.join("c")])
        );
        assert_eq!(m.bins.unwrap()[0].path, r.join("x/m.cpp"));
        if cfg!(windows) {
            let err = read(&with("[build]\ninclude-dirs = [\"/inc\"]\n")).unwrap_err();
            assert_eq!(err.location.as_ref().unwrap().line, 5);
            assert_eq!(err.exit_class(), ExitClass::Setup);
        }
    }

    /// Mutation: show the file by its full path when it is under the current
    /// directory, or stop reading it from the project; this test fails.
    #[test]
    fn a_project_file_is_loaded_and_named_as_the_user_would() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("cuno.toml"), with("[build]\nnope = 1\n")).unwrap();
        let project = Project {
            root: dir.path().to_owned(),
        };
        let loaded = load_manifest(&project, dir.path()).unwrap();
        assert_eq!(loaded.value.package.name, "app");
        assert_eq!(
            loaded.warnings[0].to_string(),
            "unknown key `build.nope` in cuno.toml, ignored\n --> cuno.toml:5:1"
        );
        let missing = Project {
            root: dir.path().join("none"),
        };
        let err = load_manifest(&missing, dir.path()).unwrap_err();
        assert!(err.message.starts_with("cannot read"), "{err}");
    }

    /// Mutation: file any compiler's `flags` under another, or drop them;
    /// this test fails.
    #[test]
    fn each_compilers_flags_are_its_own() {
        let m = read(&with(concat!(
            "[build.msvc]\nflags = [\"m\"]\n",
            "[build.clang]\nflags = [\"c\"]\n",
            "[build.clang-cl]\nflags = [\"cc\"]\n",
            "[build.gcc]\nflags = [\"g\"]\n",
            "[build.safec]\nflags = [\"s\"]\n",
        )))
        .unwrap()
        .value;
        let flags = |name| vec![String::from(name)];
        assert_eq!(
            m.compiler_flags,
            BTreeMap::from([
                (CompilerName::Msvc, flags("m")),
                (CompilerName::Clang, flags("c")),
                (CompilerName::ClangCl, flags("cc")),
                (CompilerName::Gcc, flags("g")),
                (CompilerName::Safec, flags("s")),
            ])
        );
    }

    /// Mutation: read any `opt-level` as another, or file a profile's table
    /// under the other profile; this test fails.
    #[test]
    fn each_opt_level_and_profile_is_read_as_written() {
        for (text, level) in [
            ("0", OptLevel::O0),
            ("1", OptLevel::O1),
            ("2", OptLevel::O2),
            ("3", OptLevel::O3),
            ("\"s\"", OptLevel::Size),
        ] {
            let m = read(&with(&format!(
                "[profile.dev]\nopt-level = {text}\n[profile.release.build]\ndefines = [\"R\"]\n"
            )))
            .unwrap()
            .value;
            assert_eq!(m.profiles[&Profile::Dev].opt_level, Some(level), "{text}");
            assert_eq!(m.profiles[&Profile::Release], ProfileTable::default());
            assert_eq!(
                m.profile_build[&Profile::Release].defines,
                Some(vec!["R".into()])
            );
            assert!(!m.profile_build.contains_key(&Profile::Dev));
        }
    }

    /// Mutation: misspell any value the spec lists, or tie an edition to the
    /// other language; this test fails.
    #[test]
    fn every_value_the_spec_lists_is_accepted() {
        let package = |lang: &str, edition: &str| {
            format!(
                "[package]\nname = \"a\"\nversion = \"1.0.0\"\nlang = \"{lang}\"\nedition = \"{edition}\"\n"
            )
        };
        for (lang, editions, other) in [
            ("c++", ["c++17", "c++20", "c++23"], "c"),
            ("c", ["c11", "c17", "c23"], "c++"),
        ] {
            for edition in editions {
                let m = read(&package(lang, edition)).unwrap().value;
                assert_eq!(m.package.edition.as_str(), edition);
                assert_eq!(m.package.lang.as_str(), lang);
                assert!(
                    read(&package(other, edition)).is_err(),
                    "{edition} under {other}"
                );
            }
        }
        for (warnings, expected) in [
            ("default", Warnings::Default),
            ("all", Warnings::All),
            ("pedantic", Warnings::Pedantic),
        ] {
            let m = read(&with(&format!("[build]\nwarnings = \"{warnings}\"\n"))).unwrap();
            assert_eq!(m.value.build.warnings, Some(expected));
        }
        for (runtime, expected) in [("dynamic", Runtime::Dynamic), ("static", Runtime::Static)] {
            let m = read(&with(&format!("[build]\nruntime = \"{runtime}\"\n"))).unwrap();
            assert_eq!(m.value.build.runtime, Some(expected));
        }
        for (subsystem, expected) in [
            ("console", Subsystem::Console),
            ("windows", Subsystem::Windows),
        ] {
            let m = read(&with(&format!("[[bin]]\nsubsystem = \"{subsystem}\"\n"))).unwrap();
            assert_eq!(m.value.bins.unwrap()[0].subsystem, expected);
        }
        let platforms: String = ["windows", "linux", "macos", "android", "wasm"]
            .iter()
            .map(|os| format!("[platform.{os}.build]\ndefines = [\"{os}\"]\n"))
            .collect();
        let loaded = read(&with(&platforms)).unwrap();
        assert_eq!(loaded.warnings, []);
        for (os, name) in [
            (Os::Windows, "windows"),
            (Os::Linux, "linux"),
            (Os::Macos, "macos"),
            (Os::Android, "android"),
            (Os::Wasm, "wasm"),
        ] {
            assert_eq!(loaded.value.platform[&os].defines, Some(vec![name.into()]));
        }
        let m = read(&with("[toolchain.version]\nsafec = \">=1\"\n")).unwrap();
        assert_eq!(m.warnings, []);
        let m = read(&with("[lint]\npreset = \"strict\"\n")).unwrap().value;
        assert_eq!(m.lint.unwrap()["preset"].as_str(), Some("strict"));
    }

    /// Mutation: reject `-` or `_` in a package name; this test fails.
    #[test]
    fn a_name_may_hold_hyphens_and_underscores() {
        let m = read("[package]\nname = \"my-app_2\"\nversion = \"1.0.0\"\n").unwrap();
        assert_eq!(m.value.package.name, "my-app_2");
    }

    /// Mutation: drop or change the `help:` of any of these errors; this
    /// test fails.
    #[test]
    fn errors_say_how_to_fix_them() {
        let help = |text: &str| read(text).unwrap_err().help;
        let cases = [
            (
                "[toolchain]\n".to_owned(),
                "add `[package]` with `name` and `version`",
            ),
            (
                "[package]\nname = \"1a\"\nversion = \"1.0.0\"\n".to_owned(),
                "use letters, digits, `-` and `_`, starting with a letter",
            ),
            (
                "[package]\nname = \"a\"\nversion = \"1\"\n".to_owned(),
                "write a Semantic Versioning version, such as \"0.1.0\"",
            ),
            (with("[lib]\n"), "remove `lib`; it comes in a later release"),
            (
                with("[toolchain.version]\nclang = \"17\"\n"),
                "write conditions such as \">=17\" or \">=19.29, <19.50\"",
            ),
        ];
        for (text, expected) in cases {
            assert_eq!(help(&text).as_deref(), Some(expected), "{text}");
        }
    }

    /// Mutation: keep, or warn about rather than reject, a rule under a
    /// nested build table; keep or stop warning about a compiler table
    /// there; or accept `catch2` / `doctest`; this test fails.
    #[test]
    fn nested_build_tables_take_only_the_shared_keys() {
        for name in ["msvc", "clang", "clang-cl", "gcc", "safec"] {
            let loaded = read(&with(&format!(
                "[platform.windows.build.{name}]\nflags = [\"x\"]\n"
            )))
            .unwrap();
            assert_eq!(loaded.warnings.len(), 1, "{name}");
            assert!(
                loaded.warnings[0]
                    .message
                    .contains(&format!("platform.windows.build.{name}")),
                "{}",
                loaded.warnings[0]
            );
            assert!(loaded.value.compiler_flags.is_empty());
        }
        for table in ["platform.windows.build", "profile.dev.build"] {
            let err = read(&with(&format!("[[{table}.rule]]\nx = 1\n"))).unwrap_err();
            assert!(
                err.message
                    .contains("`[[build.rule]]` is not available yet"),
                "{err}"
            );
            assert_eq!(err.location.unwrap().line, 4);
        }
        for framework in ["catch2", "doctest"] {
            let err = read(&with(&format!("[test]\nframework = \"{framework}\"\n"))).unwrap_err();
            assert!(err.message.contains("is not available yet"), "{err}");
        }
    }

    /// Mutation: warn about `unity` / `precompiled-header` at their defaults,
    /// report an unknown platform once per key inside it, split a quoted key
    /// at its dot, report an error in a later `[[bin]]` at the first, or name
    /// cuno's own types in a message; this test fails.
    #[test]
    fn messages_are_about_what_the_user_wrote() {
        let loaded = read(&with("[build]\nunity = false\nprecompiled-header = \"\"\n")).unwrap();
        assert_eq!(loaded.warnings, []);

        let loaded = read(&with(
            "[platform.\"a.b\".build]\ndefines = [\"X\"]\nnope = 1\n",
        ))
        .unwrap();
        let shown: Vec<String> = loaded.warnings.iter().map(|w| w.to_string()).collect();
        assert_eq!(
            shown,
            ["unknown key `platform.\"a.b\"` in cuno.toml, ignored\n --> cuno.toml:4:11"]
        );

        let err = read(&with(
            "[[bin]]\nname = \"a\"\n[[bin]]\nsubsystem = \"gui\"\n",
        ))
        .unwrap_err();
        assert_eq!(err.location.unwrap().line, 7);

        for text in [
            with("[build]\nwarnings = []\n"),
            "package = []\n".to_owned(),
            "package = \"x\"\n".to_owned(),
        ] {
            let err = read(&text).unwrap_err();
            assert!(!err.message.contains("Raw"), "{err}");
        }
        let err = read("package = \"x\"\n").unwrap_err();
        assert!(err.message.contains("expected a table"), "{err}");
    }
}
