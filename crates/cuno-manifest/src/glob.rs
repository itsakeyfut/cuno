//! The patterns `[fmt] exclude` and `[lint] exclude` hold
//! (`docs/en/specs/dev-tools.md`, "Target files").

use std::fmt;

use cuno_core::NameCase;

/// A path pattern from the project root: `*` is any characters within one
/// directory, `**` (a whole segment) any number of directories, `?` one
/// character, `/` the separator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Glob {
    written: String,
    segments: Vec<Segment>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Segment {
    /// `**`: zero or more directories.
    Any,
    /// One directory or file name, with `*` and `?` in it.
    Name(String),
}

impl Glob {
    /// Reads `written`, or says why it is not a pattern.
    pub fn parse(written: &str) -> Result<Glob, String> {
        if written.is_empty() {
            return Err("it is empty".to_owned());
        }
        if written.contains('\\') {
            return Err("it has a `\\`; separate directories with `/`".to_owned());
        }
        if written.starts_with('/') {
            return Err("it starts with `/`; write it from the project root".to_owned());
        }
        let mut segments = Vec::new();
        for part in written.split('/') {
            if part.is_empty() {
                return Err("it has an empty directory name (`//`, or `/` at the end)".to_owned());
            }
            if part == "**" {
                segments.push(Segment::Any);
            } else if part.contains("**") {
                return Err(format!(
                    "`{part}` has `**` beside other characters; `**` is a whole directory name"
                ));
            } else {
                segments.push(Segment::Name(part.to_owned()));
            }
        }
        Ok(Glob {
            written: written.to_owned(),
            segments,
        })
    }

    /// Whether `relative`, a path from the project root with `/` between
    /// its names, matches; without regard to case where `case` says so.
    pub fn matches(&self, relative: &str, case: NameCase) -> bool {
        let names: Vec<String> = relative.split('/').map(|name| fold(name, case)).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let segments: Vec<Segment> = self
            .segments
            .iter()
            .map(|segment| match segment {
                Segment::Any => Segment::Any,
                Segment::Name(name) => Segment::Name(fold(name, case)),
            })
            .collect();
        match_segments(&segments, &names)
    }
}

impl fmt::Display for Glob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.written)
    }
}

/// `name` as `case` compares it.
fn fold(name: &str, case: NameCase) -> String {
    case.fold(name.as_ref()).to_string_lossy().into_owned()
}

/// Whether `segments` match `names` exactly; a table over both, so a
/// pattern of many `**` takes time in proportion to its length times the
/// path's, never more.
fn match_segments(segments: &[Segment], names: &[&str]) -> bool {
    // `can[j]`: the segments so far match the first `j` names.
    let mut can = vec![false; names.len() + 1];
    can[0] = true;
    for segment in segments {
        let mut next = vec![false; names.len() + 1];
        for j in 0..=names.len() {
            next[j] = match segment {
                // Zero names, or one more than a match one shorter.
                Segment::Any => can[j] || (j > 0 && next[j - 1]),
                Segment::Name(pattern) => j > 0 && can[j - 1] && match_name(pattern, names[j - 1]),
            };
        }
        can = next;
    }
    can[names.len()]
}

/// `*` and `?` within one name, by the same kind of table.
fn match_name(pattern: &str, name: &str) -> bool {
    let name: Vec<char> = name.chars().collect();
    let mut can = vec![false; name.len() + 1];
    can[0] = true;
    for p in pattern.chars() {
        let mut next = vec![false; name.len() + 1];
        for j in 0..=name.len() {
            next[j] = match p {
                '*' => can[j] || (j > 0 && next[j - 1]),
                '?' => j > 0 && can[j - 1],
                c => j > 0 && can[j - 1] && name[j - 1] == c,
            };
        }
        can = next;
    }
    can[name.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glob(written: &str) -> Glob {
        Glob::parse(written).unwrap()
    }

    /// `*` stays within one name, `**` crosses any number of directories,
    /// `?` is one character, and the whole path has to match.
    ///
    /// Mutation: let `*` cross `/`, make `**` need at least one directory,
    /// let `?` match nothing, or match a prefix of the path; this test
    /// fails.
    #[test]
    fn patterns_match_as_the_spec_says() {
        let s = NameCase::Sensitive;
        assert!(glob("third_party/**").matches("third_party/a/b.cpp", s));
        assert!(glob("third_party/**").matches("third_party", s));
        assert!(glob("src/*.cpp").matches("src/main.cpp", s));
        assert!(!glob("src/*.cpp").matches("src/a/main.cpp", s));
        assert!(!glob("src/*.cpp").matches("src/main.c", s));
        assert!(glob("src/m*n.cpp").matches("src/main.cpp", s));
        assert!(!glob("src/m*n.cpp").matches("src/xain.cpp", s));
        assert!(glob("**/gen/*.c").matches("gen/x.c", s));
        assert!(glob("**/gen/*.c").matches("a/b/gen/x.c", s));
        assert!(glob("src/?.c").matches("src/a.c", s));
        assert!(!glob("src/?.c").matches("src/.c", s));
        assert!(!glob("src").matches("src/main.cpp", s));
    }

    /// A pattern of many `**` and `*` against a deep path is answered at
    /// once, not by trying every split.
    ///
    /// Mutation: match by trying every split (backtracking); this test does
    /// not finish.
    #[test]
    fn many_wildcards_do_not_take_exponential_time() {
        let pattern = vec!["**"; 40].join("/") + "/x";
        let path = vec!["a"; 60].join("/");
        assert!(!glob(&pattern).matches(&path, NameCase::Sensitive));
        let star = format!("{}x", "*a".repeat(30));
        assert!(!glob(&star).matches(&"a".repeat(80), NameCase::Sensitive));
    }

    /// Case is ignored only where the host ignores it.
    ///
    /// Mutation: ignore `case`, either way; this test fails.
    #[test]
    fn case_follows_the_host() {
        assert!(glob("Third_Party/**").matches("third_party/x.c", NameCase::Insensitive));
        assert!(!glob("Third_Party/**").matches("third_party/x.c", NameCase::Sensitive));
    }

    /// What cannot be read as a pattern is refused, saying why.
    ///
    /// Mutation: accept any of these; this test fails.
    #[test]
    fn malformed_patterns_are_refused() {
        for bad in ["", "***", "a**/b", "src\\x", "/src", "a//b", "src/"] {
            assert!(Glob::parse(bad).is_err(), "{bad:?}");
        }
        assert_eq!(glob("src/**").to_string(), "src/**");
    }
}
