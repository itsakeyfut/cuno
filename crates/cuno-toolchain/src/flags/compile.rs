//! The compile step's arguments, row by row from `docs/en/specs/build.md`,
//! "Settings and flags".

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::{BuildSettings, Edition, Lang, OptLevel, ProfileSettings, Runtime, Warnings};
use crate::compiler::{CompilerKind, Dependencies, Family, Syntax};
use crate::{ToolchainError, Version};

/// Whether a source is compiled to an object or only checked (`cuno check`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Compile to an object.
    Compile,
    /// Check the syntax and types only.
    Check,
}

/// The target triple, spelled once: every compiler cuno uses on Windows
/// builds for the MSVC ABI, so the objects link with MSVC-built libraries.
macro_rules! target {
    () => {
        "x86_64-pc-windows-msvc"
    };
}

/// The switch that names the target to an LLVM driver or tool.
pub const TARGET_SWITCH: &str = concat!("--target=", target!());

/// What one compile of the build is made from.
#[derive(Clone, Copy, Debug)]
pub struct CompileInput<'a> {
    /// The driver.
    pub kind: CompilerKind,
    /// This source's language, from its extension (`docs/en/specs/project-layout.md`,
    /// "Target files").
    pub lang: Lang,
    /// The standard for this source: the package's edition when `lang` is the
    /// package's language, otherwise that language's default.
    pub edition: Edition,
    /// The resolved build settings.
    pub build: &'a BuildSettings,
    /// The resolved profile.
    pub profile: &'a ProfileSettings,
    /// The include directories cuno always adds (`src/`, the bundled
    /// headers), before `build.include_dirs`.
    pub include_dirs: &'a [PathBuf],
    /// A test build, which defines `CUNO_TEST`.
    pub test: bool,
    /// Compile or check.
    pub mode: Mode,
}

/// The flags every source of the build gets, without the mode, the source
/// and the output. `[build.<compiler>] flags` come last.
pub fn compile_flags(input: &CompileInput<'_>) -> Vec<OsString> {
    let mut out = Flags::default();
    let msvc_style = match input.kind.syntax() {
        Syntax::Cl => true,
        Syntax::Gnu => false,
    };
    let cxx = input.lang == Lang::Cxx;
    let build = input.build;
    let profile = input.profile;

    // Always on.
    if msvc_style {
        out.push(&["/nologo", "/utf-8"]);
    }
    match input.kind.family() {
        Family::Llvm => out.push(&[TARGET_SWITCH]),
        Family::Msvc => {}
    }
    if msvc_style && cxx {
        out.push(&["/Zc:__cplusplus"]);
    }

    out.push(&[standard(input.kind, input.edition)]);

    out.push(match (build.warnings, input.kind) {
        (Warnings::Default, _) => &[][..],
        (Warnings::All, CompilerKind::Clang) => &["-Wall", "-Wextra"],
        (Warnings::All, CompilerKind::Msvc | CompilerKind::ClangCl) => &["/W4"],
        (Warnings::Pedantic, CompilerKind::Msvc) => &["/W4", "/permissive-"],
        (Warnings::Pedantic, CompilerKind::ClangCl) => &["/W4", "-Wpedantic"],
        (Warnings::Pedantic, CompilerKind::Clang) => &["-Wall", "-Wextra", "-Wpedantic"],
    });
    if build.warnings_as_errors {
        out.push(&[if msvc_style { "/WX" } else { "-Werror" }]);
    }

    out.push(&[match (build.runtime, input.kind) {
        (Runtime::Dynamic, CompilerKind::Msvc | CompilerKind::ClangCl) => "/MD",
        (Runtime::Static, CompilerKind::Msvc | CompilerKind::ClangCl) => "/MT",
        (Runtime::Dynamic, CompilerKind::Clang) => "-fms-runtime-lib=dll",
        (Runtime::Static, CompilerKind::Clang) => "-fms-runtime-lib=static",
    }]);

    if cxx {
        // With exceptions or RTTI off, cl and clang-cl only warn about
        // `try` / `throw` and `typeid` / `dynamic_cast`; these make it an
        // error, as clang does (build.md, "Settings and flags").
        out.push(match (build.exceptions, input.kind) {
            (true, CompilerKind::Clang) => &[][..],
            (true, CompilerKind::Msvc | CompilerKind::ClangCl) => &["/EHsc"],
            (false, CompilerKind::Msvc) => &["/we4530"],
            (false, CompilerKind::ClangCl) => &[],
            (false, CompilerKind::Clang) => &["-fno-exceptions"],
        });
        if !build.rtti {
            out.push(match input.kind {
                CompilerKind::Msvc => &["/GR-", "/we4541"][..],
                CompilerKind::ClangCl => &["/GR-", "-Werror=rtti"],
                CompilerKind::Clang => &["-fno-rtti"],
            });
        }
    }

    out.push(&[match (profile.opt_level, msvc_style) {
        (OptLevel::O0, true) => "/Od",
        (OptLevel::O1 | OptLevel::O2 | OptLevel::O3, true) => "/O2",
        (OptLevel::Size, true) => "/O1",
        (OptLevel::O0, false) => "-O0",
        (OptLevel::O1, false) => "-O1",
        (OptLevel::O2, false) => "-O2",
        (OptLevel::O3, false) => "-O3",
        (OptLevel::Size, false) => "-Os",
    }]);
    if profile.debug_info {
        out.push(&[if msvc_style { "/Z7" } else { "-g" }]);
    }
    if profile.lto {
        out.push(&[match input.kind.family() {
            Family::Msvc => "/GL",
            Family::Llvm => "-flto",
        }]);
    }

    let define = if msvc_style { "/D" } else { "-D" };
    let mut defines: Vec<&str> = build.defines.iter().map(String::as_str).collect();
    if !profile.debug_assertions {
        defines.push("NDEBUG");
    }
    if input.test {
        defines.push("CUNO_TEST");
    }
    if cxx && !build.exceptions {
        defines.push("_HAS_EXCEPTIONS=0");
    }
    for d in defines {
        out.0.push(format!("{define}{d}").into());
    }

    let include = if msvc_style { "/I" } else { "-I" };
    for dir in input.include_dirs.iter().chain(&build.include_dirs) {
        let mut arg = OsString::from(include);
        arg.push(dir);
        out.0.push(arg);
    }

    out.0.extend(build.flags.iter().map(OsString::from));
    out.0
}

