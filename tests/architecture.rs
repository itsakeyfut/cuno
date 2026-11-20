//! The allowed dependencies between cuno's crates, checked against every
//! `crates/*/Cargo.toml`.
//!
//! Cargo rejects only cycles, so a dependency in a direction the design does
//! not allow would otherwise build without complaint. The table below mirrors
//! `docs/en/specs/architecture.md`, "Dependencies between crates"; change the
//! spec first, then this table.

use std::path::Path;

/// Each crate under `crates/` and the `cuno-*` crates it may depend on.
const ALLOWED: &[(&str, &[&str])] = &[
    ("cuno-core", &[]),
    ("cuno-toolchain", &["cuno-core"]),
    ("cuno-manifest", &["cuno-core", "cuno-toolchain"]),
    (
        "cuno-build",
        &["cuno-core", "cuno-manifest", "cuno-toolchain"],
    ),
    ("cuno-test", &["cuno-core"]),
    (
        "cuno-tools",
        &["cuno-core", "cuno-manifest", "cuno-toolchain"],
    ),
    (
        "cuno-deps",
        &["cuno-core", "cuno-manifest", "cuno-toolchain"],
    ),
    ("cuno-script", &["cuno-core", "cuno-build"]),
];

/// The package name and the `cuno-*` entries of `[dependencies]` in a
/// `Cargo.toml`. Entries are read in both the `cuno-x = ...` and the
/// `cuno-x.workspace = true` forms; `[dev-dependencies]` and other tables are
/// ignored, since test-only edges do not shape the build.
fn crate_and_dependencies(manifest: &str) -> (String, Vec<String>) {
    let mut name = None;
    let mut deps = Vec::new();
    let mut table = String::new();
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            table = line.trim_matches(|c| c == '[' || c == ']').to_owned();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if table == "package" && key == "name" {
            name = Some(value.trim().trim_matches('"').to_owned());
        } else if table == "dependencies" {
            let dep = key.split('.').next().unwrap_or(key).trim();
            if dep.starts_with("cuno-") {
                deps.push(dep.to_owned());
            }
        }
    }
    (name.expect("a [package] name"), deps)
}

/// Every crate under `crates/`, as (package name, `cuno-*` dependencies).
fn workspace_crates() -> Vec<(String, Vec<String>)> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates");
    let mut found: Vec<_> = std::fs::read_dir(&crates)
        .expect("crates/ should exist")
        .map(|entry| entry.unwrap().path().join("Cargo.toml"))
        .filter(|manifest| manifest.is_file())
        .map(|manifest| crate_and_dependencies(&std::fs::read_to_string(manifest).unwrap()))
        .collect();
    found.sort();
    found
}

fn allowed(name: &str) -> Option<&'static [&'static str]> {
    ALLOWED
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, deps)| *deps)
}

/// Mutation: add `cuno-manifest = { path = "../cuno-manifest" }` under
/// `[dependencies]` in `crates/cuno-toolchain/Cargo.toml`, or any `cuno-*`
/// dependency to `crates/cuno-core/Cargo.toml`; this test fails.
#[test]
fn crate_dependencies_follow_architecture() {
    let mut violations = Vec::new();
    for (name, deps) in workspace_crates() {
        let Some(permitted) = allowed(&name) else {
            continue; // reported by every_crate_is_in_the_architecture_table
        };
        for dep in deps {
            if !permitted.contains(&dep.as_str()) {
                violations.push(format!(
                    "{name} depends on {dep}, which architecture.md does not allow"
                ));
            }
        }
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

/// Mutation: add a crate under `crates/` without an entry in `ALLOWED`, or
/// remove `cuno-core` from `ALLOWED`; this test fails.
#[test]
fn every_crate_is_in_the_architecture_table() {
    let missing: Vec<_> = workspace_crates()
        .into_iter()
        .map(|(name, _)| name)
        .filter(|name| allowed(name).is_none())
        .collect();
    assert!(
        missing.is_empty(),
        "crates not in the dependency table of architecture.md: {missing:?}"
    );
}

/// Mutation: make `crate_and_dependencies` skip the `cuno-x.workspace = true`
/// form, or read `[dev-dependencies]`; this test fails.
#[test]
fn dependencies_are_read_in_every_form_and_only_from_dependencies() {
    let manifest = r#"
[package]
name = "cuno-example"

[dependencies]
cuno-core.workspace = true
cuno-toolchain = { path = "../cuno-toolchain" }
clap.workspace = true

[dev-dependencies]
cuno-build = { path = "../cuno-build" }
"#;
    assert_eq!(
        crate_and_dependencies(manifest),
        (
            "cuno-example".to_owned(),
            vec!["cuno-core".to_owned(), "cuno-toolchain".to_owned()]
        )
    );
}
