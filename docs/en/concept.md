<!-- translated-from: docs/ja/concept.md @ blob bfd5cb6f1b129b85e06c47726cdefaa46223cf01 -->

# cuno concept

## A better C/C++ development experience

cuno is a tool that brings together package management, building, toolchain management, testing, formatting and static analysis for C/C++ development under one command and one configuration file.

```
cuno new mygame
cd mygame
cuno run
```

## Where the name comes from

The name combines **C** and **Uno** ("one" in Spanish and Italian). It was chosen with the goal of becoming **the one C/C++ development tool**: a single tool that provides everything C/C++ development needs.

## Why build it

Rust developers can work along a single, clear path, starting with `cargo new` and moving on to `cargo build`, `cargo test`, `cargo fmt` and `cargo clippy`. C/C++ has nothing like that.

To start a C/C++ project, you have to choose each of the following separately, configure each in its own syntax, and wire them together.

| What you want to do | What C/C++ uses today |
|---|---|
| Build configuration | CMake (CMakeLists.txt, `.cmake`) |
| Running builds | Ninja / MSBuild / Make |
| Dependencies | vcpkg / Conan / by hand |
| Testing | ctest + GoogleTest / Catch2 / doctest |
| Formatting and static analysis | `.clang-format` / `.clang-tidy` |
| Getting started | New-project wizards in Visual Studio or CLion |

The result is the following pain points.

1. Configuration is hard to read and scattered across many files
2. It is hard to get started casually without an IDE
3. There is a proliferation of competing test frameworks
4. You have to remember compiler-specific arguments such as `-Werror` / `/WX`
5. Every time you add a library, you have to look up its hash and write the configuration

This is not just a personal impression. In the ISO C++ developer survey, managing dependencies has ranked among the "biggest pain points" for years, and many developers also cite managing CMake projects as a pain point.
More than a third of C++ developers do not write unit tests, and behind this lies "the effort of setting them up".

cuno solves these as **the experience of a single tool**.

## Vision

### Definition of success

**Once C/C++ developers install cuno, they can create C/C++ projects and carry development forward using only cuno commands.**

### Long-term picture

At first, cuno invokes existing tools such as clang-format and clang-tidy from within cuno.
Later, it gradually replaces them with versions rebuilt for cuno, freeing C/C++ developers from a scattered collection of tools.

### Users three years from now

1. **Individual developers** use cuno and enjoy the experience
2. They start using cuno **at work** as well
3. Bugs found in real-world use are filed as **issues**, and improvements arrive as **PRs**

The aim is a flow from individuals finding cuno "pleasant to use", to adoption in professional work, and then to contributions.
The target is professional C/C++ development, starting with game development, with an emphasis on being adoptable even in workplaces centered on Windows and Visual Studio.

## Design principles

### 1. Base it on cargo

cargo is the benchmark for features and usability. From the command structure and the approach to configuration files to the output format, the aim is an experience in which anyone who has used cargo never feels lost.
Only where cargo's approach does not fit C/C++ is it adapted to the circumstances of C/C++.

### 2. Keep configuration in one place

Project configuration goes in `cuno.toml`. Instead of a dedicated language like CMake's, it uses a form that can be read and written as TOML. Users do not need to write `.clang-format` or `.clang-tidy`.

### 3. Work without configuration by default, and allow changes when needed

Right after `cuno new`, everything works without writing a single line of configuration. At the same time, cuno never enforces a directory layout or coding rules.
Whether you keep `include/` separate, put headers alongside sources in `src/`, or split them into `Public/` / `Private/` in the Unreal Engine style, `cuno.toml` can accommodate any convention.

### 4. cuno absorbs compiler differences

Write `warnings-as-errors = true`, and MSVC receives `/WX` while GCC / Clang receive `-Werror`. Users do not need to remember compiler-specific flags. For cases this does not cover, an escape hatch for writing raw flags remains.

### 5. Treat all major compilers equally

All three families, GCC, Clang and MSVC, are targeted. Windows and MSVC are not left for later. The design allows a new compiler to be added by implementing a single interface.

### 6. Assume modern C/C++

The target is C++17 / C11 and later; the default is C++20 / C17. With C++20, tests can be written without macros.
The experience of newcomers is not sacrificed for the sake of old conventions.

