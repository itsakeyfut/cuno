//! Which headers a compile read, from what its compiler wrote
//! (`docs/en/specs/build.md`, "Incremental and parallel builds").

use std::path::{Path, PathBuf};

/// Where a compile's headers are found once it has run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Headers {
    /// cl's `/sourceDependencies` JSON file.
    Json(PathBuf),
    /// clang-cl's `Note: including file:` lines on standard output.
    Notes,
    /// clang's `-MD` dependency file.
    DepFile(PathBuf),
}

/// The prefix of the line clang-cl prints for each header it reads, in
/// English whatever the system's language.
const NOTE: &[u8] = b"Note: including file:";

/// Removes clang-cl's `Note: including file:` lines from `stdout`, which
/// are not shown (`build.md`, "Settings and flags"), and returns the paths
/// they name, or `None` when one is not UTF-8.
pub fn take_notes(stdout: &mut Vec<u8>) -> Option<Vec<PathBuf>> {
    let mut kept = Vec::with_capacity(stdout.len());
    let mut paths = Some(Vec::new());
    for line in stdout.split_inclusive(|&b| b == b'\n') {
        match line.strip_prefix(NOTE) {
            Some(rest) => {
                let path = std::str::from_utf8(rest)
                    .ok()
                    .map(|p| p.trim_start_matches(' ').trim_end_matches(['\r', '\n']));
                match (path, &mut paths) {
                    (Some(path), Some(paths)) => paths.push(PathBuf::from(path)),
                    _ => paths = None,
                }
            }
            None => kept.extend_from_slice(line),
        }
    }
    *stdout = kept;
    paths
}

/// The headers in cl's `/sourceDependencies` file: `Data.Includes`.
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

/// The prerequisites in a `-MD` dependency file, which is make's syntax:
/// `<target>: <prerequisite> ...`, a space in a path written `\ `, `#`
/// written `\#`, `$` written `$$`, and a line continued with `\` at its end.
/// Any other backslash is a Windows path separator and is kept.
pub fn from_depfile(text: &str) -> Option<Vec<PathBuf>> {
    let mut tokens: Vec<String> = Vec::new();
    let mut token = String::new();
    let mut chars = text.chars().peekable();
    let end = |token: &mut String, tokens: &mut Vec<String>| {
        if !token.is_empty() {
            tokens.push(std::mem::take(token));
        }
    };
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
