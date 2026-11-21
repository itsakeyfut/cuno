//! Reading a TOML file with positions: deserializing it while collecting the
//! keys nothing read, and finding where a key is written, so that every
//! warning and error can point at its line (`docs/en/specs/manifest.md`,
//! "Unknown keys").

use std::path::Path;

use serde::de::DeserializeOwned;
use toml::de::{DeTable, DeValue};

use crate::{ConfigError, ConfigWarning, Location};

/// A configuration file being read.
pub(crate) struct Doc<'a> {
    pub text: &'a str,
    /// The file as shown in messages.
    pub file: &'a Path,
    table: toml::Spanned<DeTable<'a>>,
}

impl<'a> Doc<'a> {
    /// Parses `text`, or the syntax error with its position.
    pub fn parse(text: &'a str, file: &'a Path) -> Result<Doc<'a>, ConfigError> {
        let table = DeTable::parse(text).map_err(|e| toml_error(text, file, &e))?;
        Ok(Doc { text, file, table })
    }

    /// The file as `T`, and the dotted path of every key `T` has no place
    /// for, in file order. A value of the wrong type is an error at its
    /// position.
    pub fn deserialize<T: DeserializeOwned>(&self) -> Result<(T, Vec<String>), ConfigError> {
        let de = toml::de::Deserializer::parse(self.text)
            .map_err(|e| toml_error(self.text, self.file, &e))?;
        let mut unknown = Vec::new();
        let value = serde_ignored::deserialize(de, |path| {
            let mut parts = Vec::new();
            segments(&path, &mut parts);
            unknown.push(dotted(&parts));
        })
        .map_err(|e| toml_error(self.text, self.file, &e))?;
        // In the order they are written, whatever order serde visited them.
        unknown.sort_by_key(|path| self.key_offset(path).unwrap_or(0));
        Ok((value, unknown))
    }

    /// Where the key at the dotted `path` is written (`bin.0.name` for the
    /// first `[[bin]]`'s `name`; a key that is not bare is quoted, as
    /// [`key`] writes it), or the start of the file when it is not.
    pub fn locate(&self, path: &str) -> Location {
        let offset = self.key_offset(path).unwrap_or(0);
        Location::at(self.file, self.text, offset)
    }

    fn key_offset(&self, path: &str) -> Option<usize> {
        let mut table = self.table.get_ref();
        let mut offset = None;
        let parts = split(path);
        let mut parts = parts.iter().map(String::as_str).peekable();
        while let Some(part) = parts.next() {
            let (key, mut value) = table.get_key_value(part)?;
            offset = Some(key.span().start);
            // Through arrays of tables, by index.
            while let DeValue::Array(items) = value.get_ref() {
                let Some(index) = parts.peek().and_then(|p| p.parse::<usize>().ok()) else {
                    break;
                };
                parts.next();
                value = items.get(index)?;
                offset = Some(value.span().start);
            }
            match value.get_ref() {
                DeValue::Table(inner) => table = inner,
                _ if parts.peek().is_none() => {}
                _ => return None,
            }
        }
        offset
    }

    /// A warning about the key at `path`.
    pub fn warning(&self, path: &str, message: String) -> ConfigWarning {
        ConfigWarning {
            message,
            location: Some(self.locate(path)),
        }
    }

    /// An error about the key at `path`.
    pub fn error(&self, path: &str, message: String, help: Option<String>) -> ConfigError {
        ConfigError {
            message,
            location: Some(self.locate(path)),
            help,
        }
    }

    /// The warning for a key cuno does not know.
    pub fn unknown(&self, path: &str) -> ConfigWarning {
        self.warning(
            path,
            format!("unknown key `{path}` in {}, ignored", self.file.display()),
        )
    }
}

/// `segment` as one part of a dotted path: bare when TOML would write it
/// bare, else quoted, so a key such as `"a.b"` stays one part.
pub(crate) fn key(segment: &str) -> String {
    let bare = !segment.is_empty()
        && segment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if bare {
        segment.to_owned()
    } else {
        format!("{segment:?}")
    }
}

fn dotted(parts: &[String]) -> String {
    parts.iter().map(|p| key(p)).collect::<Vec<_>>().join(".")
}

/// The parts of a dotted path [`dotted`] wrote.
fn split(path: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut chars = path.chars().peekable();
    while chars.peek().is_some() {
        let mut part = String::new();
        if chars.peek() == Some(&'"') {
            chars.next();
            while let Some(c) = chars.next() {
                match c {
                    '"' => break,
                    '\\' => part.extend(chars.next()),
                    c => part.push(c),
                }
            }
            chars.next(); // the `.` after it
        } else {
            for c in chars.by_ref() {
                if c == '.' {
                    break;
                }
                part.push(c);
            }
        }
        parts.push(part);
    }
    parts
}

