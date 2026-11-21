<!-- translated-from: docs/ja/specs/distribution.md @ blob c7a10232790453e64fb6fb6cd6bc56c100666716 -->

# Distribution and Operation of cuno Itself

## License

- **Dual-licensed under MIT and Apache-2.0** (users can choose either).
- Reason: bundled headers such as `cuno/test.h` are compiled together with users' code, so they need a permissive license that poses no problem even when game companies and others use them in product development. This is also the standard in the Rust ecosystem, and the patent clause of Apache-2.0 gives companies peace of mind.

## Distribution format

- **A single binary**. Bundled headers such as `cuno/test.h` are also embedded in the binary and extracted into the cuno home directory on first run.

## Installation

- Binaries are distributed via GitHub Releases.
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

## Self-update

- `cuno self update` (same approach as rustup).

## Platforms supported by cuno itself

Supported in the order Windows x64 → Linux x64 → macOS (arm64).

## Versioning policy

- cuno itself uses semantic versioning.
- From 1.0 onward, backward compatibility of `cuno.toml` is guaranteed (an old `cuno.toml` works as is with a newer cuno).
- Whether to release 1.0 will be considered once production adoption is reported. Completing milestones alone does not make a 1.0 ([roadmap.md](../roadmap.md)).

## Accepting contributions

- Accepted via GitHub Issues / PRs.
- CONTRIBUTING.md includes the procedure for adding a compiler ([toolchain.md](toolchain.md)).

## Open questions

- Code of Conduct, and whether a DCO / CLA is required
