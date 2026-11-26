//! The migration guide (`docs/en/guides/migration.md`) is held to cuno as it
//! is: every command it names exists or is planned, every command that
//! exists has a row, and every `cuno.toml` table it names is one the specs
//! define. A new command fails here until the guide says how it replaces
//! what somebody used before, which is how the guide stays current.

mod support;

use std::path::Path;

use support::Project;

/// The repository root.
fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &str) -> String {
    std::fs::read_to_string(root().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// The guide, as it is checked.
fn guide() -> String {
    read("docs/en/guides/migration.md")
}

/// Each command named as `` `cuno <command>`` `` in `text`, with the number
/// of the line it is on.
fn commands_named(text: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (n, line) in text.lines().enumerate() {
        for (i, _) in line.match_indices("`cuno ") {
            let name: String = line[i + "`cuno ".len()..]
                .chars()
                .take_while(|c| c.is_ascii_lowercase() || *c == '-')
                .collect();
            if !name.is_empty() {
                found.push((n + 1, name));
            }
        }
    }
    found
}

/// The commands `cuno --help` lists, `help` aside.
fn commands_today() -> Vec<String> {
    let run = Project::new().cuno(&["--help"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    run.stdout
        .lines()
        .skip_while(|line| !line.starts_with("Commands:"))
        .skip(1)
        .take_while(|line| line.starts_with("  "))
        .filter_map(|line| line.split_whitespace().next())
        .filter(|name| *name != "help")
        .map(str::to_owned)
        .collect()
}

/// Each `[table]` or `[[table]]` written in backticks in `text`.
fn tables_named(text: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (n, line) in text.lines().enumerate() {
        for span in line.split('`').skip(1).step_by(2) {
            if span.starts_with('[') && span.ends_with(']') && !span.contains(' ') {
                found.push((n + 1, span.to_owned()));
            }
        }
    }
    found
}

/// Whether `table` is an instance of `pattern`, where a `<name>` in the
/// pattern stands for one dotted part: `[build.msvc]` is an instance of
/// `[build.<compiler>]`.
fn instance_of(table: &str, pattern: &str) -> bool {
    let (t, p) = (table.split('.'), pattern.split('.'));
    if t.clone().count() != p.clone().count() {
        return false;
    }
    t.zip(p).all(|(t, p)| {
        t == p
            || p.find('<').is_some_and(|start| {
                let end = p.rfind('>').unwrap_or(p.len() - 1) + 1;
                let (before, after) = (&p[..start], &p[end..]);
                t.len() > before.len() + after.len() && t.starts_with(before) && t.ends_with(after)
            })
    })
}

/// Every command the guide names is one `cuno --help` lists or one the
/// roadmap plans, so the guide never sends a reader to a command that is
/// neither there nor coming.
///
/// Mutation: write `` `cuno frobnicate` `` in the guide; this test fails.
#[test]
fn every_command_in_the_guide_exists_or_is_planned() {
    let today = commands_today();
    let planned: Vec<String> = commands_named(&read("docs/en/roadmap.md"))
        .into_iter()
        .map(|(_, name)| name)
        .collect();
    let unknown: Vec<String> = commands_named(&guide())
        .into_iter()
        .filter(|(_, name)| !today.contains(name) && !planned.contains(name))
        .map(|(line, name)| format!("line {line}: cuno {name}"))
        .collect();
    assert!(
        unknown.is_empty(),
        "the guide names commands cuno neither has nor plans:\n{}",
        unknown.join("\n")
    );
}

/// Every command cuno has appears in the guide, so a new command fails
/// here until the guide has its row.
///
/// Mutation: remove every mention of `cuno doctor` from the guide; this
/// test fails.
#[test]
fn every_command_has_a_row_in_the_guide() {
    let named: Vec<String> = commands_named(&guide())
        .into_iter()
        .map(|(_, name)| name)
        .collect();
    let missing: Vec<String> = commands_today()
        .into_iter()
        .filter(|name| !named.contains(name))
        .collect();
    assert!(
        missing.is_empty(),
        "docs/en/guides/migration.md has no row for: {}",
        missing.join(", ")
    );
}

/// Every `cuno.toml` table the guide names is one the specs write,
/// directly or as a pattern (`[build.<compiler>]`).
///
/// Mutation: write `` `[buidl]` `` in the guide; this test fails.
#[test]
fn every_table_in_the_guide_is_in_the_specs() {
    let mut defined = Vec::new();
    for entry in std::fs::read_dir(root().join("docs/en/specs")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "md") {
            let text = std::fs::read_to_string(&path).unwrap();
            defined.extend(tables_named(&text).into_iter().map(|(_, t)| t));
        }
    }
    let unknown: Vec<String> = tables_named(&guide())
        .into_iter()
        .filter(|(_, table)| !defined.iter().any(|d| instance_of(table, d)))
        .map(|(line, table)| format!("line {line}: {table}"))
        .collect();
    assert!(
        unknown.is_empty(),
        "the guide names tables no spec defines:\n{}",
        unknown.join("\n")
    );
}

/// A table matches its pattern part by part, and only with the same number
/// of parts.
///
/// Mutation: stop comparing the number of parts; this test fails.
#[test]
fn a_table_matches_its_pattern_part_by_part() {
    assert!(instance_of("[build.msvc]", "[build.<compiler>]"));
    assert!(instance_of(
        "[profile.release.build]",
        "[profile.<name>.build]"
    ));
    assert!(instance_of(
        "[platform.windows.build]",
        "[platform.<OS>.build]"
    ));
    assert!(!instance_of("[platform.windows]", "[platform.<OS>.build]"));
    assert!(!instance_of("[buidl]", "[build]"));
}
