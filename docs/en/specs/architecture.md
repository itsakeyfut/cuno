<!-- translated-from: docs/ja/specs/architecture.md @ blob 69c19d3e3ce6100699384356c68aa14c04226b41 -->

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
  - A node of the graph (one step, such as a compilation) is data holding the program to run, its arguments, environment variables, working directory, inputs and outputs, plus the kind of step (a compilation with its source, a link, a custom step) and how its header dependencies are discovered (none, a depfile, MSVC's JSON, `/showIncludes`). The kind and the discovery method are held as values decided by `cuno-toolchain`, not guessed by reading the arguments again.
    - Rationale: so that compile_commands.json and a future Ninja backend can be produced from the graph without executing it. Both need to know which step compiles which source, and how header dependencies are discovered.

### What goes in `cuno-core`

Only the following three. To add anything else, change this specification first.

| Item | Contents |
|---|---|
| Error classification | The classification of exit codes (1: a problem with the user's code or build artifacts, 2: incorrect usage, 3: a configuration or environment problem, 101: an internal cuno error; [commands.md](commands.md#exit-codes)), and a common trait that every crate's error type implements (returning the classification and an optional `help:`). Displaying the error and exiting is done in one place, in the cuno binary |
| Output | Cargo-style progress lines (`Compiling` / `Finished` / `Running`), warnings and errors, color and verbosity (`[term]`, `NO_COLOR`). Crates do not print directly; they write through this |
| cuno home | Deciding where the cuno home is (as in [distribution.md](distribution.md#cuno-home-directory)) |

- Rationale: library crates cannot depend on the cuno binary. Without a shared place, either each crate decides exit codes and display on its own, or the binary must know every crate's errors and map them to exit codes. In the latter case, an error whose mapping was forgotten silently becomes exit code 1.

## Dependencies between crates

The allowed directions of dependency are limited to the following. `tests/architecture.rs` reads this table, compares it with each crate's dependencies as Cargo resolves them (normal dependencies and build-script dependencies, including target-specific ones), and fails on a dependency not in the table, or on a crate not in the table. Test-only dependencies (`[dev-dependencies]`) are out of scope, since they do not shape what is built.

- The "(later)" rows are provisional, to be revisited when the milestone that creates the crate designs it.

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
  - Rationale: it only runs the executables it is given and collects the results, so it needs no knowledge of settings or compilers.
- `cuno-tools` is given the list of target files the cuno binary built from "the places where the package's code lives" ([project-layout.md](project-layout.md#where-the-packages-code-lives)), and the path of compile_commands.json. What lint acts on is decided by the list; compile_commands.json only supplies each source's command. It does not depend on `cuno-build`.
- `cuno-build` does not depend on `cuno-deps` / `cuno-script`. Resolved dependencies and script results are passed by the cuno binary as data.
- **Library crates do not read the process's environment variables or command-line arguments directly.** The cuno binary collects them and passes them as values (for example, passing the list of environment variables to cuno home resolution and to compiler detection).
  - Likewise, the OS cuno runs on (the host) is received as a value rather than decided directly with `cfg!` and the like. The OS being built for (the target) is distinguished from the host, as in [manifest.md](manifest.md). In the MVP, the binary passes target = host.
  - Rationale: so that tests can construct any environment, and the behavior of a library does not depend on the environment of whatever process it happens to run in. Making the host a value lets branches for other OSes be tested on Windows CI as well.

## Owners of shared rules

A rule used by several commands is held in one place, by the crate below. Each command calls it rather than writing the same rule again.

| Rule | Owner |
|---|---|
| Interpreting command-line arguments, calling each part, displaying errors and deciding the exit code | `cuno` (using `cuno-core`'s classification) |
| Error classification, output, cuno home | `cuno-core` |
| Finding the project, resolving paths the user wrote (in configuration files and command-line arguments), reading and validating `cuno.toml` and the configuration files, the precedence of each value, layering the settings for the selected compiler, target and profile | `cuno-manifest` |
| Kinds of compiler, detection, selection precedence, environment resolution on Windows, version conditions, flag translation, how the target and the host are represented | `cuno-toolchain` |
| Classifying sources (entry files, package body, tests), the output paths cuno decides, the build graph, building tests, compile_commands.json, bundled headers | `cuno-build` |
| Running tests and displaying results | `cuno-test` |
| fmt / lint, generating `.clang-format` / `.clang-tidy` | `cuno-tools` |

- Settings are decided in this order: `cuno-manifest` reads each layer and decides the precedence of each value, `cuno-toolchain` selects the compiler from the result, and `cuno-manifest` layers `[platform.*]` / `[build.<compiler>]` / `[profile.*]` for the selected compiler, target and profile.
- Paths have two owners. Where a path the user wrote (in configuration files or command-line arguments) is resolved from belongs to `cuno-manifest`; where cuno puts its outputs (the layout inside `target/`) belongs to `cuno-build`.
  - Rationale: the former is a question of what the configuration means and the latter a question of how the build works, and they change for different reasons.

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
