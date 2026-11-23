# cuno

**One tool for C/C++ development, with the feel of Cargo.**

cuno brings package management, build management, toolchain management, and developer tools for C and C++ together under a single command.

```
cuno new mygame
cd mygame
cuno fmt
cuno lint
cuno test
cuno run
```

## Concept

- **Cargo as the model**
  - Commands, configuration, and output follow Cargo.
  - If you have used Cargo, you already know your way around.
  - cuno departs from Cargo only where C/C++ needs something different.
- **One config file, zero setup**
  - Everything lives in `cuno.toml`.
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

cuno replaces CMake, Ninja, ctest, and hand-written `.clang-format` / `.clang-tidy` files with one `cuno.toml`.

## Documentation

| Language | Concept | Roadmap | Specifications |
|---|---|---|---|
| English | [docs/en/concept.md](docs/en/concept.md) | [docs/en/roadmap.md](docs/en/roadmap.md) | [docs/en/specs/](docs/en/specs/README.md) |
| 日本語 | [docs/ja/concept.md](docs/ja/concept.md) | [docs/ja/roadmap.md](docs/ja/roadmap.md) | [docs/ja/specs/](docs/ja/specs/README.md) |

The Japanese documents are the source of truth. The English version is a translation for public readers; if the two disagree, the Japanese version prevails.

## Examples

[`examples/`](examples/) holds small C and C++ projects, one per feature cuno supports today: a single file, several files and headers, several executables, `[[bin]]`, `include-dirs`, the language edition and the release profile, per-compiler flags, and C inside a C++ package. Each builds, checks and runs on MSVC, Clang and clang-cl in CI.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.
