use super::*;

/// cl's `/sourceDependencies` file, as cl 19.52 writes it: every header,
/// absolute and in lower case, the source itself left out. An entry that
/// is not a string makes the file unreadable rather than skipped, so no
/// header goes unrecorded.
///
/// Mutation: read only the first header, read the source, or skip an
/// entry that is not a string; this test fails.
#[test]
fn every_header_in_the_json_is_read() {
    let json = r#"{
    "Version": "1.2",
    "Data": {
        "Source": "c:\\p\\src\\a.cpp",
        "ProvidedModule": "",
        "Includes": [
            "c:\\p\\src\\inc dir\\h1.h",
            "c:\\p\\src\\inc dir\\日本.h",
            "c:\\program files\\microsoft visual studio\\18\\community\\vc\\tools\\msvc\\14.52.37025\\include\\cstdio"
        ],
        "ImportedModules": [],
        "ImportedHeaderUnits": []
    }
}"#;
    assert_eq!(
        from_json(json.as_bytes()).unwrap(),
        [
            PathBuf::from(r"c:\p\src\inc dir\h1.h"),
            PathBuf::from(r"c:\p\src\inc dir\日本.h"),
            PathBuf::from(
                r"c:\program files\microsoft visual studio\18\community\vc\tools\msvc\14.52.37025\include\cstdio"
            ),
        ]
    );
    assert_eq!(from_json(b"{\"Version\": \"1.2\"}"), None);
    assert_eq!(from_json(b"not json"), None);
    assert_eq!(from_json(br#"{"Data":{"Includes":["c:\\a.h",1]}}"#), None);
}

/// clang's dependency file: the target is skipped, an escaped space stays
/// in its path, a continued line goes on, a drive's colon is not the
/// target's, and other backslashes are separators.
///
/// Mutation: split at an escaped space, keep the target, or stop at a
/// continuation; this test fails.
#[test]
fn a_dependency_file_is_read_as_make_writes_it() {
    let text = "C:\\p\\target\\debug\\obj\\app\\src\\a.cpp.obj: src\\a.cpp src\\inc\\ dir\\h1.h \\\r\n  \
                C:\\Program\\ Files\\x\\cstdio \\\n  odd\\#name$$.h\r\n";
    assert_eq!(
        from_depfile(text).unwrap(),
        [
            PathBuf::from(r"src\a.cpp"),
            PathBuf::from(r"src\inc dir\h1.h"),
            PathBuf::from(r"C:\Program Files\x\cstdio"),
            PathBuf::from(r"odd#name$.h"),
        ]
    );
    assert_eq!(from_depfile("no target here"), None);
}

/// The rules a user's `-MP` adds after the first, one per header with no
/// prerequisites, are not read as headers: their targets would be taken
/// for files and the step never recorded.
///
/// Mutation: read every rule; this test fails.
#[test]
fn only_the_first_rule_is_read() {
    let text = "a.obj: src\\a.cpp src\\a.h\r\n\r\nsrc\\a.h:\r\n";
    assert_eq!(
        from_depfile(text).unwrap(),
        [PathBuf::from(r"src\a.cpp"), PathBuf::from(r"src\a.h")]
    );
}

/// Each kind of file is read by its own parser.
///
/// Mutation: read a JSON file as a dependency file, or the reverse; this
/// test fails.
#[test]
fn each_file_is_read_by_its_kind() {
    let json = Headers::Json("a.json".into());
    let dep = Headers::DepFile("a.d".into());
    assert_eq!(
        json.parse(br#"{"Data":{"Includes":["a.h"]}}"#),
        Some(vec![PathBuf::from("a.h")])
    );
    assert_eq!(dep.parse(b"a.obj: a.h\n"), Some(vec![PathBuf::from("a.h")]));
    assert_eq!(json.parse(b"a.obj: a.h\n"), None);
    assert_eq!(dep.parse(br#"{"Data":{"Includes":["a.h"]}}"#), None);
    assert_eq!(json.file(), Path::new("a.json"));
}

/// A relative path is the compiler's working directory's; an absolute
/// one is kept.
///
/// Mutation: keep a relative path as it is; this test fails.
#[test]
fn a_relative_header_is_the_working_directorys() {
    let cwd = Path::new(r"C:\p");
    assert_eq!(resolve(cwd, r"src\a.h".into()), Path::new(r"C:\p\src\a.h"));
    assert_eq!(resolve(cwd, r"D:\x.h".into()), Path::new(r"D:\x.h"));
}
