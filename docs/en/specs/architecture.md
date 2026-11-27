<!-- translated-from: docs/ja/specs/architecture.md @ blob 083504fd3c8adf61197117ad25977a399f80b261 -->

# Internal Structure of cuno Itself

## Crate layout

A Rust workspace, with crates split by role.

| Crate | Role | Timeline |
|---|---|---|
| `cuno` | The main body of the single binary. Collects input from the process, calls `cuno-cli`, and exits with the exit code it returns (`src/main.rs` only) | MVP |
| `cuno-cli` | The CLI. Interprets command-line arguments, resolves settings, dispatches to each command, displays errors and decides the exit code. A module per command combines the crates ([What goes in `src/` and `cuno-cli`](#what-goes-in-src-and-cuno-cli)) | MVP |
| `cuno-core` | Only the seven things several crates share: classifying errors and exit codes, output (cargo-style display, color, verbosity), resolving the cuno home, the list of environment variables, path identity, starting a built program, and the scratch directory | MVP |
| `cuno-manifest` | Loading and validating `cuno.toml` / `config.toml` ([manifest.md](manifest.md)) | MVP |
| `cuno-toolchain` | Compiler abstraction (interface), detection, per-compiler implementations, flag translation ([toolchain.md](toolchain.md)) | MVP |
| `cuno-build` | Building and executing the build graph, change detection, outputting compile_commands.json ([build.md](build.md)) | MVP |
| `cuno-test` | Test discovery, execution, and result output ([testing.md](testing.md)) | MVP |
| `cuno-tools` | fmt / lint (invoking clang-format / clang-tidy) ([dev-tools.md](dev-tools.md)) | MVP |
| `cuno-script` | Embedding Lua build scripts | Later |
| `cuno-deps` | Dependency resolution, fetching, and the lock file ([dependencies.md](dependencies.md)) | Dependency management stage 1 |

- Within `cuno-build`, "building the build graph" and "executing the graph" are separated into modules (leaving room for a future Ninja backend).
  - A node of the graph (one step, such as a compilation) is data holding the program to run, its arguments, inputs and outputs, plus the kind of step (a compilation with its source, a link, a custom step) and how its header dependencies are discovered (none, a depfile, MSVC's JSON, `/showIncludes`). The kind and the discovery method are held as values decided by `cuno-toolchain`, not guessed by reading the arguments again.
    - Environment variables and the working directory are not held per step, because they are the same for every step today (the compiler's environment and the project root). Custom steps (M7) are the first to need them to differ per step, and they will be held by the step then.
    - Every place that behaves differently by kind of step is written as a `match` covering every kind, so that adding a new kind (archives in M6, custom steps in M7) makes every place that does not answer for it a compile error.
    - Rationale: so that compile_commands.json and a future Ninja backend can be produced from the graph without executing it. Both need to know which step compiles which source, and how header dependencies are discovered.

### What goes in `cuno-core`

Only the following seven. To add anything else, change this specification first.

| Item | Contents |
|---|---|
| Error classification | The classification of exit codes (1: a problem with the user's code or build artifacts, 2: incorrect usage, 3: a configuration or environment problem, 101: an internal cuno error; [commands.md](commands.md#exit-codes)), and a common trait that every crate's error type implements (returning the classification and an optional `help:`). Displaying the error is done in one place, in `cuno-cli`, and exiting the process in one place, in the cuno binary |
| Output | Cargo-style progress lines (`Compiling` / `Finished` / `Running`), warnings and errors, color and verbosity (`[term]`, `NO_COLOR`). Crates do not print directly; they write through this |
| cuno home | Deciding where the cuno home is (as in [distribution.md](distribution.md#cuno-home-directory)) |
| List of environment variables | The type through which the environment variables the cuno binary collected are passed to libraries as a value. On Windows, names are looked up without distinguishing upper and lower case |
| Path identity | The one way of judging whether two paths are the same place, or whether one is inside the other ([project-layout.md](project-layout.md#source-collection-and-linking)). Classifying sources, checking the output directory, and matching Visual Studio locations use it. Rationale: the same question had several answers that compared spellings in different ways, and each new place that used it had the same kind of mistake fixed separately |
| Starting a built program | How cuno decides, when it starts an executable it built, the environment variables (only those cuno received; the compiler environment is not passed on), the working directory (the project root) and the path it displays (relative to the current directory) ([commands.md](commands.md#cuno-run), [testing.md](testing.md#execution)). `cuno run` and test execution use it. How standard input and output are handled (`cuno run` leaves them connected, tests capture them) is decided by the caller. Rationale: it was written in two places, and the display of one was later brought back in line with the other |
| Scratch directory | How to create a directory used by a single run only and shared with nothing else, and remove it when done. Its name is `cuno-<command>-<process id>-<n>`; an existing one is never used, whoever created it, and the next number is tried instead. When it cannot be created in the temporary directory (on Windows, the one the environment variable `TMP`, or else `TEMP`, names), every command reports ``error: cannot create <directory> to <purpose>: <reason>`` (`<purpose>` is `check clang`, `format`, `analyse` and so on) with `help: set TMP and TEMP to a directory that exists and can be written` (exit code 3). Work such as checking the standard library creates it only once that work is known to be needed. A build also checks that one can be created before compiling, even with a compiler that has no standard library check (MSVC), with `<purpose>` `compile`: the compiler itself uses the temporary directory (cl stops with D8050 when it cannot), and its message makes the cause hard to read. The build, `cuno fmt`, `cuno lint`, `cuno doctor` and test execution use it. Rationale: it was written in four places, the same bug (reusing a name) was fixed twice separately, and the error and `help:` when `TEMP` was unusable also differed from command to command |

- Reason for the list of environment variables: compiler detection, cuno home resolution and reading the configuration all need environment variables, and libraries receive them as values ([Dependencies between crates](#dependencies-between-crates)). It keeps the case-insensitive lookup on Windows from being written again in each crate.
- Rationale: library crates cannot depend on the cuno binary. Without a shared place, either each crate decides exit codes and display on its own, or `cuno-cli` must know every crate's errors and map them to exit codes. In the latter case, an error whose mapping was forgotten silently becomes exit code 1.

### What goes in `src/` and `cuno-cli`

`src/` holds only `main.rs`. `main.rs` collects what only the process can provide (the command-line arguments, the environment variables, the current directory, the host, the location of the temporary directory, the number of CPU cores, standard output and standard error and whether each is a terminal), passes them to `cuno-cli`, and ends the process with the exit code it returns. Only `main.rs` ends the process.

`cuno-cli` holds only the following.

- `lib.rs`: interpreting command-line arguments (the values it receives are parsed with clap, and help and errors are written to the output it receives), resolving settings, dispatching to each command, displaying errors and deciding the exit code.
- Command modules, limited to those in the table below.
  - A command module only combines and calls the crates (including choosing the real implementations and passing them in) and displays the result. It holds no rule that a crate could answer if given the values (naming rules, diagnostic judgements, how paths are decided, and so on).
  - Data used only by that command (the templates of `cuno new`) may be embedded.

Anything several commands use is not put in `cuno-cli`, but in the crate listed in [Owners of shared rules](#owners-of-shared-rules).

The command modules are limited to those in the following table. `tests/architecture.rs` reads this table and fails if the modules of `cuno-cli` do not match it. To add a command, add a row to this table (`fmt` and `lint` become modules that call `cuno-tools`).

| Module | Commands |
|---|---|
| `build` | `build`, `check`, `run`, `test` |
| `clean` | `clean` |
| `doctor` | `doctor` |
| `fmt` | `fmt` |
| `lint` | `lint` |
| `new` | `new` |
| `toolchain` | `toolchain list`, `toolchain default` |

- `build` holds four commands because they share one build flow, as in [commands.md](commands.md#available-for-build-commands-build--run--check--test).
- Rationale: when there is a place to put processing that several commands share, processing accumulates there without anyone asking which crate should answer, and rules get duplicated (the toolchain preparation shared by the build and doctor, and doctor holding a copy of the compiler selection order). Removing the shared place forces the decision of where something belongs at the time it is written. Also, because only `main.rs` reads the process, commands can be called with values alone.

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
| `cuno-cli` | all |
| `cuno` (the binary) | `cuno-cli`, `cuno-core` |

- **`cuno-manifest` depends on `cuno-toolchain`, not the reverse.** `cuno-toolchain` defines the kinds of compiler and the types of the settings that flag translation receives, and `cuno-manifest` reads TOML into those types (`[build.<compiler>]` is also looked up by the kind of compiler).
  - Every decision that branches on the kind of compiler (how its command line is written, whether it is of the MSVC or the LLVM family, which LLVM tool it is) is answered in one place in `cuno-toolchain`, covering every kind. Other code does not compare kinds but uses those answers, so a new compiler does not compile while any decision is unanswered for it.
  - The names `cuno.toml` can be written with (the keys of `[build.<compiler>]` and `[toolchain.version]`) belong to `cuno-manifest`, since they include compilers cuno recognises but cannot build with (`gcc` / `safec`). The mapping from a kind of compiler to its name is made in one place in `cuno-manifest`.
  - Rationale: to keep the structure of TOML out of the compiler abstraction. Adding a compiler then takes a change to `cuno-toolchain` alone (design principle 5 in [concept.md](../concept.md)).
- `cuno-test` holds only running tests and displaying their results. Building tests is one mode of `cuno-build`.
  - Rationale: it only runs the executables it is given and collects the results, so it needs no knowledge of settings or compilers.
- `cuno-cli` gives `cuno-tools` the list of target files in "the places where the package's code lives" ([project-layout.md](project-layout.md#where-the-packages-code-lives)), and the path of compile_commands.json. `cuno-build` builds the list with the same walk as classifying sources (the output directory, links to directories, how upper and lower case are treated). What lint acts on is decided by the list; compile_commands.json only supplies each source's command. `cuno-tools` does not depend on `cuno-build`.
  - Rationale: with two walks, the handling of the output directory and of other spellings of the same name would diverge (the same kind of mistake has been fixed several times in classifying sources). The command module (`cuno-cli`) holds no rules, so the walk is placed where classifying sources is.
- `cuno-build` does not depend on `cuno-deps` / `cuno-script`. Resolved dependencies and script results are passed by `cuno-cli` as data.
- **Library crates do not read the process's environment variables, command-line arguments, current directory, the location of the temporary directory or the number of CPU cores directly.** The cuno binary (`main.rs`) collects them and passes them as values (for example, passing the list of environment variables to cuno home resolution and to compiler detection).
  - Likewise, the OS cuno runs on (the host) is received as a value rather than decided directly with `cfg!` and the like. The OS being built for (the target) is distinguished from the host, as in [manifest.md](manifest.md). In the MVP, the binary passes target = host.
  - Rationale: so that tests can construct any environment, and the behavior of a library does not depend on the environment of whatever process it happens to run in. Making the host a value lets branches for other OSes be tested on Windows CI as well.

## Owners of shared rules

A rule used by several commands is held in one place, by the crate below. Each command calls it rather than writing the same rule again.

| Rule | Owner |
|---|---|
| Interpreting command-line arguments, resolving settings, dispatching to each command, displaying errors and deciding the exit code | `cuno-cli` (using `cuno-core`'s classification) |
| Collecting input from the process, and ending the process | `cuno` |
| Error classification, output, cuno home, list of environment variables, path identity, starting a built program, scratch directory | `cuno-core` |
| Finding the project, resolving paths the user wrote (in configuration files and command-line arguments), reading and validating `cuno.toml` and the configuration files, the precedence of each value, layering the settings for the selected compiler, target and profile | `cuno-manifest` |
| Kinds of compiler, detection, selection precedence, environment resolution on Windows, version conditions, flag translation, how the target and the host are represented | `cuno-toolchain` |
| Classifying sources (entry files, package body, tests), the output paths cuno decides, the build graph, building tests, compile_commands.json, bundled headers, preparing the compiler used for the build (connecting `cuno-toolchain`'s detection, selection and environment resolution with the check of the standard library against the build settings) | `cuno-build` |
| Running tests and displaying results | `cuno-test` |
| fmt / lint, generating `.clang-format` / `.clang-tidy` | `cuno-tools` |

- Settings are decided in this order: `cuno-manifest` reads each layer and decides the precedence of each value, `cuno-toolchain` selects the compiler from the result, and `cuno-manifest` layers `[platform.*]` / `[build.<compiler>]` / `[profile.*]` for the selected compiler, target and profile.
- Paths have two owners. Where a path the user wrote (in configuration files or command-line arguments) is resolved from belongs to `cuno-manifest`; where cuno puts its outputs (the layout inside `target/`) belongs to `cuno-build`.
  - Rationale: the former is a question of what the configuration means and the latter a question of how the build works, and they change for different reasons.

## Repository layout

```
cuno/
├── Cargo.toml        # the workspace, and also the cuno package (binary)
├── src/main.rs       # the cuno binary (the only file in src/)
├── crates/           # the crates above other than cuno (libraries, including cuno-cli)
├── tests/            # integration tests for cuno itself (actually invoke compilers)
└── docs/specs/       # specifications
```

- The bundled headers (`test.hpp` / `test.h` / `test_main.c`) live in `crates/cuno-build/headers/cuno/` and the samples generated by `cuno new` live in `crates/cuno-cli/templates/`; each is embedded into the binary at build time.
  - Reason: a package published to crates.io contains only the files inside that crate's directory. A file that is embedded must live inside the crate that embeds it.
- The root is both the workspace and the `cuno` package.
  - Reason: unless the root is a package, the root `tests/` belongs to no package and is never built. Making the root the `cuno` package lets `tests/` run as integration tests of the cuno binary.
  - It also lets `cargo run` / `cargo install --path .` work at the root without `-p cuno`.

## Main external libraries

| Library | Purpose | Timeline |
|---|---|---|
| clap | Command-line argument parsing | MVP |
| serde + toml | Loading configuration files | MVP |
| toml_edit | Rewriting configuration files (`cuno toolchain default`, keeping comments and other keys) | M5 |
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