/// The keys and indices of a path serde_ignored reports, without its marks
/// for an `Option` or newtype layer, which are not keys.
fn segments(path: &serde_ignored::Path<'_>, out: &mut Vec<String>) {
    use serde_ignored::Path;
    match path {
        Path::Root => {}
        Path::Seq { parent, index } => {
            segments(parent, out);
            out.push(index.to_string());
        }
        Path::Map { parent, key } => {
            segments(parent, out);
            out.push(key.clone());
        }
        Path::Some { parent }
        | Path::NewtypeStruct { parent }
        | Path::NewtypeVariant { parent } => segments(parent, out),
    }
}

fn toml_error(text: &str, file: &Path, error: &toml::de::Error) -> ConfigError {
    ConfigError {
        message: format!("{} in {}", error.message(), file.display()),
        location: Some(Location::at(
            file,
            text,
            error.span().map_or(0, |span| span.start),
        )),
        help: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    #[allow(dead_code)]
    struct Shape {
        build: Option<Build>,
        bin: Option<Vec<Bin>>,
    }
    #[derive(Deserialize)]
    #[allow(dead_code)]
    struct Build {
        rtti: Option<bool>,
    }
    #[derive(Deserialize)]
    #[allow(dead_code)]
    struct Bin {
        name: Option<String>,
    }

    const TEXT: &str =
        "[build]\nwanings = 1\n\n[[bin]]\nname = \"a\"\n\n[[bin]]\nname = \"b\"\nnmae = 2\n";

    /// Mutation: keep the `?` of an `Option` in the path, stop collecting the
    /// keys nothing read, or list them in another order than the file's; this
    /// test fails.
    #[test]
    fn keys_nothing_reads_are_listed_by_dotted_path() {
        let doc = Doc::parse(TEXT, Path::new("cuno.toml")).unwrap();
        let (_, unknown): (Shape, _) = doc.deserialize().unwrap();
        assert_eq!(unknown, ["build.wanings", "bin.1.nmae"]);
    }

    /// Mutation: point at the table instead of the key, ignore the index
    /// into an array of tables, or count from another place; this test
    /// fails.
    #[test]
    fn a_key_is_located_where_it_is_written() {
        let doc = Doc::parse(TEXT, Path::new("cuno.toml")).unwrap();
        let line = |path| doc.locate(path).line;
        assert_eq!(line("build.wanings"), 2);
        assert_eq!(line("bin.0.name"), 5);
        assert_eq!(line("bin.1.name"), 8);
        assert_eq!(line("bin.1.nmae"), 9);
        assert_eq!(line("bin"), 4);
        assert_eq!(line("nothing.here"), 1);
        assert_eq!(
            doc.unknown("build.wanings").to_string(),
            "unknown key `build.wanings` in cuno.toml, ignored\n --> cuno.toml:2:1"
        );
    }

    /// Mutation: split a quoted key at its dot, drop a key named `?`, or
    /// write a bare key quoted; this test fails.
    #[test]
    fn keys_that_are_not_bare_stay_one_part() {
        let text = "[build]
\"a.b\" = 1
\"?\" = 2
plain-key = 3
";
        let doc = Doc::parse(text, Path::new("cuno.toml")).unwrap();
        let (_, unknown): (Shape, _) = doc.deserialize().unwrap();
        assert_eq!(unknown, ["build.\"a.b\"", "build.\"?\"", "build.plain-key"]);
        let lines: Vec<usize> = unknown.iter().map(|p| doc.locate(p).line).collect();
        assert_eq!(lines, [2, 3, 4]);
        assert_eq!(
            split("platform.\"a.b\".build"),
            ["platform", "a.b", "build"]
        );
        assert_eq!(key("say \"hi\""), "\"say \\\"hi\\\"\"");
        assert_eq!(split(&key("say \"hi\"")), ["say \"hi\""]);
    }

    /// Mutation: lose the position of a syntax or type error; this test
    /// fails.
    #[test]
    fn syntax_and_type_errors_point_at_their_place() {
        let err = Doc::parse("[build]\nrtti = \n", Path::new("cuno.toml"))
            .err()
            .unwrap();
        assert_eq!(err.location.unwrap().line, 2, "{}", err.message);
        let doc = Doc::parse("\n[build]\nrtti = \"yes\"\n", Path::new("cuno.toml")).unwrap();
        let err = doc.deserialize::<Shape>().err().unwrap();
        assert_eq!(err.location.as_ref().unwrap().line, 3);
        assert!(err.message.contains("cuno.toml"), "{}", err.message);
    }
}
