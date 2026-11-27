<!-- translated-from: docs/ja/specs/overview.md @ blob 272341ea3e12fe905e5912715c814fefadcc8d94 -->

# Overview

## Purpose

cuno is an integrated tool for C/C++. It combines package management, build management, toolchain management, and development tools into a single command, so that C/C++ development can be started and continued with **the same feel as cargo**.

What it replaces: CMake, Ninja, ctest, separate management of `.clang-format` / `.clang-tidy`, and so on.

## Pain points addressed

1. Configuration (CMakeLists.txt / .cmake) is hard to read and scattered across files
2. Projects cannot be started casually without an IDE
3. Testing mechanisms (ctest / GTest / Catch, etc.) are fragmented
4. Build and run arguments (`-Werror`, etc.) are not intuitive
5. Adding a library requires looking up hash values yourself. vcpkg is also hard to understand

## Users

- First user: the cuno developer themself, for **new projects started from scratch**.
- Future: adoption in professional work such as game development.
- cuno **fully replaces** CMake. Coexistence with existing CMake projects is not a goal.

## Goals (MVP)

The following workflow works on Windows 11:

```
cuno new mygame   # generate a project + sample code (with tests)
cd mygame
cuno fmt          # format
cuno lint         # static analysis
cuno test         # run tests
cuno run          # build and run
```

- If the workload turns out to be too heavy, the minimum is `cuno new` / `cuno test` / `cuno run`.
- The full set of commands in the MVP is `new` / `build` / `run` / `check` / `clean` / `test` / `fmt` / `lint` / `doctor` ([commands.md](commands.md)).
- Both C and C++ are supported (`cuno new --lang c`; the default is C++).
- Debug / Release can be switched (`--release`).

## Non-goals (outside the MVP; handled Right after MVP or later)

- Dependency management (`cuno add`, etc.)
- Support for external test frameworks (GTest / Catch2 / doctest)
- GCC support (Right after MVP)
- Automatic compiler installation
- C++20 modules
- Linux / macOS support
- Ninja backend
- Public package service

## Anti-goals (considered failures even if they work)

- Users have to write CMake or another build DSL
- Configuration is scattered across many files beyond `Cuno.toml` + `.cuno/config.toml` (the per-user `<cuno home>/config.toml` and the build script as an escape hatch are exceptions; see [manifest.md](manifest.md))
- An IDE is required to get started
- Users have to look up dependency hash values by hand
- Users have to memorize compiler-specific flags (`/WX`, `-Werror`, etc.)

## Implementation language

- cuno itself is implemented in **Rust**.
- Where extreme optimization is needed, **C or assembly** may also be used, but only when measurements confirm the benefit.

## Future considerations

- safec support (after cuno works with the major compilers; see [toolchain.md](toolchain.md))
