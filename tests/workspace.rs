//! Properties of the workspace itself, as Cargo reports them.

use std::process::Command;

/// The entries of the JSON array `"<key>":[...]` in `cargo metadata` output.
/// The entries are package IDs, which contain no `[`, `]` or `,`.
fn array<'a>(json: &'a str, key: &str) -> Vec<&'a str> {
    let start = json
        .find(&format!("\"{key}\":["))
        .unwrap_or_else(|| panic!("{key} missing from cargo metadata"))
        + key.len()
        + 4;
    let end = start + json[start..].find(']').unwrap();
    let mut entries: Vec<&str> = json[start..end]
        .split(',')
        .filter(|e| !e.is_empty())
        .collect();
    entries.sort_unstable();
    entries
}

/// A plain `cargo test` (without `--workspace`) must test every crate, as CI
/// does. The root is both the workspace and the `cuno` package, so without
/// `default-members` in `Cargo.toml` it would test only `cuno`.
///
/// Mutation: delete the `default-members` line from `Cargo.toml`; the default
/// members are then only `cuno` and this test fails.
#[test]
fn plain_cargo_commands_cover_every_workspace_member() {
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo metadata should run");
    assert!(output.status.success(), "{output:?}");
    let json = String::from_utf8(output.stdout).expect("cargo metadata prints UTF-8");

    let members = array(&json, "workspace_members");
    assert!(members.len() > 1, "{members:?}");
    assert_eq!(array(&json, "workspace_default_members"), members);
}
