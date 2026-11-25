//! Every citation of a spec section in the code names a heading that
//! exists, so a renamed heading fails here rather than leaving comments that
//! send a reader nowhere.
//!
//! A citation is a Markdown file name, optionally in backticks, a comma, and
//! a section title in double quotes, written in a comment; a comment
//! wrapped over several lines is read as one. The file is looked for in
//! `docs/en/specs/`, then `docs/en/`, and the title must be one of its
//! headings, word for word.

use std::path::{Path, PathBuf};

/// The repository root.
fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// Every `.rs` file under `dir`, `target/` aside.
fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n != "target") {
                rust_files(&path, out);
            }
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The text of a comment line, without its `//`, `///` or `//!` and one
/// space; `None` for a line that is not a comment.
fn comment(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix("//")?;
    let rest = rest
        .strip_prefix('/')
        .or_else(|| rest.strip_prefix('!'))
        .unwrap_or(rest);
    Some(rest.strip_prefix(' ').unwrap_or(rest))
}

/// Each run of consecutive comment lines in `text`, joined with spaces,
/// with the 1-based line each joined piece starts on.
fn comments(text: &str) -> Vec<(String, Vec<(usize, usize)>)> {
    let mut runs = Vec::new();
    let mut joined = String::new();
    let mut starts = Vec::new();
    for (index, line) in text.lines().enumerate() {
        match comment(line) {
            Some(piece) => {
                if !joined.is_empty() {
                    joined.push(' ');
                }
                starts.push((joined.len(), index + 1));
                joined.push_str(piece);
            }
            None if !joined.is_empty() => {
                runs.push((std::mem::take(&mut joined), std::mem::take(&mut starts)));
            }
            None => {}
        }
    }
    if !joined.is_empty() {
        runs.push((joined, starts));
    }
    runs
}

/// A citation found in a comment: the file named, the title, and where.
/// `title` is `None` for one the test cannot read.
#[derive(Debug)]
struct Citation {
    file: String,
    title: Option<String>,
    line: usize,
}

/// The citations in one joined comment. One the test cannot read is
/// returned with no title, to be reported rather than skipped: a file name
/// followed by a comma and an opening quote with no closing one, or by a
/// parenthesis holding quoted titles.
fn citations(text: &str, starts: &[(usize, usize)]) -> Vec<Citation> {
    let line_of = |offset: usize| {
        starts
            .iter()
            .rev()
            .find(|(start, _)| *start <= offset)
            .map_or(0, |(_, line)| *line)
    };
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = text[from..].find(".md") {
        let at = from + at;
        from = at + 3;
        let after = &text[at + 3..];
        let after = after.strip_prefix('`').unwrap_or(after);
        let title = match after.strip_prefix(',') {
            Some(rest) => match rest.trim_start().strip_prefix('"') {
                Some(quoted) => quoted.find('"').map(|end| quoted[..end].to_owned()),
                None => continue,
            },
            None => {
                let Some(inside) = after.trim_start().strip_prefix('(') else {
                    continue;
                };
                if !inside.split(')').next().unwrap_or("").contains('"') {
                    continue;
                }
                None
            }
        };
        let name_start = text[..at]
            .rfind(|c: char| !(c.is_ascii_alphanumeric() || "_./-".contains(c)))
            .map_or(0, |i| i + 1);
        found.push(Citation {
            file: format!("{}.md", &text[name_start..at]),
            title,
            line: line_of(at),
        });
    }
    found
}

/// The headings of `file` as written after their `#`s, or `None` when it is
/// in neither `docs/en/specs/` nor `docs/en/`.
fn headings(file: &str) -> Option<Vec<String>> {
    let name = Path::new(file).file_name()?;
    let docs = root().join("docs").join("en");
    let text = [docs.join("specs").join(name), docs.join(name)]
        .iter()
        .find_map(|path| std::fs::read_to_string(path).ok())?;
    Some(
        text.lines()
            .filter_map(|line| {
                let rest = line.trim_start_matches('#');
                (rest.len() < line.len() && rest.starts_with(' ')).then(|| rest.trim().to_owned())
            })
            .collect(),
    )
}

/// Every citation in `src/`, `crates/` and `tests/` names a heading of the
/// file it names.
///
/// Mutation: rename a cited heading in `docs/en/specs/`, cite one that does
/// not exist, leave a citation's closing quote out, or put its titles in
/// parentheses after the file; this test fails, naming the file and line of
/// each citation.
#[test]
fn every_spec_citation_names_a_heading() {
    let mut files = Vec::new();
    for dir in ["src", "crates", "tests"] {
        rust_files(&root().join(dir), &mut files);
    }
    let mut checked = 0;
    let mut broken = Vec::new();
    for path in &files {
        let text = std::fs::read_to_string(path).unwrap();
        let shown = path.strip_prefix(root()).unwrap().display().to_string();
        for (joined, starts) in comments(&text) {
            for citation in citations(&joined, &starts) {
                checked += 1;
                let at = format!("{shown}:{}", citation.line);
                match (&citation.title, headings(&citation.file)) {
                    (None, _) => broken.push(format!(
                        "{at}: a citation of {} this test cannot read; write it as (`{0}`, \"Section\")",
                        citation.file
                    )),
                    (_, None) => broken.push(format!(
                        "{at}: {} is in neither docs/en/specs/ nor docs/en/",
                        citation.file
                    )),
                    (Some(title), Some(headings)) if !headings.contains(title) => broken.push(
                        format!("{at}: {} has no heading \"{title}\"", citation.file),
                    ),
                    _ => {}
                }
            }
        }
    }
    assert!(checked > 100, "only {checked} citations found");
    assert!(broken.is_empty(), "{}", broken.join("\n"));
}