/// The full arguments for compiling, or checking, one `source`: the mode,
/// [`compile_flags`], the language switch and the source, then `object`
/// when compiling.
pub fn compile_args(
    input: &CompileInput<'_>,
    source: &Path,
    object: Option<&Path>,
) -> Vec<OsString> {
    let msvc_style = match input.kind.syntax() {
        Syntax::Cl => true,
        Syntax::Gnu => false,
    };
    let mut args: Vec<OsString> = vec![
        match (input.mode, msvc_style) {
            (Mode::Compile, true) => "/c",
            (Mode::Check, true) => "/Zs",
            (Mode::Compile, false) => "-c",
            (Mode::Check, false) => "-fsyntax-only",
        }
        .into(),
    ];
    args.extend(compile_flags(input));
    // The language switch: stated, so the source's language is never left to
    // the driver's own guess from the extension.
    match (input.lang, msvc_style) {
        (Lang::C, true) => args.push("/TC".into()),
        (Lang::Cxx, true) => args.push("/TP".into()),
        (Lang::C, false) => args.extend(["-x".into(), "c".into()]),
        (Lang::Cxx, false) => args.extend(["-x".into(), "c++".into()]),
    }
    args.push(source.into());
    if let (Mode::Compile, Some(object)) = (input.mode, object) {
        if msvc_style {
            let mut arg = OsString::from("/Fo");
            arg.push(object);
            args.push(arg);
        } else {
            args.extend(["-o".into(), object.into()]);
        }
    }
    args
}

/// The arguments that make `kind` color its diagnostics when cuno colors
/// (`color`), placed first so a user's `-fno-color-diagnostics` in `flags`
/// comes later and wins: through a pipe Clang colors only when asked, and
/// on Windows writes color as escape codes only with the second flag. cl
/// has no such switch, and nothing is given when cuno does not color
/// (`docs/en/specs/build.md`, "Always-on flags").
pub fn color_args(kind: CompilerKind, color: bool) -> Vec<OsString> {
    match (color, kind.family()) {
        (true, Family::Llvm) => ["-fcolor-diagnostics", "-fansi-escape-codes"]
            .map(OsString::from)
            .to_vec(),
        (false, _) | (true, Family::Msvc) => Vec::new(),
    }
}

