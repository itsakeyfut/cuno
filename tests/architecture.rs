//! The allowed dependencies between cuno's crates, checked against what Cargo
//! actually resolves for every crate under `crates/`.
//!
//! Cargo rejects only cycles, so a dependency in a direction the design does
//! not allow would otherwise build without complaint. The allowed directions
//! are read from the table in `docs/en/specs/architecture.md`, "Dependencies
//! between crates", so the spec is the only copy of them: changing the design
//! means changing that table, and this test follows.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

const SPEC: &str = "docs/en/specs/architecture.md";
const SECTION: &str = "## Dependencies between crates";

/// The crates in the spec's table and the `cuno-*` crates each may depend on.
/// `None` means "all" (the `cuno` binary). A row marked "(later)" is a crate
/// that does not exist yet and is read like any other.
fn allowed_from_spec(spec: &str) -> BTreeMap<String, Option<Vec<String>>> {
    let section = spec
        .split_once(SECTION)
        .unwrap_or_else(|| panic!("{SPEC} has no {SECTION:?} section"))
        .1;
    let section = section.split("\n## ").next().unwrap();
    let mut table = BTreeMap::new();
    for row in section.lines().filter(|line| line.starts_with("| `")) {
        let cells: Vec<&str> = row.trim_matches('|').split('|').map(str::trim).collect();
        let [krate, deps] = cells[..] else {
            panic!("unexpected row in {SPEC}: {row}");
        };
        let name = backticked(krate).into_iter().next().unwrap();
        let deps = (deps != "all").then(|| backticked(deps));
        table.insert(name, deps);
    }
    assert!(!table.is_empty(), "no rows read from {SECTION:?} in {SPEC}");
    table
}

/// The names written in backticks in a table cell.
fn backticked(cell: &str) -> Vec<String> {
    cell.split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

/// Every crate under `crates/` and the `cuno-*` packages it depends on, as
/// Cargo resolves them: every form of declaring a dependency (inline, table,
/// `.workspace = true`, renamed with `package =`, target-specific) and both
/// normal and build dependencies. Dev-dependencies are left out, since
/// test-only edges do not shape what is built.
fn workspace_crates() -> BTreeMap<String, Vec<String>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(root)
        .output()
        .expect("cargo metadata should run");
    assert!(output.status.success(), "{output:?}");
    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("cargo metadata prints JSON");

    let crates_dir = root.join("crates");
    let mut found = BTreeMap::new();
    for package in metadata["packages"].as_array().unwrap() {
        let manifest = Path::new(package["manifest_path"].as_str().unwrap());
        if !manifest.starts_with(&crates_dir) {
            continue; // the root `cuno` package, which may depend on all
        }
        let mut deps: Vec<String> = package["dependencies"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|dep| dep["kind"].as_str() != Some("dev"))
            .map(|dep| dep["name"].as_str().unwrap().to_owned())
            .filter(|name| name.starts_with("cuno-"))
            .collect();
        deps.sort();
        deps.dedup();
        found.insert(package["name"].as_str().unwrap().to_owned(), deps);
    }
    found
}

fn spec() -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(SPEC)).unwrap()
}

/// Mutation: add `cuno-manifest = { path = "../cuno-manifest" }` to
/// `crates/cuno-toolchain/Cargo.toml` under `[dependencies]`, under
/// `[target.'cfg(windows)'.dependencies]`, under `[build-dependencies]`, as
/// `[dependencies.cuno-manifest]`, or renamed (`m = { path = ..., package =
/// "cuno-manifest" }`); this test fails each time.
#[test]
fn crate_dependencies_follow_architecture() {
    let allowed = allowed_from_spec(&spec());
    let mut violations = Vec::new();
    for (name, deps) in workspace_crates() {
        let Some(Some(permitted)) = allowed.get(&name) else {
            continue; // reported by every_crate_is_in_the_architecture_table
        };
        for dep in deps.iter().filter(|dep| !permitted.contains(dep)) {
            violations.push(format!(
                "{name} depends on {dep}, which {SPEC}, {SECTION:?}, does not allow"
            ));
        }
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

/// Mutation: add a crate under `crates/` without a row in the spec's table, or
/// remove the `cuno-core` row from it; this test fails.
#[test]
fn every_crate_is_in_the_architecture_table() {
    let allowed = allowed_from_spec(&spec());
    let missing: Vec<_> = workspace_crates()
        .into_keys()
        .filter(|name| !allowed.contains_key(name))
        .collect();
    assert!(
        missing.is_empty(),
        "crates with no row in {SPEC}, {SECTION:?}: {missing:?}"
    );
}

/// Mutation: make `allowed_from_spec` read "all" as an empty list, or drop
/// the "(later)" rows; this test fails.
#[test]
fn the_spec_table_is_read_as_written() {
    let spec = "\
## Dependencies between crates

| Crate | May depend on |
|---|---|
| `cuno-core` | none |
| `cuno-manifest` | `cuno-core`, `cuno-toolchain` |
| `cuno-deps` (later) | `cuno-core` |
| `cuno` (the binary) | all |

## Next
| `ignored` | `ignored` |
";
    let table = allowed_from_spec(spec);
    let expect = |deps: &[&str]| Some(deps.iter().map(|d| d.to_string()).collect());
    assert_eq!(table.len(), 4, "{table:?}");
    assert_eq!(table["cuno-core"], expect(&[]));
    assert_eq!(
        table["cuno-manifest"],
        expect(&["cuno-core", "cuno-toolchain"])
    );
    assert_eq!(table["cuno-deps"], expect(&["cuno-core"]));
    assert_eq!(table["cuno"], None);
}
