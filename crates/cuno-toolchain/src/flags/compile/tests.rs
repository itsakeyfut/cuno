use super::*;

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
    let base = Case::new();
    let mut case = Case::new();
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
                (&["/std:c++20"], &["/std:c++latest"]),
                (&["/std:c++20"], &["/std:c++latest"]),
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
                (&["/EHsc"], &["/D_HAS_EXCEPTIONS=0"]),
                (&["/EHsc"], &["/D_HAS_EXCEPTIONS=0"]),
                (&[], &["-fno-exceptions", "-D_HAS_EXCEPTIONS=0"]),
            ],
        ),
        (
            "rtti false",
            |c| c.build.rtti = false,
            [(&[], &["/GR-"]), (&[], &["/GR-"]), (&[], &["-fno-rtti"])],
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
        (Edition::C23, "/std:clatest", "-std=c23"),
    ] {
        let mut case = Case::new();
        case.lang = Lang::C;
        case.edition = edition;
        case.build.exceptions = false;
        case.build.rtti = false;
        for kind in DRIVERS {
            let flags = case.flags(kind);
            let expected = if kind == CompilerKind::Clang {
                clang
            } else {
                msvc
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
