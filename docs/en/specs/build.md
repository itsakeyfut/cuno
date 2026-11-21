<!-- translated-from: docs/ja/specs/build.md @ blob 8009773809148be7e3338e1f786df340832f1e0c -->

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

## Settings and flags

How the settings in `cuno.toml`, and the flags cuno always adds, become each compiler's arguments. The flags are written only here; other documents link to this section. `clang` is the GNU-style Clang (targeting `x86_64-pc-windows-msvc`). Every mapping was checked with the real cl, clang-cl and clang.

### Always-on flags

| When | MSVC | clang-cl | clang |
|---|---|---|---|
| Always | `/nologo` `/utf-8` | `/nologo` `/utf-8` `--target=x86_64-pc-windows-msvc` | `--target=x86_64-pc-windows-msvc` |
| For C++ | `/Zc:__cplusplus` | `/Zc:__cplusplus` | — |
| Build / `check` | `/c` / `/Zs` | `/c` / `/Zs` | `-c` / `-fsyntax-only` |
| Language (per source) | `/TC` / `/TP` | `/TC` / `/TP` | `-x c` / `-x c++` |
| Object file (when building) | `/Fo<obj>` | `/Fo<obj>` | `-o <obj>` |

- The flags that find header dependencies (`/showIncludes` etc.) are decided with the build engine (M2) ([architecture.md](architecture.md)), and are not in this table.

### Compiling

| Setting | MSVC | clang-cl | clang |
|---|---|---|---|
| `edition` `c++17` / `c++20` / `c++23` | `/std:c++17` / `/std:c++20` / `/std:c++23` (cl 19.52 or later) | `/std:c++17` / `/std:c++20` / `/clang:-std=c++23` | `-std=c++17` / `-std=c++20` / `-std=c++23` |
| `edition` `c11` / `c17` / `c23` | `/std:c11` / `/std:c17` / `/std:clatest` (cl 19.37 or later) | `/std:c11` / `/std:c17` / `/clang:-std=c2x` | `-std=c11` / `-std=c17` / `-std=c2x` |
| `warnings` `default` / `all` / `pedantic` | None / `/W4` / `/W4 /permissive-` | None / `/W4` / `/W4 -Wpedantic` | None / `-Wall -Wextra` / `-Wall -Wextra -Wpedantic` |
| `warnings-as-errors = true` | `/WX` | `/WX` | `-Werror` |
| `defines`, test builds (`CUNO_TEST`) | `/D<define>` | `/D<define>` | `-D<define>` |
| Include paths (`src/`, the bundled header location, `include-dirs`, in that order) | `/I<dir>` | `/I<dir>` | `-I<dir>` |
| `runtime` `dynamic` / `static` | `/MD` / `/MT` | `/MD` / `/MT` | `-fms-runtime-lib=dll` / `-fms-runtime-lib=static` |
| `exceptions` (C++) `true` / `false` | `/EHsc` / `/we4530 /D_HAS_EXCEPTIONS=0` | `/EHsc` / `/D_HAS_EXCEPTIONS=0` | None / `-fno-exceptions -D_HAS_EXCEPTIONS=0` |
| `rtti = false` (C++) | `/GR- /we4541` | `/GR- -Werror=rtti` | `-fno-rtti` |
| `opt-level` `0` / `1` / `2` / `3` / `"s"` | `/Od` / `/O2` / `/O2` / `/O2` / `/O1` | Same as MSVC | `-O0` / `-O1` / `-O2` / `-O3` / `-Os` |
| `debug-info = true` | `/Z7` | `/Z7` | `-g` |
| `debug-assertions = false` | `/DNDEBUG` | `/DNDEBUG` | `-DNDEBUG` |
| `lto = true` | `/GL` | `-flto` | `-flto` |
| `[build.<compiler>] flags` | Last, as given | Last, as given | Last, as given |

### Linking

Linking goes through the compiler driver (so that each driver adds the C runtime and other libraries itself).

| Setting | MSVC | clang-cl | clang |
|---|---|---|---|
| Always | `/nologo` | `/nologo` | `--target=x86_64-pc-windows-msvc` |
| Linker | `link.exe` (arguments after `/link`) | `-fuse-ld=lld`, `lld-link` (arguments after `/link`) | `-fuse-ld=lld` |
| Output | `/Fe:<exe>` | `/Fe:<exe>` | `-o <exe>` |
| `links` | `<lib>.lib` | `<lib>.lib` | `-l<lib>` |
| `lib-dirs` | `/LIBPATH:<dir>` | `/LIBPATH:<dir>` | `-L<dir>` |
| `subsystem = "console"` | `/SUBSYSTEM:CONSOLE` | `/SUBSYSTEM:CONSOLE` | `-Wl,/subsystem:console` |
| `subsystem = "windows"` | `/SUBSYSTEM:WINDOWS /ENTRY:mainCRTStartup` | `/SUBSYSTEM:WINDOWS /ENTRY:mainCRTStartup` | `-Wl,/subsystem:windows -Wl,/entry:mainCRTStartup` |
| `debug-info = true` | `/DEBUG /INCREMENTAL:NO` | `/DEBUG` | `-g` |
| `lto = true` | `/LTCG` | `-flto`, `/opt:lldlto=<n>` | `-flto -Wl,/opt:lldlto=<n>` |
| `link-flags` | Last after `/link`, as given | Last after `/link`, as given | Last, each as `-Xlinker <flag>` |

