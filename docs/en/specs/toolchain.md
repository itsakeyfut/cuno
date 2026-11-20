<!-- translated-from: docs/ja/specs/toolchain.md @ blob 26b6b7d8d6addfd992ec774b802c9ee015147a6d -->

# Toolchain

## Supported compilers

All three major families are supported.

| Family | Compilers |
|---|---|
| GCC family | GCC, MinGW-w64 |
| Clang family | Clang, clang-cl, Apple Clang, Emscripten, Android NDK, Intel oneAPI (icx) |
| MSVC family | cl.exe |

- Option syntax is largely shared between the GCC and Clang families, and between MSVC and clang-cl.
- NVCC (CUDA) is deferred.
- Console SDKs (PS5 / Switch, etc.) are not officially supported due to NDAs, but the design allows supporting them via extensions.

### safec

- **safec** (https://github.com/itsakeyfut/safec), an experimental compiler developed separately by the cuno developer, is also one of the compiler choices alongside Clang / MSVC / GCC (`compiler = "safec"`).
  - safec: a compiler that redesigns C/C++ with its own Safety IR, experimenting with how much of Rust's safety (memory safety, thread safety, lifetimes, ownership, etc.) can be brought to C/C++.
  - Because the developer controls safec's design, deeper integration than with other compilers (structured diagnostic output, etc.) is also possible.
- As with other compilers, it is supported by implementing the compiler abstraction interface.
- Timing: after cuno is established on the major compilers.
- **On hold: safec's output format**. Currently safec cannot link with libraries built by other compilers. The output format will be decided based on the state of safec when work on safec support begins. The compiler abstraction leaves room to support either of:
  - Emitting native object files directly
  - A two-stage approach that translates to C, etc., and has an existing compiler compile it afterward

### MVP targets

- OS: Windows 11
- Compilers: **MSVC** and **Clang (including clang-cl)**
- GCC is added right after MVP.

## Language standards

| | Minimum supported | `cuno new` default |
|---|---|---|
| C++ | C++17 | C++20 |
| C | C11 | C17 |

- If the compiler in use does not accept the standard specified in `edition` (e.g. `c++23` on an old MSVC), it is an error, and cuno indicates the required compiler version.
- The mapping table from `edition` to each compiler's standard flag (`/std:c++20`, `-std=c++20`, etc.) will be created during implementation and written back into this spec.

### Minimum compiler versions

A compiler must accept a C++20 specification.

| Compiler | Minimum version |
|---|---|
| MSVC | VS 2019 16.11 or later (cl.exe 19.29 or later) |
| Clang | 17 or later |
| GCC | 10 or later |

- Why Clang 17 or later: the VS 2022 standard library (MSVC STL) errors out (STL1000) on Clang older than 17. Also, `-fms-runtime-lib`, which specifies the C runtime kind for GNU-style `clang`, is only available in Clang 16 and later (without it, the default is the static runtime, which conflicts with the `runtime = "dynamic"` default).
- `cuno doctor` reports a "problem" if the selected compiler is below the minimum version.

- Inside cuno, in `[toolchain.version]` of `cuno.toml`, and in output messages, MSVC versions are expressed as the **cl.exe version** (e.g. `19.29`). The Visual Studio product version (e.g. `16.11`) is not used.

## Compiler abstraction

- Differences between compiler families (detection, translating `cuno.toml` settings into flags, obtaining header dependency information, parsing diagnostic output) are **confined behind a single interface**.
- A new compiler can be supported just by adding one implementation of that interface. Since additions by other developers (GCC, etc.) are expected, the procedure for adding one will be documented.
- `[build]` settings in `cuno.toml` (e.g. `warnings-as-errors = true`) are translated into each family's flags (`-Werror` / `/WX`).

## Environment resolution on Windows

- Users can use MSVC without opening a Developer Command Prompt.
- cuno automatically resolves the location of Visual Studio (or Build Tools) and the required environment (environment variables, etc.).
- **The same resolution also applies to Clang (`clang` / `clang-cl`)**, because Clang on Windows also uses the MSVC headers, libraries, and Windows SDK.
- If Visual Studio / Build Tools is not found, it is an error even if Clang is selected, and cuno shows the same guidance as `cuno doctor`.

### Clang on Windows runs in MSVC-compatible mode

- For both `clang` / `clang-cl`, the target is `x86_64-pc-windows-msvc` (same ABI as MSVC). Libraries built with MSVC can be mixed and linked.
- MinGW-based Clang is out of scope (handled when GCC / MinGW is supported).

### Linker

| Compiler | Linker | Reason |
|---|---|---|
| `msvc` | `link.exe` | MSVC standard |
| `clang` / `clang-cl` | `lld-link` (bundled with LLVM) | Faster than `link.exe` |

## Detection

- Searches PATH, Visual Studio installation locations (vswhere), LLVM's standard installation location, and environment variables.
- `cuno toolchain list` lists the detected compilers.

## Selection

Precedence (higher wins):

1. Command-line argument `--compiler`
2. Environment variable `CUNO_COMPILER`
3. Project-level `.cuno/config.toml`
4. `[toolchain]` in `cuno.toml`
5. User-level `<cuno home>/config.toml`
6. Automatic selection

- Settings decided by the project (`cuno.toml`) take precedence over personal global settings (user-level config).
- If a compiler not supported in MVP (`gcc` / `safec`) is selected, it is an error (exit code 3). cuno does not warn and build with another compiler (to prevent silently building with a different compiler).

Automatic selection defaults: MSVC on Windows, GCC on Linux, Apple Clang on macOS.

- On Windows, if Visual Studio / Build Tools is not found, it is an error (Clang also requires Visual Studio, so switching to Clang does not solve it).
- **When multiple versions of the same compiler are installed**, the newest one that satisfies the version constraint is selected. Preview versions are not selected.

## Version pinning

- Write a per-compiler range or exact version in `[toolchain.version]` of `cuno.toml` ([manifest.md](manifest.md)).
- Only the constraint for the compiler actually used is checked.
- If it is not satisfied, it is an error, and cuno indicates how to resolve it.
- No separate file like rust-toolchain.toml is created (policy of not adding configuration files).

## Automatic installation (Future)

| Compiler | Method |
|---|---|
| LLVM / Clang | Download the official distribution and place it under cuno's management |
| MinGW (GCC) | Same as above |
| MSVC | Cannot be redistributed by cuno due to licensing. Launch the official Build Tools installer with the required configuration |

## Cross-compilation (Later)

- MVP only builds for the same environment as the host. The `--target` option is reserved only ([commands.md](commands.md)).
- Specified with `--target <target>`. Names follow Rust / LLVM notation (e.g. `x86_64-pc-windows-msvc`, `aarch64-linux-android`, `wasm32-unknown-emscripten`).
- Locations of each environment's tools (Android NDK, Emscripten, etc.) are written in `.cuno/config.toml` or auto-detected.
- Build scripts are Lua, so no compiler for the host is needed.

## Open questions

- Concrete method for resolving the MSVC environment (use of vswhere, etc.)
- Detailed search order for detection
- List of supported targets, and how to detect each target's tools
