<!-- translated-from: docs/ja/guides/compilers.md @ blob 6e5e48d785e31ff650f3f0be55ecc4f64cddf398 -->

# Compiler guide

On Windows, cuno can use MSVC (cl), Clang (clang and clang-cl) and GCC (MinGW-w64). For each compiler, this page covers how to install it so that cuno can find it, how to tell cuno where it is, and how it differs from the other compilers.

Everything here is in the specification ([toolchain.md](../specs/toolchain.md), [build.md](../specs/build.md#settings-and-flags), [manifest.md](../specs/manifest.md)). This page decides nothing new; it only reads the specification again from each compiler's side.

## Which compiler is used

The compiler is decided in the following order; earlier entries take precedence ([toolchain.md](../specs/toolchain.md#selection)).

1. The command's `--compiler`
2. The environment variable `CUNO_COMPILER`
3. `[toolchain] compiler` in the project's `.cuno/config.toml`
4. `[toolchain] compiler` in `Cuno.toml`
5. `[toolchain] compiler` in the user-level `config.toml`
6. Automatic selection (MSVC on Windows)

```toml
# Cuno.toml
[toolchain]
compiler = "clang-cl"   # msvc / clang / clang-cl / gcc
```

- `cuno toolchain list` lists the compilers found, and marks the one a build where it is run would select ([commands.md](../specs/commands.md#cuno-toolchain-list)).

  ```
  msvc          19.52.37029  C:\Program Files\Microsoft Visual Studio\18\Community\VC\Tools\MSVC\14.52.37025\bin\Hostx64\x64\cl.exe  (selected: automatic selection)
  clang         22.1.8       C:\Program Files\LLVM\bin\clang.exe
  clang-cl      22.1.8       C:\Program Files\LLVM\bin\clang-cl.exe
  gcc           16.2.0       C:\msys64\ucrt64\bin\gcc.exe
  clang-format  22.1.8       C:\Program Files\LLVM\bin\clang-format.exe
  clang-tidy    22.1.8       C:\Program Files\LLVM\bin\clang-tidy.exe
  ```

- `cuno doctor --compiler <name>` diagnoses whether that compiler can build, and gives a fix for each problem ([commands.md](../specs/commands.md#cuno-doctor)). After installing a compiler, check with this first.
- `cuno toolchain default <name>` writes the default compiler for all of your projects into the user-level `config.toml` ([commands.md](../specs/commands.md#cuno-toolchain-default-compiler)). If a project selects a compiler in `Cuno.toml`, that one takes precedence.

## Specifying the location and version

### Location (`[toolchain.paths]`)

cuno looks for compilers in fixed locations, so usually you need to write nothing. To use one installed elsewhere, write its location in a per-machine settings file (the project's `.cuno/config.toml` or the user-level `config.toml`) ([manifest.md](../specs/manifest.md#toolchainpaths)). Do not write it in `Cuno.toml`, because locations differ from machine to machine.

```toml
# .cuno/config.toml
[toolchain.paths]
msvc = "C:/Program Files/Microsoft Visual Studio/18/Community"   # Visual Studio installation directory
clang = "C:/LLVM/bin/clang.exe"
clang-cl = "C:/LLVM/bin/clang-cl.exe"
gcc = "C:/msys64/ucrt64/bin/gcc.exe"
```

- For `msvc`, write the installation directory of Visual Studio (or Build Tools), not cl.exe, because cuno assembles the environment from there.
- Only what you write is used, even when a newer one exists elsewhere. If what you wrote cannot be used (the file does not exist, the version is too old, clang-cl is written for `clang`, and so on), it is an error that names that setting ([toolchain.md](../specs/toolchain.md#llvm-clang--clang-cl--clang-format--clang-tidy)).

### Version (`[toolchain.version]`)

Write the compiler versions the project needs in `Cuno.toml` ([manifest.md](../specs/manifest.md#toolchainversion)).

```toml
[toolchain.version]
msvc = ">=19.37"   # cl.exe version
clang = ">=18"
gcc = ">=13"
```

- Only the constraint for the compiler actually used is checked. Switching to another compiler with `--compiler` does not fail on an unrelated constraint.
- When several installations of the same compiler exist, the newest one that satisfies the constraint is selected. Previews and prereleases are not selected ([toolchain.md](../specs/toolchain.md#selection)).
- Write the MSVC version as the cl.exe version (`19.37` and so on), not the Visual Studio product version (`17.7` and so on).

### Minimum versions

| Compiler | Minimum version | Needed for `edition` |
|---|---|---|
| MSVC | cl.exe 19.29 (VS 2019 16.11) | `c++23` needs cl 19.52 or later, `c23` needs cl 19.37 or later |
| Clang | 17, and one that the MSVC standard library in use accepts | — |
| GCC | 10 | `c++23` needs GCC 11 or later |

If the version is too old, it is an error before the build, showing the version needed ([toolchain.md](../specs/toolchain.md#minimum-compiler-versions)).

## What is the same for every compiler

- **Environment variables that the compiler or linker reads by itself are not used.** `CL` / `_CL_` (cl and clang-cl), `LINK` / `_LINK_` (link.exe and lld-link), `CPATH` / `C_INCLUDE_PATH` / `CPLUS_INCLUDE_PATH` (Clang and GCC), `LIBRARY_PATH` / `GCC_EXEC_PREFIX` / `COMPILER_PATH` (GCC), and `DEPENDENCIES_OUTPUT` / `SUNPRO_DEPENDENCIES` (GCC) are removed from the environment of builds, the standard library check, `cuno doctor` and GCC detection. Build commands warn about each one that was set. This keeps flags that never appear on a command line from producing a build that differs from `Cuno.toml`. Write flags in `Cuno.toml` ([toolchain.md](../specs/toolchain.md#how-it-is-resolved)).

## MSVC (cl)

The compiler used on Windows when nothing is selected.

### Installing

- Install **Build Tools for Visual Studio** with the **"Desktop development with C++"** workload (https://visualstudio.microsoft.com/visual-cpp-build-tools/ ). Installing the same workload into Visual Studio itself works too.
- cuno finds Visual Studio with vswhere and assembles the environment (`INCLUDE`, `LIB`, `PATH` and so on) itself. There is no need to open a Developer Command Prompt, and `vcvars64.bat` is not run ([toolchain.md](../specs/toolchain.md#environment-resolution-on-windows)).
- To check: `cuno doctor --compiler msvc`.

### How it differs from the other compilers

- **`cuno check` compiles the same way as a build** (it only skips linking). cl's syntax check (`/Zs`) does not produce diagnostics that come from the code generation stage, such as a function that returns no value, so `check` would report success and the build would then fail. This makes `check` slower than with clang and clang-cl, but the objects it produces are used as they are by the build that follows ([build.md](../specs/build.md#always-on-flags)).
- **Even with `exceptions = false`, cl cannot detect a `throw` with no `try`.** cuno raises cl's warnings for exceptions being turned off to errors to stop `try` / `catch`, but a `throw` with no `try` is something no flag makes cl detect ([build.md](../specs/build.md#rationale)).
- **The difference between `warnings = "pedantic"` and `"all"` is small**, because with cl, the standard selection for C++20 and later and for C already includes `/permissive-`.
- **The linker is `link.exe`.** With `debug-info = true`, incremental linking is turned off (`/INCREMENTAL:NO`) so that, as with lld, no `.ilk` is made.

### What it cannot do

- cuno does not assemble the headers and libraries of the .NET Framework SDK (`metahost.h`, `mscoree.lib` and so on), nor the environment needed by C++/CLI (`/clr`) and C++/CX (`/ZW`) ([toolchain.md](../specs/toolchain.md#how-it-is-resolved)).

## Clang (clang / clang-cl)

Clang on Windows builds with the same ABI as MSVC (target `x86_64-pc-windows-msvc`), so it can be linked together with libraries built with MSVC. It can be used under two names.

| | Flag style | `[build.<compiler>]` key |
|---|---|---|
| `clang` | GCC style (`-O2`, `-Wall`) | `[build.clang]` |
| `clang-cl` | MSVC style (`/O2`, `/W4`) | `[build.clang-cl]` |

### Installing

- Install the **LLVM release for Windows** with the official installer (https://github.com/llvm/llvm-project/releases ). In the default location (`%ProgramFiles%\LLVM\bin`), it is found without adding it to `PATH`. The Clang bundled with Visual Studio (`VC\Tools\Llvm\x64\bin`) is found too ([toolchain.md](../specs/toolchain.md#llvm-clang--clang-cl--clang-format--clang-tidy)).
- **Visual Studio (or Build Tools) is needed too**, because Clang on Windows uses MSVC's headers and libraries and the Windows SDK. Install the same thing as for MSVC above.
- To check: `cuno doctor --compiler clang`, or `--compiler clang-cl`. If Clang can be used, the line `Accepted by the MSVC standard library` is shown for a C++ project and outside any project.
- A Clang whose default target is not MSVC (`*-windows-msvc`), such as a Clang for MinGW, is not used, because it cannot be linked together with MSVC.

### How it differs from the other compilers

- **Which Clang can be used is decided by the Visual Studio version.** The MSVC standard library rejects a Clang older than the version it requires with an error (STL1000), and that lower bound rises as Visual Studio is updated (for example, MSVC 14.52 requires Clang 22). cuno asks the standard library itself before the build, and stops if the Clang is not accepted ([toolchain.md](../specs/toolchain.md#minimum-compiler-versions)).

  ```
  error: clang 20.1.8 is not accepted by the MSVC standard library in use (STL1000)
  help: install Clang 22 or newer, or select another compiler
  ```

  After updating Visual Studio, you may have to update LLVM as well. For a C-only project, this check is not made.
- **Clang uses one MSVC toolset as well.** It is the one with the newest cl among the complete, non-preview Visual Studio toolsets. `[toolchain.version] msvc` is checked only when MSVC is selected, so it does not constrain the toolset Clang uses. Writing `[toolchain.paths] msvc` limits the Visual Studio that Clang uses to that one as well ([toolchain.md](../specs/toolchain.md#selection), [manifest.md](../specs/manifest.md#toolchainpaths)).
- **The linker is `lld-link`** (bundled with LLVM).
- **`warnings = "pedantic"` can flag Windows SDK headers.** It warns about `__uuidof`, used by `IID_PPV_ARGS` and others, as an extension, so combined with `warnings-as-errors` it becomes an error. cl does not flag it ([build.md](../specs/build.md#rationale)).
- **Mixing up `clang` and `clang-cl` in `[toolchain.paths]` is an error.** Even though they are the same executable, its behaviour changes with its name, so it would be given flags in the other style ([toolchain.md](../specs/toolchain.md#llvm-clang--clang-cl--clang-format--clang-tidy)).

## GCC (MinGW-w64)

cuno uses MSYS2's UCRT64 GCC (target `x86_64-w64-mingw32`). It does not use the MSVC toolset, so **it can be used without Visual Studio**.

### Installing

- Install **MSYS2** (https://www.msys2.org/ ), then install the UCRT64 GCC from an MSYS2 shell.

  ```sh
  pacman -S mingw-w64-ucrt-x86_64-gcc
  ```

- In the default location (`C:\msys64\ucrt64\bin`), it is found without adding it to `PATH` ([toolchain.md](../specs/toolchain.md#mingw-gcc)).
- To check: `cuno doctor --compiler gcc`.
- **A GCC whose C runtime is MSVCRT** (MSYS2's mingw64, older distributions) is not used. If only such a GCC exists, it is an error, with guidance to install the UCRT64 GCC.

### How it differs from the other compilers

- **GCC's runtime is always built into the executable.** GCC's runtime (`libgcc`, `libstdc++`) and the threading library (`winpthread`) are linked statically, and the C runtime (UCRT) is the one in Windows. Windows does not ship these DLLs, so without building them in, the executable would not start outside cuno. `runtime = "static"` and `"dynamic"` behave the same, because MinGW cannot link UCRT statically ([build.md](../specs/build.md#rationale)).
- **Libraries written in `links` are not made static.** cuno links whatever `ld` finds, so, for example, `links = ["pthread"]` links `libpthread.dll.a`, and the executable built needs `libwinpthread-1.dll`.
- **For C++ with GCC 14 or later, `libstdc++exp` is linked too.** MinGW's libstdc++ keeps the part of C++23's `std::print` / `std::println` that writes to the terminal in this library. It also makes `std::stacktrace` and others linkable.
- **`cuno check` compiles the same way as a build** (as with MSVC), because diagnostics such as out-of-bounds array accesses (`-Warray-bounds`) only come from the optimisation stage.
- **`lto = true` makes compiling slower**, because cuno adds `-ffat-lto-objects` so that diagnostics from the optimisation stage still appear with LTO.
- **`subsystem = "windows"` is `-mwindows`**, and no entry point needs to be specified.
- **`exceptions = false` is only `-fno-exceptions`.** With that alone, GCC's standard library uses its non-throwing implementation (the MSVC standard library's macro `_HAS_EXCEPTIONS` is not defined).
- **`g++` is taken from the same directory as the `gcc` in use.** There is no setting to give the location of `g++` separately ([toolchain.md](../specs/toolchain.md#mingw-gcc)).
- **GCC's DLLs are not added to the environment of the executables you build.** cuno adds GCC's directory to `PATH` only when it starts GCC, so that an executable that needs those DLLs does not work only inside cuno.

### Non-ASCII paths

MinGW's assembler (`as`) and linker (`ld`) run in the ANSI code page, so they cannot open files whose paths are not ASCII. cuno makes building possible even in a directory with a Japanese name, for example by passing paths inside the project as relative paths. When a non-ASCII path would still have to be passed, it is an error before compiling, showing that path ([toolchain.md](../specs/toolchain.md#mingw-gcc)).

- The output directory (`target-dir`) path is not ASCII, even after cuno makes it relative to the root when it is inside the project (outside the project, as it is): build into an ASCII directory (`--target-dir`, `CUNO_TARGET_DIR`, `[build] target-dir`).
- The path of a `lib-dirs` directory is not ASCII even after cuno makes it relative to the project root: put the library in an ASCII directory. Only when linking (anything other than `cuno check`).
- The executable name taken from a file name in `src/bin/` is not ASCII: rename it to an ASCII name, or write it in `[[bin]]` with an ASCII name. Only when linking a program (`cuno build`, `cuno run`).
- The temporary directory (`TMP`, or `TEMP` if that is not set) is not ASCII, has no short 8.3 name, and either LTO is used or the path cannot be represented in the ANSI code page: set `TMP` and `TEMP` to an ASCII directory. Only when linking.

Even when the temporary directory is not ASCII, cuno uses its short name if it has one. If it has no short name and the path can be represented in the ANSI code page (such as a Japanese user name on a Japanese edition of Windows), ordinary linking succeeds, so cuno does not stop. However, if the link command line is very long (around 30,000 characters), linking fails with an `ld` error about a garbled path. There is no reliable way to check for that in advance, so cuno does not stop in this case either.

## fmt and lint

`cuno fmt` and `cuno lint` use LLVM's clang-format and clang-tidy whichever compiler is selected ([dev-tools.md](../specs/dev-tools.md)). Even for a project built only with MSVC or GCC, install LLVM to use fmt and lint. The minimum version of clang-format and clang-tidy is 17 ([dev-tools.md](../specs/dev-tools.md#tool-versions)). In a C++ project with MSVC or Clang selected, the standard library check above applies to clang-tidy too ([toolchain.md](../specs/toolchain.md#minimum-compiler-versions)).
