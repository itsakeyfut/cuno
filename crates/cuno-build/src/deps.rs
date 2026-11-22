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
mod tests;
