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

fn match_segments(segments: &[Segment], names: &[&str]) -> bool {
    match segments.split_first() {
        None => names.is_empty(),
        Some((Segment::Any, rest)) => {
            (0..=names.len()).any(|skip| match_segments(rest, &names[skip..]))
        }
        Some((Segment::Name(pattern), rest)) => match names.split_first() {
            Some((name, names)) => match_name(pattern, name) && match_segments(rest, names),
            None => false,
        },
    }
}

/// `*` and `?` within one name.
fn match_name(pattern: &str, name: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let name: Vec<char> = name.chars().collect();
    fn at(p: &[char], n: &[char]) -> bool {
        match p.split_first() {
            None => n.is_empty(),
            Some(('*', rest)) => (0..=n.len()).any(|skip| at(rest, &n[skip..])),
            Some(('?', rest)) => !n.is_empty() && at(rest, &n[1..]),
            Some((c, rest)) => n.first() == Some(c) && at(rest, &n[1..]),
        }
    }
    at(&pattern, &name)
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
        assert!(glob("**/gen/*.c").matches("gen/x.c", s));
        assert!(glob("**/gen/*.c").matches("a/b/gen/x.c", s));
        assert!(glob("src/?.c").matches("src/a.c", s));
        assert!(!glob("src/?.c").matches("src/.c", s));
        assert!(!glob("src").matches("src/main.cpp", s));
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
