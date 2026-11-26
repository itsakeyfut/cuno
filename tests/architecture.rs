//! The allowed dependencies between cuno's crates, checked against what Cargo
//! actually resolves for every crate under `crates/` and for the binary; and
//! what `src/` and `cuno-cli` hold.
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

/// Every crate under `crates/`, the binary, and the `cuno-*` packages each
/// depends on, as
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

    let mut found = BTreeMap::new();
    for package in metadata["packages"].as_array().unwrap() {
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
/// "cuno-manifest" }`); or give the root `Cargo.toml` a `[dependencies]`
/// entry for `cuno-build`; this test fails each time.
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

/// The names in the first column of the table under `heading` in `spec`, up
/// to the next heading.
fn first_column(spec: &str, heading: &str) -> Vec<String> {
    let section = spec
        .split_once(heading)
        .unwrap_or_else(|| panic!("{SPEC} has no {heading:?} section"))
        .1;
    let section = section.split("\n#").next().unwrap();
    section
        .lines()
        .filter(|line| line.starts_with("| `"))
        .map(|row| backticked(row.trim_matches('|').split('|').next().unwrap()).remove(0))
        .collect()
}

/// The modules directly under `dir`: each `.rs` file but `lib.rs`, and each
/// directory, by name. A `main.rs` counts, so a second entry point is
/// reported like any module the spec does not list.
fn modules(dir: &Path) -> Vec<String> {
    let mut found: Vec<String> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir() || path.extension().is_some_and(|e| e == "rs"))
        .map(|path| path.file_stem().unwrap().to_string_lossy().into_owned())
        .filter(|name| name != "lib")
        .collect();
    found.sort();
    found
}

/// The binary only collects the process and calls `cuno-cli`, so `src/`
/// holds `main.rs` and nothing else (`architecture.md`, "What goes in
/// `src/` and `cuno-cli`").
///
/// Mutation: add any other `.rs` file or a directory under `src/`; this test
/// fails.
#[test]
fn src_holds_only_main() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found: Vec<String> = std::fs::read_dir(&src)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();
    assert_eq!(found, ["main.rs"], "src/ holds more than main.rs");
}

/// `cuno-cli` holds `lib.rs` and the command modules its table in the spec
/// lists, and no other: what several commands share belongs to another
/// crate (`architecture.md`, "What goes in `src/` and `cuno-cli`").
///
/// Mutation: add a module to `crates/cuno-cli/src/` that the table does not
/// list, or a row to the table with no module; this test fails.
#[test]
fn cli_modules_follow_architecture() {
    const HEADING: &str = "### What goes in `src/` and `cuno-cli`";
    let mut listed = first_column(&spec(), HEADING);
    listed.sort();
    assert!(
        !listed.is_empty(),
        "no modules read from {HEADING:?} in {SPEC}"
    );
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/cuno-cli/src");
    assert_eq!(
        modules(&dir),
        listed,
        "the modules of cuno-cli differ from {SPEC}, {HEADING:?}"
    );
}

/// Mutation: read another column than the first, or read past the next
/// heading; this test fails.
#[test]
fn the_module_table_is_read_as_written() {
    let spec = "### Modules

| Module | Commands |
|---|---|
| `a` | `b`, `c` |
| `d` | `e` |

### Next
| `ignored` | `ignored` |
";
    assert_eq!(first_column(spec, "### Modules"), ["a", "d"]);
}

/// Every `.rs` file under `dir`.
fn rust_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Only the binary asks the machine how many processors it has; the
/// library crates take the number as a value, so the default number of
/// jobs is decided in one place, the settings (`architecture.md`, "What
/// goes in `src/` and `cuno-cli`").
///
/// Mutation: read `available_parallelism` in a library crate again; this
/// test fails.
#[test]
fn only_the_binary_reads_the_number_of_processors() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    let readers: Vec<String> = files
        .iter()
        .filter(|path| {
            std::fs::read_to_string(path)
                .unwrap()
                .contains("available_parallelism")
        })
        .map(|path| path.display().to_string())
        .collect();
    assert!(readers.is_empty(), "{readers:?}");
    let main = std::fs::read_to_string(root.join("src/main.rs")).unwrap();
    assert!(main.contains("available_parallelism"));
}
