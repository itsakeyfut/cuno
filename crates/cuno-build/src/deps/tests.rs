use super::*;

/// cl's `/sourceDependencies` file, as cl 19.52 writes it: every header,
/// absolute and in lower case, the source itself left out.
///
/// Mutation: read only the first header, or the source; this test fails.
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
}

/// clang-cl's notes, indented by include depth and relative where the
/// include was, are read and removed; the rest of its output stays as it
/// was.
///
/// Mutation: keep the note lines, keep their indentation, or drop the
/// other lines; this test fails.
#[test]
fn notes_are_read_and_removed() {
    let mut stdout = "Note: including file: src\\inc dir/h1.h\r\n\
                      Note: including file:  src\\inc dir\\日本.h\r\n\
                      src\\a.cpp(3,1): warning: unused\r\n\
                      Note: including file: C:\\Program Files\\LLVM\\lib\\clang\\22\\include\\yvals_core.h\n"
        .as_bytes()
        .to_vec();
    let paths = take_notes(&mut stdout).unwrap();
    assert_eq!(
        paths,
        [
            PathBuf::from(r"src\inc dir/h1.h"),
            PathBuf::from(r"src\inc dir\日本.h"),
            PathBuf::from(r"C:\Program Files\LLVM\lib\clang\22\include\yvals_core.h"),
        ]
    );
    assert_eq!(stdout, b"src\\a.cpp(3,1): warning: unused\r\n");
}

/// A note whose path is not UTF-8 leaves the headers unknown, so the step
/// is not recorded, and is still removed.
///
/// Mutation: read it lossily; this test fails.
#[test]
fn a_note_that_is_not_utf8_leaves_the_headers_unknown() {
    let mut stdout = b"Note: including file: \x93\xfa.h\r\nok\r\n".to_vec();
    assert_eq!(take_notes(&mut stdout), None);
    assert_eq!(stdout, b"ok\r\n");
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