### 7. Self-contained in a single binary

cuno runs as a single executable. There is no need to install CMake or Ninja separately.

### 8. Make realistic decisions that work in practice

Working in real-world settings takes priority over ideals. For example, the following decisions were made.

- The way tests are built was chosen so that it does not conflict with SDL3's `#define main SDL_main`
- The C runtime default was chosen so that mixing in prebuilt dependencies does not cause link errors
- A mechanism that does not depend on the output language was chosen so that cuno works with the Japanese edition of Visual Studio

### 9. Allow complex configuration, and never be silently wrong

The special, complex configuration of large projects can also be written. There are three tiers, and anything that can be written in a higher tier is written there: declarations in `cuno.toml`, custom rules (`[[build.rule]]`), and Lua build scripts, in that order ([build.md](specs/build.md#custom-build-steps-three-layers)).

In every tier, whenever the result could differ from what was written, cuno reports an error instead of guessing. For example:

- A pattern that matches no files
- An output that was declared but not produced
- A value that changes silently, outside the layering rules (a script overriding a declaration, for example)
- An input missed by the rebuild check

Reporting success and doing something different damages users' trust far more than stopping with an error because something cannot be written. A survey of complaints about existing tools found that most took this form ([survey](../research/build-tool-complaints.md) (in Japanese)).

Keep the amount to learn small. New concepts are not required until they are needed. The order in which settings are read, and which layer wins, are explained on one page ([manifest.md "Settings layering"](specs/manifest.md#settings-layering)).

## What cuno does not do

- **It does not aim to coexist with existing CMake projects.** cuno is a replacement for CMake.
- **It does not enforce a directory layout or coding rules.** There are defaults, but you are not forced to follow them.
- **It does not become a general-purpose build system that can do anything.** It focuses on C/C++ development. For complex processing, Lua build scripts are provided as an escape hatch, but the aim is for most projects never to need them. The escape hatch, too, is never silently wrong, as principle 9 requires.

## How cuno differs from existing tools

cuno's design is not built on experience from comparing existing tools; it is built **on cargo**, based on **survey results about C/C++ developers' pain points and opinions**. The main differences from tools for C/C++ are as follows.

| Tool | Characteristics | Difference from cuno |
|---|---|---|
| CMake + vcpkg / Conan | The de facto standard. The broadest coverage | Requires combining multiple tools and syntaxes. cuno unifies them into one |
| xmake | Builds are described in Lua. Includes package management | Configuration centers on Lua scripts. cuno centers on TOML declarations, with Lua limited to an escape hatch |
| build2 | A full-fledged toolchain aiming for a cargo-like experience | Aimed at large, complex projects, with a steep learning curve. cuno prioritizes ease of getting started |
| cabin (formerly Poac) | A tool strongly influenced by cargo | Like cuno, it takes cargo as its model. cuno targets Windows / MSVC as a primary platform from the start and emphasizes professional game development |

## Long-term outlook

cuno is a project meant to be grown over the long term.

| Stage | Contents |
|---|---|
| MVP | Windows 11 + MSVC / Clang. `new` / `build` / `run` / `check` / `clean` / `test` / `fmt` / `lint` / `doctor` |
| Right after MVP | GCC support, `cuno init`, listing and selecting toolchains |
| Dependency management | Git / URL sources → recipe index (`cuno add sdl3`) → a public package service in the future |
| Later | Libraries and workspaces, Linux / macOS, cross-compilation (Android / WebAssembly), IDE integration, build cache |
| Future | In-house formatter, automatic compiler installation, C++20 modules |

### About safec

As an experiment in making memory-safe C/C++, cuno's developer is also developing a compiler, **[safec](https://github.com/itsakeyfut/safec)**.
It is a project that explores how much of Rust's safety (memory safety, thread safety, lifetimes, ownership) can be brought to C/C++.

Once cuno is established on the major compilers, it plans to treat safec as one of the compiler choices alongside Clang / MSVC / GCC.

## License

Dual-licensed under MIT and Apache-2.0. This is so that cuno, including the parts compiled together with user code such as the test headers, can be used with confidence even in commercial game development.
