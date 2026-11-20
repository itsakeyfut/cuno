<!-- translated-from: docs/ja/specs/build.md @ blob 6f98be2484075a987fdc05aaf7d80ebe38ff271b -->

# Build

## Build engine

- **cuno has its own build engine**. It works standalone and does not require Ninja / CMake.
- A choice between it and Ninja is not adopted (behavioral drift between two paths, plus the testing and maintenance burden, is not worth it).
- **Build graph construction and execution are separated**, leaving room to add a Ninja backend in the future.

## Incremental and parallel builds

- Header dependencies are obtained from `-MD`-style dependency files for GCC / Clang, `/sourceDependencies` (JSON output; VS 16.7 or later) for MSVC, and `/showIncludes` for clang-cl (clang-cl's output is in English regardless of language settings, so it does not have MSVC's problem).
  - MSVC does not use `/showIncludes` because on the Japanese edition of Visual Studio the output is in Japanese (「メモ: インクルード ファイル:」), making it language-dependent.
- Only what has changed is recompiled.
- Compilation runs in parallel.

## Default include paths

Include paths that cuno always adds, separately from `include-dirs` in `cuno.toml`.

| Path | Reason |
|---|---|
| `src/` | So that headers of the package body can be referenced like `#include "calc.h"` |
| `include/` | After library support (Later). For public headers |
| Bundled header location (`<cuno home>/include/<cuno version>/`) | **Always added, even in normal builds**, so that writing `#include <cuno/test.h>` without wrapping it in `#ifdef CUNO_TEST` does not break the normal build |

## Always-on flags

Flags that cuno always adds, independent of `cuno.toml` settings.

| Compiler | Flag | Reason |
|---|---|---|
| MSVC / clang-cl | `/nologo` | Suppresses the startup logo |
| MSVC / clang-cl | `/utf-8` | Sets the source and execution character sets to UTF-8. Prevents UTF-8 sources from being read as CP932 on Japanese Windows |
| MSVC / clang-cl | `/Zc:__cplusplus` | Makes the `__cplusplus` macro report the correct standard value |
| MSVC / clang-cl (C++ with `exceptions = true`) | `/EHsc` | Standard C++ exception handling |

- For flags determined by settings (warnings, optimization, debug info, C runtime, exceptions, RTTI, subsystem, etc.), see [manifest.md](manifest.md).

## Custom build steps (three layers)

Processing other than C/C++ compilation (shader compilation, code generation, asset conversion, etc.) is handled in the following three layers. Anything that can be written in a higher layer is written there. MVP has only layer 1. Layer 2 (custom rules) and layer 3 (build script) are "Later".

1. **`cuno.toml` (declarative settings)**: sources, defines, flags, etc. Most projects need only this.
2. **Custom rules (`cuno.toml`)**: declaratively write "convert these files, with this command, into this output". Dependency tracking (depfile support), execution order, and change detection are handled by cuno's build engine.
   ```toml
   [[build.rule]]
   inputs  = "shaders/**/*.glsl"
   command = "glslc {in} -o {out} -MD -MF {depfile}"
   output  = "{name}.spv"
   ```
3. **Build script (escape hatch)**: only for processing that cannot be written in 1 or 2. Written in **Lua** (e.g. `build.lua`). Lua is embedded in cuno itself, and scripts hook into the build through APIs provided by cuno.
   - cuno can restrict the operations a script can perform (sandbox).
   - Build scripts of dependency packages are run only if the user allows them in `cuno.toml`.

### Rationale

- Declarative settings alone cannot handle complex cases, so an escape hatch is needed (same idea as Rust's build.rs and Zig's build.zig).
- The pain points of CMake custom steps (dependency tracking, execution order, "silently doing nothing") are absorbed by cuno in layer 2.
- Layer 3 is a "rarely used escape hatch", so the benefit of writing it in C/C++ — "no need to learn a new language" — carries little weight. On the other hand, Lua's benefits apply every time, even to users who never write scripts:
  - **Safety**: build scripts of dependency packages written by others can be run safely with restricted operations.
  - **No waiting**: runs immediately without compilation.
  - **Easy cross-compilation**: no need to separately prepare a compiler for the host.
  - **Concise**: string and file operations are short to write.
  - **Familiar**: a language widely used in game development.
  - **Simple to implement**: easy to embed in Rust.

### Known costs and mitigations

| Cost | Mitigation |
|---|---|
| Requires writing a language other than C/C++ | Handle most cases in layers 1 and 2, reducing the situations where scripts are needed at all |
| Weak type checking and completion | Consider adopting a typed Lua variant (Luau, etc.) or providing type definition files for the API |
| C/C++ libraries cannot be used directly in build steps | Run external tools via layer 2 custom rules, or via command execution from scripts |
| Future version management due to embedding a language | Pin the adopted Lua version, and provide cuno's API with versioning |

