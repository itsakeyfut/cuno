# cuno

[![Crates.io](https://img.shields.io/crates/v/cuno.svg)](https://crates.io/crates/cuno)
[![GitHub Release](https://img.shields.io/github/v/release/itsakeyfut/cuno)](https://github.com/itsakeyfut/cuno/releases/latest)
[![CI](https://github.com/itsakeyfut/cuno/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/itsakeyfut/cuno/actions/workflows/ci.yml)
[![Platform](https://img.shields.io/badge/platform-Windows%20x64-0078D6.svg)](#installation)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

**Everything C/C++ development needs, in one tool.**

cuno brings package management, build management, toolchain management, and developer tools for C and C++ together under a single command.

```
cuno new mygame
cd mygame
cuno fmt
cuno lint
cuno test
cuno run
```

## Installation

cuno runs on Windows x64. It builds with the compilers already on the machine: Visual Studio's MSVC (Build Tools with "Desktop development with C++" are enough) or LLVM's clang and clang-cl. `cuno fmt` and `cuno lint` use LLVM's clang-format and clang-tidy, 17 or later. `cuno doctor` says what it finds and what is missing.

**From a release.** Download `cuno-<version>-x86_64-pc-windows-msvc.zip` from [Releases](https://github.com/itsakeyfut/cuno/releases), check it against the `.sha256` beside it, unzip it, and put the folder holding `cuno.exe` on your `PATH`:

```powershell
certutil -hashfile cuno-<version>-x86_64-pc-windows-msvc.zip SHA256
```

`cuno.exe` is not code-signed, so Windows SmartScreen may warn the first time it runs.

**With Cargo.**

```sh
cargo install cuno
```

## Concept

- **Cargo as the model**
  - Commands, configuration, and output follow Cargo.
  - If you have used Cargo, you already know your way around.
  - cuno departs from Cargo only where C/C++ needs something different.
- **One config file, zero setup**
  - Everything lives in `Cuno.toml`.
  - A project created with `cuno new` builds without writing a single line of config.
  - Directory layout and coding rules are never forced on you.
- **Every major compiler, on equal terms**
  - GCC, Clang, and MSVC are all first-class. Windows is not an afterthought.
  - Settings like `warnings-as-errors = true` become each compiler's own flags.

See [docs/en/concept.md](docs/en/concept.md) for the full concept and design principles.

## Why

- Build configuration (CMakeLists.txt, `.cmake` files) is hard to read and scattered across many files.
- Starting a C/C++ project without an IDE is harder than it should be.
- Testing setups (ctest, GoogleTest, Catch2, ...) are fragmented.
- Compiler flags such as `-Werror` / `/WX` are not intuitive.
- Adding a library often means looking up hashes and wiring things up by hand.

cuno replaces CMake, Ninja, ctest, and hand-written `.clang-format` / `.clang-tidy` files with one `Cuno.toml`.

## Documentation

| Language | Concept | Roadmap | Specifications | Migration guide |
|---|---|---|---|---|
| English | [docs/en/concept.md](docs/en/concept.md) | [docs/en/roadmap.md](docs/en/roadmap.md) | [docs/en/specs/](docs/en/specs/README.md) | [docs/en/guides/migration.md](docs/en/guides/migration.md) |
| 日本語 | [docs/ja/concept.md](docs/ja/concept.md) | [docs/ja/roadmap.md](docs/ja/roadmap.md) | [docs/ja/specs/](docs/ja/specs/README.md) | [docs/ja/guides/migration.md](docs/ja/guides/migration.md) |

Coming from CMake, Visual Studio, ctest, clang-format, clang-tidy, vcpkg, Conan, Cargo, Premake, Meson or Bazel? The migration guide shows, tool by tool, what you did there and what you do in cuno.

The Japanese documents are the source of truth. The English version is a translation for public readers; if the two disagree, the Japanese version prevails.

## Examples

[`examples/`](examples/) holds small C and C++ projects, one per feature cuno supports today: a single file, several files and headers, several executables, `[[bin]]`, `include-dirs`, the language edition and the release profile, per-compiler flags, and C inside a C++ package. Each builds, checks and runs on MSVC, Clang and clang-cl in CI.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.