/// The arguments that make `kind` write the headers a compile read into
/// `file` (`build.md`, "Settings and flags"). They come after
/// [`compile_args`], so they follow the user's `flags` and win over a
/// user's `-MF`.
pub fn dependency_args(kind: CompilerKind, file: &Path) -> Vec<OsString> {
    match (kind.dependencies(), kind.syntax()) {
        (Dependencies::SourceDependencies, _) => vec!["/sourceDependencies".into(), file.into()],
        (Dependencies::DepFile, Syntax::Cl) => {
            let mut to = OsString::from("/clang:-MF");
            to.push(file);
            vec!["/clang:-MD".into(), to]
        }
        (Dependencies::DepFile, Syntax::Gnu) => vec!["-MD".into(), "-MF".into(), file.into()],
    }
}

/// The language standard switch, naming the standard on every driver:
/// `/std:c++latest` is already C++26 on current cl and clang-cl, and
/// clang-cl ignores `/std:c++23` and `/std:c23`, so it gets clang's own
/// through `/clang:`. C23 is `c2x`, which Clang 17 also accepts.
pub(super) fn standard(kind: CompilerKind, edition: Edition) -> &'static str {
    match (kind, edition) {
        (CompilerKind::Clang, Edition::Cxx17) => "-std=c++17",
        (CompilerKind::Clang, Edition::Cxx20) => "-std=c++20",
        (CompilerKind::Clang, Edition::Cxx23) => "-std=c++23",
        (CompilerKind::Clang, Edition::C11) => "-std=c11",
        (CompilerKind::Clang, Edition::C17) => "-std=c17",
        (CompilerKind::Clang, Edition::C23) => "-std=c2x",
        (CompilerKind::ClangCl, Edition::Cxx23) => "/clang:-std=c++23",
        (CompilerKind::ClangCl, Edition::C23) => "/clang:-std=c2x",
        (CompilerKind::Msvc, Edition::Cxx23) => "/std:c++23",
        (CompilerKind::Msvc, Edition::C23) => "/std:clatest",
        (CompilerKind::Msvc | CompilerKind::ClangCl, Edition::Cxx17) => "/std:c++17",
        (CompilerKind::Msvc | CompilerKind::ClangCl, Edition::Cxx20) => "/std:c++20",
        (CompilerKind::Msvc | CompilerKind::ClangCl, Edition::C11) => "/std:c11",
        (CompilerKind::Msvc | CompilerKind::ClangCl, Edition::C17) => "/std:c17",
    }
}

/// Whether `kind` at `version` accepts `edition`. cl only warns about a
/// `/std` it does not know and compiles with the old standard, so this is
/// decided from its version (`docs/en/specs/toolchain.md`, "Language
/// standards"): `/std:c++23` from cl 19.52, the oldest checked, and
/// `/std:clatest` from 19.37 (Visual Studio 2022 17.7). Every Clang cuno
/// supports accepts every edition.
pub fn check_edition(
    kind: CompilerKind,
    version: Version,
    edition: Edition,
) -> Result<(), ToolchainError> {
    let needed = match kind {
        // cl ignores a `/std` it does not know, so its version decides.
        CompilerKind::Msvc => match edition {
            Edition::Cxx23 => Version {
                major: 19,
                minor: 52,
                patch: 0,
            },
            Edition::C23 => Version {
                major: 19,
                minor: 37,
                patch: 0,
            },
            _ => return Ok(()),
        },
        // Every supported Clang accepts every edition (as `c2x` for C23).
        CompilerKind::Clang | CompilerKind::ClangCl => return Ok(()),
    };
    if version >= needed {
        return Ok(());
    }
    Err(ToolchainError::EditionNotSupported {
        compiler: kind.name(),
        version,
        edition: edition.as_str(),
        needed,
    })
}

#[derive(Default)]
struct Flags(Vec<OsString>);