### History

Originally, following Rust (build.rs) and Zig (build.zig), the plan was to write it in the same language as the project (`build.cpp` / `build.c`). The benefits of writing in C/C++ are "no need to learn a new language" and "type checking, completion, and debuggers work". However, in the three-layer structure, layer 3 is a "rarely used escape hatch", and since there are few occasions to write it, these benefits carry little weight. Lua's benefits (safety, no waiting, easy cross-compilation), on the other hand, apply every time, even to users who never write scripts. This re-evaluation led to switching to Lua.

## Output

- **compile_commands.json is always generated** (for clang-tidy and editor completion).
  - It is placed **at the project root** (`<project>/compile_commands.json`). `cuno new` adds it to `.gitignore`.
  - Reason: clangd only searches automatically in "the parent directories of the source" and their `build/` subdirectories, not inside `target/`. Placing it at the root lets clangd work with no configuration (the same experience as Rust's rust-analyzer). It is a generated artifact, not a configuration file users write, so it does not conflict with the "don't add configuration files" policy.
  - Other approaches considered: placing it inside `target/` and having cuno generate `.clangd` (adds a configuration file); having users configure the location in their editor (shifts the effort to users).
  - **Generation is independent of compilation**: cuno generates it from the build graph (the compile commands scheduled to run). Regardless of whether compilation succeeds, running any of `build` / `run` / `check` / `test` / `lint` updates it to the latest content. `cuno lint` works even if nothing has been built yet.
  - **Self-contained content**: MSVC / Windows SDK include paths (`/imsvc` etc. for clang-cl style) are written explicitly, so that clangd running outside a Developer Command Prompt can still find the standard headers and Windows headers.
  - **Content is always fixed**: it contains the commands for "the `dev` profile with `CUNO_TEST` defined". The content does not change regardless of which profile is built.
    - Reason: editor completion and clang-tidy can analyze in-source test code as well (just as rust-analyzer analyzes test code). If the content changed with every build, the editor's analysis results would fluctuate.

### Output directory

- The default is `target/`. Its contents are organized by target.
  - Normal: `target/<profile name>/`. However, the `dev` profile uses `debug/`, as in Cargo (`target/debug/`, `target/release/`; custom profiles keep their name, e.g. `target/profiling/`).
  - When cross-compiling: `target/<target>/<profile name>/`
  - Test builds: `target/<profile name>/test/<package name>/`. Kept separate so that objects built with `CUNO_TEST` defined do not mix with normal build objects. The package name is included for the same reason as for intermediate files.
  - Intermediate files (objects, etc.): `target/<profile name>/obj/<package name>/`. Prevents collisions when multiple projects share the output directory.
- `cuno clean` (with no arguments) deletes the entire output directory, as in Cargo.
- Users can change the output directory. Precedence:
  1. Command-line argument `--target-dir`
  2. Environment variable `CUNO_TARGET_DIR`
  3. Project-level `.cuno/config.toml`
  4. User-level `<cuno home>/config.toml`
  5. Default value `target/`
- `cuno new` adds the output directory to `.gitignore`.

## Profiles

- `dev` (default) and `release` are built in. Switch with `--release`.
- In MVP, `--profile` accepts only `dev` / `release`. Anything else is an error (exit code 3). Custom profiles are "Later".
- Custom profiles can be created by inheriting an existing one (same as Cargo).
  ```toml
  [profile.profiling]
  inherits = "release"   # Profile to inherit from (dev / release / another custom profile)
  debug-info = true
  ```
- For descriptions of each item and its values, see `[profile.<name>]` in [manifest.md](manifest.md).

## Build settings

- **Defines / include paths**: written in `cuno.toml`.
  ```toml
  [build]
  defines = ["GAME_DEBUG"]
  include-dirs = ["third_party/include"]
  ```
- **Raw flags escape hatch**: when the abstracted settings are not enough, raw flags can be written per compiler family.
  ```toml
  [build.msvc]
  flags = ["/Zc:preprocessor"]
  ```
- **Precompiled headers** (Later): opt-in. cuno absorbs the differences between compilers.
  ```toml
  [build]
  precompiled-header = "src/pch.h"
  ```
- **Unity builds** (Later): opt-in. For speeding up full builds of large projects.
- **Build cache** (Later): a setting to use sccache / ccache automatically. MVP does not use a build cache. A custom cache (shared across multiple machines, etc.) is Future.

## Open questions

- Change-detection method (timestamp / hash) and the storage format of build state
- [manifest.md](manifest.md) is the source of truth for the exact item names of each setting (the examples above are an overview)
- All items of custom rules
- Build script file name, Lua version (Lua 5.4 / LuaJIT / Luau, etc.), and API
