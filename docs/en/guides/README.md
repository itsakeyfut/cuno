<!-- translated-from: docs/ja/guides/README.md @ blob f1eb411c9440eb3e82224850a74c8775b34b87fe -->

# Guides

While the specification ([specs/](../specs/README.md)) decides how cuno behaves, the guides read the specification again for a given purpose and explain what to write and how. The guides decide nothing new. Where they disagree, the specification is right.

## Getting started

- [Migrating from other tools](migration.md): how to do in cuno what you did with CMake, Visual Studio, ctest, clang-format and the like.

## By kind of project

- [Settings for game development](game-development.md): the settings games often choose, such as turning off exceptions and RTTI or building the C runtime into the executable, and what stops working once they are chosen.
- OS, kernel and embedded development: to be written in M11, where cuno supports freestanding builds ([roadmap.md](../roadmap.md#m11-os-and-embedded-development-freestanding)).

## By compiler

- [Compiler guide](compilers.md): for MSVC (cl), Clang (clang / clang-cl) and GCC (MinGW-w64), how to install them, how to specify their location and version, and how they differ from the other compilers.