impl Flags {
    fn push(&mut self, flags: &[&str]) {
        self.0.extend(flags.iter().map(OsString::from));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Clang is asked to color, and to write color as escape codes, only
    /// when cuno colors; cl is given nothing either way.
    ///
    /// Mutation: give MSVC the flags, give clang or clang-cl none when
    /// coloring, or some when not, or drop or reorder one; this test fails.
    #[test]
    fn color_args_follow_the_family() {
        let flags = ["-fcolor-diagnostics", "-fansi-escape-codes"];
        for kind in [CompilerKind::Clang, CompilerKind::ClangCl] {
            assert_eq!(strings(color_args(kind, true)), flags, "{kind:?}");
            assert!(color_args(kind, false).is_empty(), "{kind:?}");
        }
        assert!(color_args(CompilerKind::Msvc, true).is_empty());
        assert!(color_args(CompilerKind::Msvc, false).is_empty());
    }

    const DRIVERS: [CompilerKind; 3] = [
        CompilerKind::Msvc,
        CompilerKind::ClangCl,
        CompilerKind::Clang,
    ];

    fn build() -> BuildSettings {
        BuildSettings {
            warnings: Warnings::Default,
            warnings_as_errors: false,
            defines: vec![],
            include_dirs: vec![],
            links: vec![],
            lib_dirs: vec![],
            link_flags: vec![],
            flags: vec![],
            runtime: Runtime::Dynamic,
            exceptions: true,
            rtti: true,
        }
    }

    fn profile() -> ProfileSettings {
        ProfileSettings {
            opt_level: OptLevel::O0,
            debug_info: false,
            debug_assertions: true,
            lto: false,
        }
    }

    /// The settings of one case, which a row changes from the defaults.
    struct Case {
        lang: Lang,
        edition: Edition,
        build: BuildSettings,
        profile: ProfileSettings,
        include_dirs: Vec<PathBuf>,
        test: bool,
    }

    impl Case {
        fn new() -> Case {
            Case {
                lang: Lang::Cxx,
                edition: Edition::Cxx20,
                build: build(),
                profile: profile(),
                include_dirs: vec![],
                test: false,
            }
        }

        fn flags(&self, kind: CompilerKind) -> Vec<String> {
            let input = CompileInput {
                kind,
                lang: self.lang,
                edition: self.edition,
                build: &self.build,
                profile: &self.profile,
                include_dirs: &self.include_dirs,
                test: self.test,
                mode: Mode::Compile,
            };
            strings(compile_flags(&input))
        }
    }

    fn strings(args: Vec<OsString>) -> Vec<String> {
        args.into_iter().map(|a| a.into_string().unwrap()).collect()
    }

    /// Mutation: drop, reorder or change any always-on flag of any driver;
    /// this test fails.
    #[test]
    fn the_defaults_give_the_always_on_flags() {
        let case = Case::new();
        let expected: [&[&str]; 3] = [
            &[
                "/nologo",
                "/utf-8",
                "/Zc:__cplusplus",
                "/std:c++20",
                "/MD",
                "/EHsc",
                "/Od",
            ],
            &[
                "/nologo",
                "/utf-8",
                "--target=x86_64-pc-windows-msvc",
                "/Zc:__cplusplus",
                "/std:c++20",
                "/MD",
                "/EHsc",
                "/Od",
            ],
            &[
                "--target=x86_64-pc-windows-msvc",
                "-std=c++20",
                "-fms-runtime-lib=dll",
                "-O0",
            ],
        ];
        for (kind, expected) in DRIVERS.into_iter().zip(expected) {
            assert_eq!(case.flags(kind), expected, "{kind:?}");
        }
    }

    /// `flags` with `removed` gone and `added` present, compared with the
    /// defaults: what one row of the table changes.
    fn assert_row(name: &str, change: impl Fn(&mut Case), rows: [(&[&str], &[&str]); 3]) {
        assert_row_for(Case::new, name, change, rows);
    }

    /// [`assert_row`] for a C package (C17).
    fn c_case() -> Case {
        let mut case = Case::new();
        case.lang = Lang::C;
        case.edition = Edition::C17;
        case
    }

    fn assert_row_for(
        start: fn() -> Case,
        name: &str,
        change: impl Fn(&mut Case),
        rows: [(&[&str], &[&str]); 3],
    ) {
        let base = start();
        let mut case = start();
        change(&mut case);
        for (kind, (removed, added)) in DRIVERS.into_iter().zip(rows) {
            let before = base.flags(kind);
            let after = case.flags(kind);
            let mut expected: Vec<String> = before
                .iter()
                .filter(|f| !removed.contains(&f.as_str()))
                .cloned()
                .collect();
            for flag in added {
                assert!(
                    after.iter().any(|f| f == flag),
                    "{name} {kind:?}: `{flag}` missing from {after:?}"
                );
                if !expected.iter().any(|f| f == flag) {
                    expected.push((*flag).to_owned());
                }
            }
            let mut sorted_after = after.clone();
            sorted_after.sort();
            expected.sort();
            assert_eq!(sorted_after, expected, "{name} {kind:?}");
        }
    }

    /// Mutation: change, drop or misplace any setting's flags for any driver
    /// (`docs/en/specs/build.md`, "Settings and flags"); this test fails.
    #[test]
    fn every_setting_gives_the_tables_flags_on_every_driver() {
        type Row = (
            &'static str,
            fn(&mut Case),
            [(&'static [&'static str], &'static [&'static str]); 3],
        );
        let rows: Vec<Row> = vec![
            (
                "c++17",
                |c| c.edition = Edition::Cxx17,
                [
                    (&["/std:c++20"], &["/std:c++17"]),
                    (&["/std:c++20"], &["/std:c++17"]),
                    (&["-std=c++20"], &["-std=c++17"]),
                ],
            ),
            (
                "c++23",
                |c| c.edition = Edition::Cxx23,
                [
                    (&["/std:c++20"], &["/std:c++23"]),
                    (&["/std:c++20"], &["/clang:-std=c++23"]),
                    (&["-std=c++20"], &["-std=c++23"]),
                ],
            ),
            (
                "warnings all",
                |c| c.build.warnings = Warnings::All,
                [
                    (&[], &["/W4"]),
                    (&[], &["/W4"]),
                    (&[], &["-Wall", "-Wextra"]),
                ],
            ),
            (
                "warnings pedantic",
                |c| c.build.warnings = Warnings::Pedantic,
                [
                    (&[], &["/W4", "/permissive-"]),
                    (&[], &["/W4", "-Wpedantic"]),
                    (&[], &["-Wall", "-Wextra", "-Wpedantic"]),
                ],
            ),
            (
                "warnings-as-errors",
                |c| c.build.warnings_as_errors = true,
                [(&[], &["/WX"]), (&[], &["/WX"]), (&[], &["-Werror"])],
            ),
            (
                "defines",
                |c| c.build.defines = vec!["A=1".into()],
                [(&[], &["/DA=1"]), (&[], &["/DA=1"]), (&[], &["-DA=1"])],
            ),
            (
                "test",
                |c| c.test = true,
                [
                    (&[], &["/DCUNO_TEST"]),
                    (&[], &["/DCUNO_TEST"]),
                    (&[], &["-DCUNO_TEST"]),
                ],
            ),
            (
                "include dirs",
                |c| {
                    c.include_dirs = vec!["src".into()];
                    c.build.include_dirs = vec!["inc".into()];
                },
                [
                    (&[], &["/Isrc", "/Iinc"]),
                    (&[], &["/Isrc", "/Iinc"]),
                    (&[], &["-Isrc", "-Iinc"]),
                ],
            ),
            (
                "runtime static",
                |c| c.build.runtime = Runtime::Static,
                [
                    (&["/MD"], &["/MT"]),
                    (&["/MD"], &["/MT"]),
                    (&["-fms-runtime-lib=dll"], &["-fms-runtime-lib=static"]),
                ],
            ),
            (
                "exceptions false",
                |c| c.build.exceptions = false,
                [
                    (&["/EHsc"], &["/we4530", "/D_HAS_EXCEPTIONS=0"]),
                    (&["/EHsc"], &["/D_HAS_EXCEPTIONS=0"]),
                    (&[], &["-fno-exceptions", "-D_HAS_EXCEPTIONS=0"]),
                ],
            ),
            (
                "rtti false",
                |c| c.build.rtti = false,
                [
                    (&[], &["/GR-", "/we4541"]),
                    (&[], &["/GR-", "-Werror=rtti"]),
                    (&[], &["-fno-rtti"]),
                ],
            ),
            (
                "opt-level 1",
                |c| c.profile.opt_level = OptLevel::O1,
                [
                    (&["/Od"], &["/O2"]),
                    (&["/Od"], &["/O2"]),
                    (&["-O0"], &["-O1"]),
                ],
            ),
            (
                "opt-level 2",
                |c| c.profile.opt_level = OptLevel::O2,
                [
                    (&["/Od"], &["/O2"]),
                    (&["/Od"], &["/O2"]),
                    (&["-O0"], &["-O2"]),
                ],
            ),
            (
                "opt-level 3",
                |c| c.profile.opt_level = OptLevel::O3,
                [
                    (&["/Od"], &["/O2"]),
                    (&["/Od"], &["/O2"]),
                    (&["-O0"], &["-O3"]),
                ],
            ),
            (
                "opt-level s",
                |c| c.profile.opt_level = OptLevel::Size,
                [
                    (&["/Od"], &["/O1"]),
                    (&["/Od"], &["/O1"]),
                    (&["-O0"], &["-Os"]),
                ],
            ),
            (
                "debug-info",
                |c| c.profile.debug_info = true,
                [(&[], &["/Z7"]), (&[], &["/Z7"]), (&[], &["-g"])],
            ),
            (
                "debug-assertions false",
                |c| c.profile.debug_assertions = false,
                [
                    (&[], &["/DNDEBUG"]),
                    (&[], &["/DNDEBUG"]),
                    (&[], &["-DNDEBUG"]),
                ],
            ),
            (
                "lto",
                |c| c.profile.lto = true,
                [(&[], &["/GL"]), (&[], &["-flto"]), (&[], &["-flto"])],
            ),
            (
                "raw flags",
                |c| c.build.flags = vec!["/raw".into()],
                [(&[], &["/raw"]), (&[], &["/raw"]), (&[], &["/raw"])],
            ),
        ];
        for (name, change, row) in rows {
            assert_row(name, change, row);
        }
    }

    /// Mutation: give a C source a C++-only flag, or map a C edition wrongly;
    /// this test fails.
    #[test]
    fn c_sources_get_the_c_standard_and_no_cxx_only_flag() {
        for (edition, msvc, clang) in [
            (Edition::C11, "/std:c11", "-std=c11"),
            (Edition::C17, "/std:c17", "-std=c17"),
            (Edition::C23, "/std:clatest", "-std=c2x"),
        ] {
            let mut case = Case::new();
            case.lang = Lang::C;
            case.edition = edition;
            case.build.exceptions = false;
            case.build.rtti = false;
            for kind in DRIVERS {
                let flags = case.flags(kind);
                let expected = match (kind, edition) {
                    (CompilerKind::Clang, _) => clang,
                    (CompilerKind::ClangCl, Edition::C23) => "/clang:-std=c2x",
                    _ => msvc,
                };
                assert!(flags.iter().any(|f| f == expected), "{kind:?} {flags:?}");
                for cxx_only in [
                    "/Zc:__cplusplus",
                    "/EHsc",
                    "/GR-",
                    "-fno-rtti",
                    "-fno-exceptions",
                    "/D_HAS_EXCEPTIONS=0",
                    "-D_HAS_EXCEPTIONS=0",
                    "/we4530",
                    "/we4541",
                    "-Werror=rtti",
                ] {
                    assert!(
                        !flags.iter().any(|f| f == cxx_only),
                        "{kind:?} {edition:?} {flags:?}"
                    );
                }
            }
        }
    }

    /// Mutation: put the raw flags anywhere but last, or put the user's
    /// defines after cuno's own or the directories cuno adds after the user's;
    /// this test fails.
    #[test]
    fn the_order_is_the_documented_one() {
        let mut case = Case::new();
        case.build.flags = vec!["/raw".into()];
        case.build.defines = vec!["USER".into()];
        case.build.include_dirs = vec!["user".into()];
        case.include_dirs = vec!["src".into()];
        case.test = true;
        case.profile.debug_assertions = false;
        for kind in DRIVERS {
            let flags = case.flags(kind);
            assert_eq!(flags.last().map(String::as_str), Some("/raw"), "{kind:?}");
            let at = |suffix: &str| flags.iter().position(|f| f.ends_with(suffix)).unwrap();
            assert!(
                at("USER") < at("NDEBUG") && at("NDEBUG") < at("CUNO_TEST"),
                "{flags:?}"
            );
            assert!(at("src") < at("user"), "{flags:?}");
            assert!(at("CUNO_TEST") < at("src"), "{flags:?}");
        }
    }

    /// Mutation: use the wrong mode switch or language switch, drop the
    /// object, or name an object when only checking; this test fails.
    #[test]
    fn a_source_is_compiled_or_checked_in_the_packages_language() {
        let case = Case::new();
        let source = Path::new("a.h");
        let object = Path::new("a.obj");
        for (kind, mode, lang, first, language, output) in [
            (
                CompilerKind::Msvc,
                Mode::Compile,
                Lang::Cxx,
                "/c",
                vec!["/TP"],
                vec!["/Foa.obj"],
            ),
            (
                CompilerKind::ClangCl,
                Mode::Compile,
                Lang::C,
                "/c",
                vec!["/TC"],
                vec!["/Foa.obj"],
            ),
            (
                CompilerKind::Clang,
                Mode::Compile,
                Lang::Cxx,
                "-c",
                vec!["-x", "c++"],
                vec!["-o", "a.obj"],
            ),
            (
                CompilerKind::Clang,
                Mode::Check,
                Lang::C,
                "-fsyntax-only",
                vec!["-x", "c"],
                vec![],
            ),
            (
                CompilerKind::Msvc,
                Mode::Check,
                Lang::C,
                "/Zs",
                vec!["/TC"],
                vec![],
            ),
        ] {
            let input = CompileInput {
                kind,
                lang,
                edition: if lang == Lang::C {
                    Edition::C17
                } else {
                    Edition::Cxx20
                },
                build: &case.build,
                profile: &case.profile,
                include_dirs: &[],
                test: false,
                mode,
            };
            let args = strings(compile_args(&input, source, Some(object)));
            let flags = strings(compile_flags(&input));
            let mut expected = vec![first.to_owned()];
            expected.extend(flags);
            expected.extend(language.into_iter().map(str::to_owned));
            expected.push("a.h".to_owned());
            expected.extend(output.into_iter().map(str::to_owned));
            assert_eq!(args, expected, "{kind:?} {mode:?}");
        }
    }

    /// Mutation: drop any setting the two languages share for a C package
    /// (defines, include directories, warnings, runtime, optimisation, debug
    /// information, `NDEBUG`, LTO, `CUNO_TEST`, raw flags); this test fails.
    #[test]
    fn a_c_package_gets_every_shared_setting() {
        type Row = (
            &'static str,
            fn(&mut Case),
            [(&'static [&'static str], &'static [&'static str]); 3],
        );
        let same = |a: &'static [&'static str], c: &'static [&'static str]| {
            [(&[][..], a), (&[][..], a), (&[][..], c)]
        };
        let rows: Vec<Row> = vec![
            (
                "warnings all",
                |c| c.build.warnings = Warnings::All,
                same(&["/W4"], &["-Wall", "-Wextra"]),
            ),
            (
                "warnings-as-errors",
                |c| c.build.warnings_as_errors = true,
                same(&["/WX"], &["-Werror"]),
            ),
            (
                "defines",
                |c| c.build.defines = vec!["A".into()],
                same(&["/DA"], &["-DA"]),
            ),
            (
                "test",
                |c| c.test = true,
                same(&["/DCUNO_TEST"], &["-DCUNO_TEST"]),
            ),
            (
                "include dirs",
                |c| c.build.include_dirs = vec!["inc".into()],
                same(&["/Iinc"], &["-Iinc"]),
            ),
            (
                "debug-info",
                |c| c.profile.debug_info = true,
                same(&["/Z7"], &["-g"]),
            ),
            (
                "debug-assertions false",
                |c| c.profile.debug_assertions = false,
                same(&["/DNDEBUG"], &["-DNDEBUG"]),
            ),
            (
                "raw flags",
                |c| c.build.flags = vec!["/raw".into()],
                same(&["/raw"], &["/raw"]),
            ),
            (
                "runtime static",
                |c| c.build.runtime = Runtime::Static,
                [
                    (&["/MD"], &["/MT"]),
                    (&["/MD"], &["/MT"]),
                    (&["-fms-runtime-lib=dll"], &["-fms-runtime-lib=static"]),
                ],
            ),
            (
                "opt-level s",
                |c| c.profile.opt_level = OptLevel::Size,
                [
                    (&["/Od"], &["/O1"]),
                    (&["/Od"], &["/O1"]),
                    (&["-O0"], &["-Os"]),
                ],
            ),
            (
                "lto",
                |c| c.profile.lto = true,
                [(&[], &["/GL"]), (&[], &["-flto"]), (&[], &["-flto"])],
            ),
            (
                "exceptions false",
                |c| c.build.exceptions = false,
                [(&[], &[]), (&[], &[]), (&[], &[])],
            ),
            (
                "rtti false",
                |c| c.build.rtti = false,
                [(&[], &[]), (&[], &[]), (&[], &[])],
            ),
        ];
        for (name, change, row) in rows {
            assert_row_for(c_case, name, change, row);
        }
    }

    /// Mutation: name an object when none is given; this test fails.
    #[test]
    fn compiling_without_an_object_names_none() {
        let case = Case::new();
        for kind in DRIVERS {
            let input = CompileInput {
                kind,
                lang: Lang::Cxx,
                edition: Edition::Cxx20,
                build: &case.build,
                profile: &case.profile,
                include_dirs: &[],
                test: false,
                mode: Mode::Compile,
            };
            let args = strings(compile_args(&input, Path::new("a.cpp"), None));
            assert_eq!(args.last().map(String::as_str), Some("a.cpp"), "{kind:?}");
        }
    }

    /// Mutation: let a cl too old for `c++23` or `c23` through, reject one that
    /// is new enough, or hold Clang to a version; this test fails.
    #[test]
    fn an_edition_cl_does_not_know_is_an_error() {
        let v = |minor| Version {
            major: 19,
            minor,
            patch: 0,
        };
        let old = Version {
            major: 17,
            minor: 0,
            patch: 0,
        };
        assert!(check_edition(CompilerKind::Msvc, v(52), Edition::Cxx23).is_ok());
        assert!(check_edition(CompilerKind::Msvc, v(37), Edition::C23).is_ok());
        let err = check_edition(CompilerKind::Msvc, v(51), Edition::Cxx23).unwrap_err();
        assert_eq!(
            err.to_string(),
            "msvc 19.51.0 does not support edition = \"c++23\""
        );
        assert_eq!(
            cuno_core::CunoError::help(&err).as_deref(),
            Some("use msvc 19.52.0 or newer, or choose another edition or compiler")
        );
        assert!(check_edition(CompilerKind::Msvc, v(36), Edition::C23).is_err());
        assert!(check_edition(CompilerKind::Msvc, v(29), Edition::Cxx20).is_ok());
        for kind in [CompilerKind::Clang, CompilerKind::ClangCl] {
            for edition in [Edition::Cxx23, Edition::C23] {
                assert!(
                    check_edition(kind, old, edition).is_ok(),
                    "{kind:?} {edition:?}"
                );
            }
        }
    }

    /// Each driver is asked for its header dependencies in its own way, the
    /// file named where it writes one (`build.md`, "Settings and flags").
    ///
    /// Mutation: ask clang-cl for `/showIncludes` (which leaves out `#embed`
    /// files) or its runtime `-MD`, cl for a `-MD` file, or leave out `-MF`;
    /// this test fails.
    #[test]
    fn each_driver_reports_its_headers_its_own_way() {
        let file = Path::new("C:/p/a.cpp.d");
        let args = |kind| strings(dependency_args(kind, file));
        assert_eq!(
            args(CompilerKind::Msvc),
            ["/sourceDependencies", "C:/p/a.cpp.d"]
        );
        assert_eq!(
            args(CompilerKind::ClangCl),
            ["/clang:-MD", "/clang:-MFC:/p/a.cpp.d"]
        );
        assert_eq!(args(CompilerKind::Clang), ["-MD", "-MF", "C:/p/a.cpp.d"]);
    }
}
