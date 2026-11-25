<!-- translated-from: docs/ja/roadmap.md @ blob 29739f3584f57c7ef63e029bcc241145b70f1052 -->

# cuno Roadmap

> The specification of each feature in [specs/](specs/README.md) is the source of truth; this document describes "in what order, and with what completion criteria".
> Timeline labels (MVP / Right after MVP / Dependency management stage / Later / Future) follow the definitions in [specs/README.md](specs/README.md).

## How we proceed

- Milestones are worked on **in order**. Work on the next milestone starts only after everything in the current milestone is complete.
- **There are no deadlines**. A milestone is complete when its completion criteria are met.

## Overview

| Milestone | Content | Version | Timeline in the spec |
|---|---|---|---|
| M0 | Development foundation | — | — |
| M1 | Configuration and toolchains | — | MVP |
| M2 | Build and run | — | MVP |
| M3 | Testing | — | MVP |
| M4 | Formatting, static analysis, and finishing the MVP | v0.1.0 | MVP |
| M5 | GCC and toolchain management | v0.2.0 | Right after MVP |
| M6 | Libraries and dependency management (Git / URL) | v0.3.0 | Dependency management stage 1 |
| M7 | Recipe index and custom builds | v0.4.0 | Dependency management stage 2 / Later |
| M8 | Multi-platform | v0.5.0 | Later |
| M9 | IDE integration and build efficiency | v0.6.0 | Later |
| M10 | Expanded testing and distribution | v0.7.0 | Later |
| — | Future considerations | — | Future |

- M1–M4 make up the MVP. Because the MVP is large, it is split into four parts so that working pieces can be built up one at a time.
- From M6 onward, items with strong dependencies come first (e.g. dependency management requires library support).
- Making existing libraries such as SDL3 usable (M7) takes priority over Linux / macOS support (M8). This is to establish practical usefulness for game development first.
- GCC support (M5) is not urgent, but is placed before dependency management (M6). Adding a third compiler family early lets us check at an early stage that the compiler abstraction is not biased toward MSVC / Clang.

## M0: Development foundation

Build the groundwork for developing cuno itself.

- Skeleton of the Rust workspace and crates ([architecture.md](specs/architecture.md))
- CI (GitHub Actions Windows runners: build, test, clippy, rustfmt)
- Integration test infrastructure (tests that actually invoke MSVC / Clang can run under `tests/`)

**Completion criteria**: the empty crates are built and tested in CI.

## M1: Configuration and toolchains

Enable cuno to correctly understand the project and the environment.

- Loading and validating `cuno.toml` / `.cuno/config.toml` (warnings for unknown keys, warnings / errors for items outside the MVP)
- Project discovery (walking up parent directories) and path resolution
- Compiler detection (Visual Studio / Build Tools, LLVM) and selection precedence
- Environment resolution on Windows (MSVC / Clang usable without the Developer Command Prompt)
- Translation from settings to compiler flags (MSVC / clang / clang-cl)
- `cuno doctor`

**Completion criteria**: `cuno doctor` correctly diagnoses the presence and versions of MSVC / Clang, and the presence of clang-format / clang-tidy.

## M2: Build and run

Enable building and running C/C++ projects.

- Source collection (entry file and package body)
- Built-in build engine (build graph, incremental builds, parallel compilation)
- Header dependency tracking (`/sourceDependencies`, `-MD`, `/showIncludes`)
- Output directory (`target/`) and profiles (`dev` / `release`)
- Generation of compile_commands.json
- `cuno build` / `cuno run` / `cuno check` / `cuno clean`

**Completion criteria**: a project with a hand-written `cuno.toml` can be run with `cuno run` using both MSVC and Clang, and incremental builds are correct even when only a header is changed.

## M3: Testing

Enable writing and running tests.

- Bundled headers (`cuno/test.hpp` / `cuno/test.h`), and embedding them in the binary and extracting them
- Test builds (`CUNO_TEST`, excluding the entry file)
- Test runner (one process per test, parallel execution, output capture)
- `cuno test`
- `cuno new` (generates a sample with tests)

