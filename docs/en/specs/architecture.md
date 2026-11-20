<!-- translated-from: docs/ja/specs/architecture.md @ blob 199f6a0cdf1d129cda476d5391b4ea1cf863fb2c -->

# Internal Structure of cuno Itself

## Crate layout

A Rust workspace, with crates split by role.

| Crate | Role | Timeline |
|---|---|---|
| `cuno` | CLI. Interprets commands and calls each crate. The main body of the single binary | MVP |
| `cuno-core` | Only the three things several crates share: classifying errors and exit codes, output (cargo-style display, color, verbosity), and resolving the cuno home | MVP |
| `cuno-manifest` | Loading and validating `cuno.toml` / `config.toml` ([manifest.md](manifest.md)) | MVP |
| `cuno-toolchain` | Compiler abstraction (interface), detection, per-compiler implementations, flag translation ([toolchain.md](toolchain.md)) | MVP |
| `cuno-build` | Building and executing the build graph, change detection, outputting compile_commands.json ([build.md](build.md)) | MVP |
| `cuno-test` | Test discovery, execution, and result output ([testing.md](testing.md)) | MVP |
| `cuno-tools` | fmt / lint (invoking clang-format / clang-tidy) ([dev-tools.md](dev-tools.md)) | MVP |
| `cuno-script` | Embedding Lua build scripts | Later |
| `cuno-deps` | Dependency resolution, fetching, and the lock file ([dependencies.md](dependencies.md)) | Dependency management stage 1 |

- Within `cuno-build`, "building the build graph" and "executing the graph" are separated into modules (leaving room for a future Ninja backend).
  - A node of the graph (one step, such as a compilation) is data holding only the program to run, its arguments, environment variables, working directory, inputs and outputs. Rationale: so that compile_commands.json and a future Ninja backend can be produced from the graph without executing it.

### What goes in `cuno-core`

Only the following three. To add anything else, change this specification first.

| Item | Contents |
|---|---|
| Error classification | The classification of exit codes (1: a problem with the user's code or build artifacts, 2: incorrect usage, 3: a configuration or environment problem, 101: an internal cuno error; [commands.md](commands.md#exit-codes)), and a common trait that every crate's error type implements (returning the classification and an optional `help:`). Displaying the error and exiting is done in one place, in the cuno binary |
| Output | Cargo-style progress lines (`Compiling` / `Finished` / `Running`), warnings and errors, color and verbosity (`[term]`, `NO_COLOR`). Crates do not print directly; they write through this |
| cuno home | Deciding where the cuno home is (`CUNO_HOME`, otherwise `%USERPROFILE%\.cuno`; [distribution.md](distribution.md)) |

- Rationale: library crates cannot depend on the cuno binary. Without a shared place, either each crate decides exit codes and display on its own, or the binary must know every crate's errors and map them to exit codes. In the latter case, an error whose mapping was forgotten silently becomes exit code 1.

## Dependencies between crates

The allowed directions of dependency are limited to the following. `tests/architecture.rs` checks each crate's `Cargo.toml` and fails on a dependency not in the table, or on a crate not in the table.

| Crate | May depend on |
|---|---|
| `cuno-core` | none |
| `cuno-toolchain` | `cuno-core` |
| `cuno-manifest` | `cuno-core`, `cuno-toolchain` |
| `cuno-build` | `cuno-core`, `cuno-manifest`, `cuno-toolchain` |
| `cuno-test` | `cuno-core` |
| `cuno-tools` | `cuno-core`, `cuno-manifest`, `cuno-toolchain` |
| `cuno-deps` (later) | `cuno-core`, `cuno-manifest`, `cuno-toolchain` |
| `cuno-script` (later) | `cuno-core`, `cuno-build` |
| `cuno` (the binary) | all |

- **`cuno-manifest` depends on `cuno-toolchain`, not the reverse.** `cuno-toolchain` defines the kinds of compiler and the types of the settings that flag translation receives, and `cuno-manifest` reads TOML into those types (`[build.<compiler>]` is also looked up by the kind of compiler).
  - Rationale: to keep the structure of TOML out of the compiler abstraction. Adding a compiler then takes a change to `cuno-toolchain` alone (design principle 5 in [concept.md](../concept.md)).
- `cuno-test` holds only running tests and displaying their results. Building tests is one mode of `cuno-build`.
- `cuno-tools` is given the path of compile_commands.json. It does not depend on `cuno-build`.
- `cuno-build` does not depend on `cuno-deps` / `cuno-script`. Resolved dependencies and script results are passed by the cuno binary as data.
- **Library crates do not read the process's environment variables or command-line arguments directly.** The cuno binary collects them and passes them as values (for example, passing the list of environment variables to cuno home resolution and to compiler detection).
  - Rationale: so that tests can construct any environment, and the behavior of a library does not depend on the environment of whatever process it happens to run in.

## Owners of shared rules

A rule used by several commands is held in one place, by the crate below. Each command calls it rather than writing the same rule again.

| Rule | Owner |
|---|---|
| Interpreting command-line arguments, calling each part, displaying errors and deciding the exit code | `cuno` (using `cuno-core`'s classification) |
| Error classification, output, cuno home | `cuno-core` |
| Finding the project, resolving paths, reading and validating `cuno.toml` and the configuration files, precedence and layering (combined into one resolved configuration) | `cuno-manifest` |
| Kinds of compiler, detection, selection precedence, environment resolution on Windows, version conditions, flag translation | `cuno-toolchain` |
| Classifying sources (entry files, package body, tests), output paths, the build graph, building tests, compile_commands.json, bundled headers | `cuno-build` |
| Running tests and displaying results | `cuno-test` |
| fmt / lint, generating `.clang-format` / `.clang-tidy` | `cuno-tools` |

## Repository layout

```
cuno/
├── Cargo.toml        # the workspace, and also the cuno package (binary)
├── src/main.rs       # the cuno binary
├── crates/           # the crates above other than cuno (libraries)
├── headers/cuno/     # bundled headers (test.hpp / test.h). Embedded into the binary at build time
├── templates/        # samples generated by cuno new
├── tests/            # integration tests for cuno itself (actually invoke compilers)
└── docs/specs/       # specifications
```

- The root is both the workspace and the `cuno` package.
  - Reason: unless the root is a package, the root `tests/` belongs to no package and is never built. Making the root the `cuno` package lets `tests/` run as integration tests of the cuno binary.
  - It also lets `cargo run` / `cargo install --path .` work at the root without `-p cuno`.

## Main external libraries

| Library | Purpose | Timeline |
|---|---|---|
| clap | Command-line argument parsing | MVP |
| serde + toml | Loading configuration files | MVP |
| mlua | Embedding Lua | Later |

## Testing cuno itself

- Unit tests per crate.
- Integration tests that actually invoke MSVC / Clang (`tests/`).
- CI: Windows runners on GitHub Actions (MSVC is installed).

## Rust

- edition 2024.
- **The MSRV (minimum supported Rust version) is 1.88.**
  - Reason: it is the first version where `let` chains (`if let Some(x) = a && b`) are available in edition 2024. cuno is full of code with stacked conditions, such as settings resolution, and they make it much easier to read.
  - cuno is distributed as prebuilt binaries, so the MSRV only affects users who build from source. Choosing a version newer than the edition 2024 minimum (1.85) therefore costs little.
  - Raising the MSRV is a deliberate change, and this specification is updated when it happens.
- The toolchain is not pinned (no `rust-toolchain.toml`). CI checks both the MSRV and stable.

## Open questions

- Parallel execution approach (thread pool / async)