- `<n>` is `opt-level` (`0` / `1` / `2` / `3`; `"s"` is `2`).

### Rationale

- `/utf-8`: prevents UTF-8 sources from being read as CP932 on Japanese Windows.
- `/Zc:__cplusplus`: without it, cl sets `__cplusplus` to `199711` even with `/std:c++20`.
- `--target=x86_64-pc-windows-msvc`: builds with the same ABI as MSVC, so libraries built with MSVC can be linked in ([toolchain.md](toolchain.md)).
- `/TC` / `/TP`, `-x c` / `-x c++`: compile in the package's language whatever the extension (the same way [dev-tools.md](dev-tools.md) treats `.h` as the package's language).
- `c++23` and `c23` name their standard on every compiler. "Latest" (`/std:c++latest`, or `/std:clatest` used for C++) is not used: on cl 19.52 and clang-cl 22, `/std:c++latest` is already the C++26 working draft (`__cplusplus` is `202400`), so the same `cuno.toml` would get a different standard than with clang, and different programs would build.
  - cl only warns about a `/std` it does not know and compiles with the old standard. So whether the standard is accepted is decided from cl's version, and it is an error when that is too old ([toolchain.md](toolchain.md)). `/std:c++23` needs cl 19.52 or later, the oldest version checked (no official document says which version accepts it; this is lowered if an older one is confirmed), and `/std:clatest` (the latest C, which includes C23) needs Visual Studio 2022 17.7 (cl 19.37) or later.
  - clang-cl ignores `/std:c++23` and `/std:c23`, so clang's switch is passed through `/clang:`. C23 is written `c2x`, which Clang 17 also accepts (`c23` is from Clang 18).
- `-fms-runtime-lib`: without it, the GNU-style `clang` uses the static C runtime, which contradicts the `runtime = "dynamic"` default. It matters only when compiling (the objects record which C runtime they use, and the linker follows them), so it is not passed when linking.
- `/SUBSYSTEM:CONSOLE`: without it, the linker silently links a program that has `WinMain` and no `main` as a GUI program, with no console. Stated, the missing `main` is a link error.
- `-Xlinker`: `-Wl,` splits its value at commas, which breaks linker flags that contain one, such as `/OPT:REF,ICF`.
- `/opt:lldlto=<n>`: with LTO, optimisation runs again at link time, and lld uses level `2` whatever `opt-level` says unless told.
- `/INCREMENTAL:NO`: MSVC's linker links incrementally by default under `/DEBUG` and writes a `.ilk` (lld does not). This keeps the output the same whatever the compiler.
- `_HAS_EXCEPTIONS=0`: when exceptions are disabled, the standard library uses its non-throwing implementation whatever the compiler.
- `/we4530`, `/we4541`, `-Werror=rtti`: with exceptions or RTTI disabled, clang makes `try` / `throw` and `typeid` / `dynamic_cast` errors, but cl and clang-cl only warn and build a program whose destructors do not run or that crashes at run time. These stop the build on every compiler. cl still cannot detect a `throw` without a `try`, with any flag.
- `pedantic` asks for strict conformance, so clang and clang-cl point out `__uuidof`, which the Windows SDK's `IID_PPV_ARGS` and the like use, as an extension (an error together with `warnings-as-errors`); cl does not. On cl, `/std:c++20` and later and the C standards already include `/permissive-`, so `pedantic` differs little from `all` there.
- `-fuse-ld=lld`: without it, clang-cl uses `link.exe` and refuses LTO.
- `debug-info` does not use `/Zi`, because PDB writes conflict in parallel builds and it cannot be cached by sccache. `clang -g` emits CodeView and links as with `/DEBUG`.
- `opt-level = 1` is `/O2` on MSVC, which has no equivalent level.
- The debug C runtime (`/MDd`) is not the default, because mixing it with prebuilt dependencies causes link errors (the same policy as Rust).

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
  - **Generation is independent of compilation**: cuno generates it from the build graph (the compile commands scheduled to run). Regardless of whether compilation succeeds, running any of `build` / `run` / `check` / `test` / `lint` updates it to the latest content. `fmt` does not generate it (because it works without a compiler; [dev-tools.md](dev-tools.md#target-files)). `cuno lint` works even if nothing has been built yet.
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
  4. `[build] target-dir` in `cuno.toml`
  5. User-level `<cuno home>/config.toml`
  6. Default value `target/`
- If the output directory is empty, or is the project root or a directory above it (outside a project, the current directory or a directory above it), it is an error (exit code 3), whichever layer sets it.
  - Rationale: `cuno clean` deletes the whole output directory. Allowing something like `target-dir = "."` would delete the project's sources with it.
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
