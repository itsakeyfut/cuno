<!-- translated-from: docs/ja/specs/distribution.md @ blob a6a92160ad8c9737f7b5c197b9bf92d41be375e9 -->

# Distribution and Operation of cuno Itself

## License

- **Dual-licensed under MIT and Apache-2.0** (users can choose either).
- Reason: bundled headers such as `cuno/test.h` are compiled together with users' code, so they need a permissive license that poses no problem even when game companies and others use them in product development. This is also the standard in the Rust ecosystem, and the patent clause of Apache-2.0 gives companies peace of mind.

## Distribution format

- **A single binary**. Bundled headers such as `cuno/test.h` are also embedded in the binary and extracted into the cuno home directory on first run.

## Installation

- Binaries are distributed via GitHub Releases.
  - Each release is tagged `v<version>`, and the section of `CHANGELOG.md` for that version becomes the release notes.
  - The release attaches `cuno-<version>-x86_64-pc-windows-msvc.zip` (with `cuno.exe`, `README.md`, `LICENSE-MIT` and `LICENSE-APACHE` at its top level) and `cuno-<version>-x86_64-pc-windows-msvc.zip.sha256`, which holds its SHA-256.
  - The binary is not code-signed. Windows SmartScreen may show a warning the first time it is run.
    - Reason: signing requires obtaining and managing a certificate, which would become a prerequisite for every release. It will be reconsidered when supporting winget and similar channels.
  - In the same release, all crates are published to crates.io (installable with `cargo install cuno`).
- Windows: winget / Scoop / a PowerShell one-liner.
- For Rust users: `cargo install cuno`.

## cuno home directory

- `~/.cuno` on all OSes (`%USERPROFILE%\.cuno` on Windows).
- Can be changed with the `CUNO_HOME` environment variable (same as cargo's `CARGO_HOME`). It must be an absolute path; a relative path, or on Windows a path that depends on the current drive (`/x`, `C:x`), is an error (exit code 3).
  - Rationale: with a relative path, the cuno home (and the user-level config) would change with the directory cuno is run from.
- Reason: a single location is easy to explain and easy to delete. Same shape as cargo / rustup. OS-specific locations (`%LOCALAPPDATA%` on Windows, XDG on Linux) follow platform conventions, but are not adopted because the location changes per OS and becomes scattered.

| Contents | Description |
|---|---|
| `config.toml` | Per-user configuration ([manifest.md](manifest.md)) |
| `bin/` | cuno itself (when installed with cuno's own installer, such as the PowerShell one-liner. When installed via winget / Scoop / `cargo install`, it is placed in each tool's location, and `cuno self update` also directs users to the respective update method) |
| `include/<cuno version>/` | Extraction destination for bundled headers. Separated per cuno version so that updating cuno does not break builds of existing projects |
| `registry/` | Recipe index |
| `sources/` | Dependency sources |
| `builds/` | Cache of built dependencies |
| `toolchains/` | Automatically installed compilers |

- A command that builds writes the bundled headers before it builds. A file whose content is already the same is not rewritten. If they cannot be written, it is an error (exit code 3).

## Self-update

- `cuno self update` (same approach as rustup).

## Platforms supported by cuno itself

Supported in the order Windows x64 → Linux x64 → macOS (arm64).

## Versioning policy

- cuno itself uses semantic versioning.
- Only versions of the form `x.y.z` are released. Pre-releases (such as `0.2.0-rc.1`) and versions with build metadata are not. `0.0.0` means "not released yet" and is never released.
  - Reason: tools that compare versions disagree on where a pre-release sorts (`0.2.0-rc.1` comes before `0.2.0`). Keeping to the one form that every tool orders alike means a release is never misjudged.
- From 1.0 onward, backward compatibility of `cuno.toml` is guaranteed (an old `cuno.toml` works as is with a newer cuno).
- Whether to release 1.0 will be considered once production adoption is reported. Completing milestones alone does not make a 1.0 ([roadmap.md](../roadmap.md)).

## Accepting contributions

- Accepted via GitHub Issues / PRs.
- CONTRIBUTING.md includes the procedure for adding a compiler ([toolchain.md](toolchain.md)).

## Open questions

- Code of Conduct, and whether a DCO / CLA is required
