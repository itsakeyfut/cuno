<!-- translated-from: docs/ja/specs/distribution.md @ blob 6465dc5ff48dce40f5697b2372eb43382206eec5 -->

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
- Windows: a PowerShell one-liner ([below](#the-powershell-one-liner)). winget / Scoop come later.
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

### The PowerShell one-liner

```
powershell -c "irm https://github.com/itsakeyfut/cuno/releases/latest/download/install.ps1 | iex"
```

- `install.ps1` sits at the root of the repository and is attached to each release beside the zip. The URL always points at the latest release's.
  - Reason: the script and the binary are always from the same release. Serving the script from `main` would let a change made before a release disagree with the releases already published.
- Steps:
  1. Decide the cuno home: `CUNO_HOME` if set (as in cuno, a path that is not absolute is an error and nothing is done), otherwise `%USERPROFILE%\.cuno`.
  2. Learn the latest version from where `https://github.com/itsakeyfut/cuno/releases/latest` redirects to (`…/releases/tag/v<version>`).
  3. Download the zip and its `.sha256` into a temporary directory, and check the SHA-256.
  4. Take `cuno.exe` from the zip and put it at `<cuno home>\bin\cuno.exe`, replacing one already there.
  5. If `<cuno home>\bin` is not on the user's `PATH` (`Path` under `HKCU\Environment` in the registry), append it and tell other processes of the change. The value's type (`REG_EXPAND_SZ`) and unexpanded forms such as `%USERPROFILE%` are kept as they are.
  6. Print the version installed and where, and say that `cuno` works in a newly opened terminal.
- On failure, it removes the temporary directory, leaves any `cuno.exe` already there and `PATH` unchanged, prints the error and stops. A checksum that does not match puts nothing in place either.
- A saved `install.ps1` takes arguments (`irm … | iex` cannot pass them).
  - `-Version <x.y.z>`: the version to install, skipping step 2.
  - `-Releases <URL>`: where releases are fetched from, `https://github.com/itsakeyfut/cuno/releases` by default. The zip is fetched from `<URL>/download/v<version>/`. For a company mirror, or for testing the installer. Anything but `https://` or `file://` is an error (the SHA-256 checked against comes from the same place, so `http://`, which can be rewritten on the way, is not accepted).
- It downloads with `curl.exe`, as cuno does ([Downloads](#downloads)), and unpacks the zip with `tar.exe`, both the ones Windows includes, run from `%SystemRoot%\System32`.
  - Reason: in PowerShell 5.1, `curl` is an alias of `Invoke-WebRequest`, and a `PATH` with Git for Windows' tools first finds a GNU tar that cannot read a zip.

## Downloads

- When cuno fetches from the network (`cuno self update`, fetching for dependency management), it runs `curl`. On Windows, it runs `%SystemRoot%\System32\curl.exe`, which Windows 10 1803 and later include, by its location, and otherwise the `curl.exe` found on `PATH`.
  - Reason: an older curl earlier on `PATH` (Git for Windows') would otherwise be run. `PATH` is searched too, so that where Windows' own has been removed, a curl the user installed works.
  - Reason: Windows' `curl.exe` verifies TLS against the Windows certificate store and follows the proxy variables such as `HTTPS_PROXY`, so it works behind a company proxy or TLS inspection without the user configuring anything. Building a Rust HTTP library in would leave cuno to handle these itself, and make the binary larger.
- If there is neither, it is an error (exit code 3), with a `help:` saying that Windows 10 1803 and later include it, and that otherwise `winget install cURL.cURL` installs it.
- Only `https://` is used, and a redirect is followed only to `https://`. A connection that takes 30 seconds, or a transfer slower than 1 byte a second for 30 seconds in a row, is stopped as a failure (the whole download has no time limit).
  - Reason: so that cuno never waits forever on a connection that does not answer, while a large download that is slow but moving is not stopped.
- A failed download (an HTTP error included) is an error (exit code 3) naming the URL and the error from `curl`.

## Self-update

- `cuno self update` updates cuno to the latest release ([commands.md](commands.md#cuno-self-update)).
- How cuno was installed is told by where the running `cuno.exe` is (compared by path identity).
  - `<cuno home>\bin\cuno.exe`: installed by cuno's own installer (the one-liner). cuno updates and removes it.
  - `<CARGO_HOME>\bin\cuno.exe` (`%USERPROFILE%\.cargo` when `CARGO_HOME` is not set): installed by `cargo install`. cuno points to cargo for updating and removing it.
  - When the two are the same place (`CARGO_HOME` is the cuno home), it counts as installed by cuno's own installer.
  - Anywhere else (a zip unpacked by hand, for example): cuno does not rewrite itself, and points to installing again with the one-liner.
  - Reason: if the installer recorded how cuno was installed, a record that disagrees with where cuno actually is (when a cuno elsewhere is run, for example) could have the wrong `cuno.exe` rewritten. Telling by location leaves no record to disagree.
- Uninstalling is `cuno self uninstall` ([commands.md](commands.md#cuno-self-uninstall)).

## Platforms supported by cuno itself

Supported in the order Windows x64 → Linux x64 → macOS (arm64).

## Versioning policy

- cuno itself uses semantic versioning.
- Only versions of the form `x.y.z` are released. Pre-releases (such as `0.2.0-rc.1`) and versions with build metadata are not. `0.0.0` means "not released yet" and is never released.
  - Reason: tools that compare versions disagree on where a pre-release sorts (`0.2.0-rc.1` comes before `0.2.0`). Keeping to the one form that every tool orders alike means a release is never misjudged.
- From 1.0 onward, backward compatibility of `Cuno.toml` is guaranteed (an old `Cuno.toml` works as is with a newer cuno).
- Whether to release 1.0 will be considered once production adoption is reported. Completing milestones alone does not make a 1.0 ([roadmap.md](../roadmap.md)).

## Accepting contributions

- Accepted via GitHub Issues / PRs.
- CONTRIBUTING.md includes the procedure for adding a compiler ([toolchain.md](toolchain.md)).

## Open questions

- Code of Conduct, and whether a DCO / CLA is required
