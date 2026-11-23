//! Which headers a compile read, from what its compiler wrote
//! (`docs/en/specs/build.md`, "Incremental and parallel builds").

use std::path::{Path, PathBuf};

/// The file a compile's headers are written to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Headers {
    /// cl's `/sourceDependencies` JSON file.
    Json(PathBuf),
    /// A `-MD` dependency file, from clang or clang-cl.
    DepFile(PathBuf),
}

impl Headers {
    /// The file the compiler writes.
    pub fn file(&self) -> &Path {
        match self {
            Headers::Json(file) | Headers::DepFile(file) => file,
        }
    }

    /// The headers named in `bytes`, the content of [`Headers::file`], or
    /// `None` when it cannot be read as that kind of file.
    pub fn parse(&self, bytes: &[u8]) -> Option<Vec<PathBuf>> {
        match self {
            Headers::Json(_) => from_json(bytes),
            Headers::DepFile(_) => from_depfile(std::str::from_utf8(bytes).ok()?),
        }
    }
}

/// The headers in cl's `/sourceDependencies` file: `Data.Includes`, every
/// entry a string.
pub fn from_json(bytes: &[u8]) -> Option<Vec<PathBuf>> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    value
        .get("Data")?
        .get("Includes")?
        .as_array()?
        .iter()
        .map(|v| v.as_str().map(PathBuf::from))
        .collect()
}

/// The prerequisites of the first rule in a `-MD` dependency file, which
/// is make's syntax: `<target>: <prerequisite> ...`, a space in a path
/// written `\ `, `#` written `\#`, `$` written `$$`, and a line continued
/// with `\` at its end. Any other backslash is a Windows path separator and
/// is kept. Later rules (a user's `-MP` adds one per header, with no
/// prerequisites) are not read (`build.md`, "Settings and flags").
pub fn from_depfile(text: &str) -> Option<Vec<PathBuf>> {
    let mut tokens: Vec<String> = Vec::new();
    let mut token = String::new();
    let mut chars = text.chars().peekable();
    let end = |token: &mut String, tokens: &mut Vec<String>| {
        if !token.is_empty() {
            tokens.push(std::mem::take(token));
        }
    };
    let has_target = |tokens: &[String]| tokens.iter().any(|t| t.ends_with(':'));
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.peek() {
                Some(' ') | Some('#') => token.push(chars.next().unwrap_or(' ')),
                Some('\n') => {
                    chars.next();
                    end(&mut token, &mut tokens);
                }
                Some('\r') => {
                    chars.next();
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    end(&mut token, &mut tokens);
                }
                _ => token.push('\\'),
            },
            '$' if chars.peek() == Some(&'$') => {
                chars.next();
                token.push('$');
            }
            // A line that is not continued ends the rule.
            '\n' => {
                end(&mut token, &mut tokens);
                if has_target(&tokens) {
                    break;
                }
            }
            c if c.is_whitespace() => end(&mut token, &mut tokens),
            c => token.push(c),
        }
    }
    end(&mut token, &mut tokens);
    // The target is everything up to the token that ends with its colon.
    let colon = tokens.iter().position(|t| t.ends_with(':'))?;
    Some(
        tokens
            .split_off(colon + 1)
            .into_iter()
            .map(PathBuf::from)
            .collect(),
    )
}

/// `path` as the compiler, running in `cwd`, meant it.
pub fn resolve(cwd: &Path, path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    }
}

#[cfg(test)]
mod tests {
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
}
