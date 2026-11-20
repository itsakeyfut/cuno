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

> **Status:** Specification phase. Nothing is implemented yet.
> The first milestone (MVP) targets Windows 11 with MSVC and Clang / clang-cl.

## Why

- Build configuration (CMakeLists.txt, `.cmake` files) is hard to read and scattered across many files.
- Starting a C/C++ project without an IDE is harder than it should be.
- Testing setups (ctest, GoogleTest, Catch2, ...) are fragmented.
- Compiler flags such as `-Werror` / `/WX` are not intuitive.
- Adding a library often means looking up hashes and wiring things up by hand.

cuno replaces CMake, Ninja, ctest, and hand-written `.clang-format` / `.clang-tidy` files with one `cuno.toml`.

## Documentation

| Language | Concept | Specifications |
|---|---|---|
| English | [docs/en/concept.md](docs/en/concept.md) | [docs/en/specs/](docs/en/specs/README.md) |
| 日本語 | [docs/ja/concept.md](docs/ja/concept.md) | [docs/ja/specs/](docs/ja/specs/README.md) |

The Japanese documents are the source of truth. The English version is a translation for public readers; if the two disagree, the Japanese version prevails.

## License

Licensed under either of

- Apache License, Version 2.0
- MIT License

at your option.