**Completion criteria**: `cuno new` → `cuno test` → `cuno run` works for both C and C++, with both MSVC and Clang.

## M4: Formatting, static analysis, and finishing the MVP

Complete the MVP workflow and ship the first release.

- `cuno fmt` / `cuno lint` (invoking clang-format / clang-tidy, preset, allow / warn / deny)
- Generation of `.clang-format` / `.clang-tidy`
- Polishing output messages and exit codes
- Distribution (binaries on GitHub Releases)

**Completion criteria**: the MVP workflow defined in [overview.md](specs/overview.md) (`cuno new` → `cuno fmt` → `cuno lint` → `cuno test` → `cuno run`) works on Windows 11 and is distributed as v0.1.0.

## M5: GCC and toolchain management

- GCC support (including documenting the steps for adding a compiler to the compiler abstraction)
- `cuno init`
- `cuno toolchain list` / `cuno toolchain default`
- `cuno self update` (updating to the latest published release; designed once v0.1.0 has settled the form of distribution, and built on that form)

**Completion criteria**: the MVP workflow works with GCC. The test environment is whichever is less effort at the time work starts (MinGW on Windows, or Linux). A cuno installed after v0.1.0 can update itself to the latest published release with `cuno self update`.

## M6: Libraries and dependency management (Git / URL)

Enable cuno-format packages to be used as dependencies of each other.

- Library projects (`[lib]`, `cuno new --lib`, static / shared / header-only)
- `[dependencies]` (Git / URL sources), `cuno add` / `remove` / `update` / `tree` / `fetch`
- `cuno.lock` (automatic retrieval and recording of hashes)
- Version resolution (one version per library)

**Completion criteria**: a cuno-format library on Git can be added with `cuno add --git` and built without looking up its hash.

## M7: Recipe index and custom builds

Make existing libraries (SDL3, FFmpeg, etc.) usable with just `cuno add`.

- Recipe index (a Git repository) and `cuno add sdl3`
- Mechanism for invoking existing build systems (CMake, etc.) behind the scenes according to a recipe
- `cuno vendor`
- features, system libraries (`system = true`), license display and warnings
- Custom rules (`[[build.rule]]`) and Lua build scripts (implemented here because recipes also need them)

**Completion criteria**: a project using SDL3 can be run with `cuno add sdl3` → `cuno run`.

## M8: Multi-platform

- Linux / macOS support (including distribution of cuno itself)
- Cross-compilation (`--target`, Android / WebAssembly)

**Completion criteria**: the MVP workflow works on Linux / macOS, and builds for Android / WebAssembly can be made from Windows.

## M9: IDE integration and build efficiency

Make day-to-day development comfortable.

- IDE integration (`cuno ide vs` / `cuno ide vscode`, `cuno metadata`)
- Faster builds (build cache, precompiled headers, unity builds)
- Custom profiles (`inherits`), `[alias]`
- Workspaces (multiple packages)

**Completion criteria**: a cuno project can be debugged with the Visual Studio debugger, and multiple packages in a workspace (e.g. engine / game / editor) can be built together.

## M10: Expanded testing and distribution

- Expanded testing (`cuno bench`, sanitizers, coverage)
- External test frameworks (GTest / Catch2 / doctest)
- `cuno install`

**Completion criteria**: existing test suites using GTest can be run with `cuno test`.

## Future considerations

- A custom formatter (replacing clang-format)
- Automatic compiler installation (`cuno toolchain install`)
- C++20 modules
- A public package service (dependency management stage 3, `cuno package` / `publish`)
- `cuno doc`
- Ninja backend
- safec support

## About v1.0

From 1.0 onward, backward compatibility of `cuno.toml` is guaranteed ([distribution.md](specs/distribution.md)).

Whether to release 1.0 **will be considered once production adoption is reported**. Completing milestones alone does not make a 1.0.
