<!-- translated-from: docs/ja/specs/build.md @ blob 18e0657b0f866d76d0196a4c5e7e7c856f320689 -->

# Build

## Build engine

- **cuno has its own build engine**. It works standalone and does not require Ninja / CMake.
- A choice between it and Ninja is not adopted (behavioral drift between two paths, plus the testing and maintenance burden, is not worth it).
- **Build graph construction and execution are separated**, leaving room to add a Ninja backend in the future.

## Incremental and parallel builds

- Header dependencies are obtained from `-MD`-style dependency files for GCC / Clang, `/sourceDependencies` (JSON output; VS 16.7 or later) for MSVC. clang-cl uses clang's `-MD` too, through `/clang:`.
  - clang-cl does not use `/showIncludes` because a file read by `#embed` does not appear in its output, so a change to it would be missed (it does appear in a `-MD` dependency file).
  - MSVC does not use `/showIncludes` because on the Japanese edition of Visual Studio the output is in Japanese (「メモ: インクルード ファイル:」), making it language-dependent.
- Only what has changed is recompiled.
- Compilation runs in parallel.

### Deciding what changed

- For each step (compiling one source, linking one executable), cuno records, when it last succeeded:
  - the command line (the arguments, the compiler's location and version, and the environment variables `INCLUDE` / `LIB` / `LIBPATH` cuno builds; on GCC, also the location, size and modification time of each runtime file linked into every program (those of `crt2.o`, `libmingw32.a`, `libmingwex.a`, `libgcc.a`, `libgcc_eh.a`, `libstdc++.a`, `libwinpthread.a` and `libucrt.a` found in GCC's library directories), because MSYS2 updates them without changing GCC's version). The variables are included because when the Windows SDK or the MSVC toolset is updated, the arguments stay the same while the headers and libraries read change. The flags that make Clang and GCC color their output ([commands.md](commands.md#format)) are not part of it;
  - each input's modification time, size and content hash. The inputs of a compile are the source and every header it read (the header dependencies above); those of a link are the object files and the libraries of `links` found in `lib-dirs` (the first library file found, searching `lib-dirs` in order: `<lib>.lib` on MSVC and Clang; on GCC, the file found by searching each directory in the order its `ld` uses, `lib<lib>.dll.a`, `<lib>.dll.a`, `lib<lib>.a`, `<lib>.lib`, `lib<lib>.lib`, `lib<lib>.dll`, `<lib>.dll`; `-Wl,--verbose` shows this order). Those not found there (system libraries such as `kernel32`) are identified by `LIB` above (on GCC, where they ship with GCC, by the compiler's version and the runtime files above).
- The next build runs only the steps for which one of these holds:
  - there is no record, or the output is missing;
  - the command line differs from the record (a setting, the compiler or the profile changed);
  - an input's modification time or size differs from the record, and its content hash differs too;
  - a file with the same name as a header or library it read has newly appeared in another place that is searched (below).
- Same-named files appearing: when a file with the same name appears in a place the search reaches earlier, the next compile reads that one instead. Recording only the files read would miss this and report success with the old code. So for each step, cuno checks the following.
  - Where it looks: for a compile, `include-dirs`, the default include paths (below), and the directories holding the source and the headers it read (because `#include "..."` searches from the directory of the including file), but not the compiler's own directories (`INCLUDE` and where the compiler is installed). For a link, `lib-dirs`.
  - Which names: for each file read, its path relative to the directory of `include-dirs` or of the default include paths that holds it (`sub/cfg.h` for `#include "sub/cfg.h"`). For a header found in a directory of `INCLUDE`, its path relative to that directory. On GCC, GCC's own include directories ([toolchain.md](toolchain.md#mingw-gcc)) are used in place of `INCLUDE`. For a file outside all of these but where the compiler is installed (such as a header that ships with Clang), its file name. A header found only in the including file's directory is not checked: `#include "..."` searches there first, so no place is searched ahead of it. For a link, the library file names above for every name in `links`, including one not found in `lib-dirs` and read from the system.
  - If any combination of these places and names that did not exist when the record was made now exists, the step runs. One that already existed then (one later in the search order, which was not read) does not count as a change. But if one of them was modified two seconds or less before the step started, or later, the step is not recorded, as with an input (whether the compiler saw it before or after it was made cannot be told).
  - Rationale: the search order differs between compilers. Checking every place that may be searched, without reproducing the order, and looking only at what differs from the time of the record misses nothing on any compiler, and does not rerun a step every time because of an unrelated file of the same name. The only extra runs come when a file with the same name as a header or library read newly appears in a place later in the search order.
- An input whose modification time or size alone differs, with the same hash, is unchanged, and its record is updated.
- A step that fails is not recorded, so the next build always runs it.
- Nor is a step one of whose inputs was modified while it ran (whether the compiler read it before or after the change cannot be known). The next build runs it. An input modified 2 seconds or less before the step starts, or later, is also treated as modified while it ran, except an input cuno itself finished writing earlier in the same build, before the step started (such as the objects a link reads), which gets no such margin.
  - Rationale: file modification times are stamped by a coarser clock than the one that measures when a step starts, so a file saved just after the start can get a modification time before it (from a few to a dozen or so milliseconds on Windows; in 2-second units on FAT file systems). Without the margin, cuno would record a step whose compiler may have read the old content. The only cost of the margin is that a file saved just before a build is compiled once more by the next build. An input cuno itself has just made is finished before the step starts, so it cannot race a user's edit; giving it the margin would put a link's objects inside it every time, and every build would link again with nothing changed.
- For the same reason, a step one of whose inputs is dated after the moment the step finishes and is recorded (in the future) is not recorded either. Every build runs that step until that input's time has passed. Each build then prints `warning: <path> is dated in the future, so what reads it is built again on every build until then` once per file (`<path>` is relative to the project root, or absolute for a file outside it). When one step read several files dated in the future, each of them is named. A same-named file in a place the search could look ("Same-named files appearing" above) whose time is in the future is treated the same way, with the same warning. The exit code does not change.
  - Rationale: future times come from a clock that ran ahead (and was corrected later), archives or checkouts extracted with their times, network shares whose clock runs ahead, and the like. With a future time, the time cannot tell whether the input was edited while the step ran, so recording the step could report success with the old code. Running it again instead of recording it keeps the result correct, and the warning says why (fixing the time, with `touch` for example, stops it).
- The output of a successful step (warnings the compiler printed, for example) is also kept with its record, and a step that is up to date and does not run shows that output as is ([commands.md](commands.md#format)). Nothing is kept for a step that printed nothing.
- `cuno check` steps (checking one source) are judged the same way. A check produces no output, so when it succeeds cuno puts an empty file in place of the output. Like the records, this file is internal to cuno.
- Rationale:
  - Judging only whether an input is newer than its output misses a file put back with an older modification time (restored from a backup, for example), and reports success with the old code. Comparing with the recorded values catches a time that went backwards too.
  - Hashing everything on every build would read the standard and Windows headers (thousands of files) even when nothing changed. Inputs whose time and size match are not read.
  - What it misses is the following, and then it reports success with the old code:
    - a file whose content changed while its modification time and size stayed the same. The modification time stays the same only when, after the record, the file is rewritten to the same size within one tick of the modification time (see the rationale for the 2-second margin above), which never happens through a person's editing. It can happen when a tool or a test rewrites a file quickly;
    - a header that `__has_include` did not find, created later. No file was read, so there is no name to check;
    - a header read through a name that leaves a search directory with `..`, such as `#include "../common/x.h"`, when a same-named header is created where the search finds it first. The names checked are relative paths inside a search directory; a name with `..` is not checked.
    - a file added later to the compiler's own directories (`INCLUDE`, or the directory of the headers that ship with Clang). These are not among the places checked. An update of the toolset or the SDK changes the value of `INCLUDE`, and an update of Clang changes the compiler's version, so either shows up as a different command line.
- The records are kept inside the output directory (`target/<profile name>/`). Their format is cuno's own and not meant for users to read. A record from another cuno version, or one that cannot be read, is treated as absent (the step runs).

## Default include paths

Include paths that cuno always adds, separately from `include-dirs` in `cuno.toml`.

| Path | Reason |
|---|---|
| `src/` | So that headers of the package body can be referenced like `#include "calc.h"` |
| `include/` | After library support (Later). For public headers |
| Bundled header location (`<cuno home>/include/<cuno version>/`) | **Always added, even in normal builds**, so that writing `#include <cuno/test.h>` without wrapping it in `#ifdef CUNO_TEST` does not break the normal build |

## Settings and flags

How the settings in `cuno.toml`, and the flags cuno always adds, become each compiler's arguments. The flags are written only here; other documents link to this section. `clang` is the GNU-style Clang (targeting `x86_64-pc-windows-msvc`). `GCC` is MinGW-w64 GCC (targeting `x86_64-w64-mingw32`; [toolchain.md](toolchain.md#mingw-gcc)). Every mapping was checked with the real cl, clang-cl, clang and GCC.

### Always-on flags

| When | MSVC | clang-cl | clang | GCC |
|---|---|---|---|---|
| Always | `/nologo` `/utf-8` | `/nologo` `/utf-8` `--target=x86_64-pc-windows-msvc` | `--target=x86_64-pc-windows-msvc` | `-pipe` ([toolchain.md](toolchain.md#mingw-gcc)) |
| For C++ | `/Zc:__cplusplus` | `/Zc:__cplusplus` | — | — |
| Build / `check` | `/c` / `/c` (the same as the build; see below) | `/c` / `/Zs` | `-c` / `-fsyntax-only` | `-c` / `-c` (the same as the build; see below) |
| Language (per source) | `/TC` / `/TP` | `/TC` / `/TP` | `-x c` / `-x c++` | `-x c` / `-x c++` |
| Object file (when building) | `/Fo<obj>` | `/Fo<obj>` | `-o <obj>` | `-o <obj>` |
| Header dependencies (when building and for `check`) | `/sourceDependencies <file>` | `/clang:-MD /clang:-MF<file>` | `-MD -MF <file>` | `-MD -MF <file>` |

- The header dependency file `<file>` is the object file with its extension replaced by `.json` (MSVC) or `.d` (clang-cl, clang, GCC). `check` uses the same name (so that `check`, which produces no object file, also avoids rechecking unchanged sources by the same mechanism).
- MSVC's and GCC's `check` run the build's own compile (the same command, the same object file) without linking. The objects and records it produces are the build's own, so the build that follows only links.
  - Rationale: MSVC's `/Zs` does not emit the diagnostics raised during code generation. For example, C4716 for a function that returns no value, under the default settings, and C4700 and C4701 for uninitialized variables and C4715 for a function with a path that returns no value, under `warnings-as-errors`, fail a build but do not appear with `/Zs`. With `/Zs`, `check` would report success and the build would then fail. clang-cl's `/Zs` and clang's `-fsyntax-only` do emit them, so there `check` stays fast and does no code generation.
  - GCC is the same, for the same reason. Diagnostics such as out-of-bounds array accesses (`-Warray-bounds`) and buffer overflows (`-Wstringop-overflow`) are raised during optimization, so they do not appear with `-fsyntax-only`, and only the build with `-O2`, `-Wall` and `warnings-as-errors` fails.
  - Cost: MSVC's and GCC's `check` are slower than `/Zs` and `-fsyntax-only`, since they do code generation. But the objects it produces are not thrown away; the build uses them, so the build after a `check` does not repeat the same work.
- The flags of this table (the language, the source, the object file and the header dependencies) come after `[build.<compiler>] flags`, so that cuno's win over a user's `-MF` and the like: cuno reads the dependency file. When a dependency file holds several rules (a user's `-MP`, for example), only the first is read.

### Compiling

| Setting | MSVC | clang-cl | clang | GCC |
|---|---|---|---|---|
| `edition` `c++17` / `c++20` / `c++23` | `/std:c++17` / `/std:c++20` / `/std:c++23` (cl 19.52 or later) | `/std:c++17` / `/std:c++20` / `/clang:-std=c++23` | `-std=c++17` / `-std=c++20` / `-std=c++23` | `-std=c++17` / `-std=c++20` / `-std=c++23` (GCC 11 or later) |
| `edition` `c11` / `c17` / `c23` | `/std:c11` / `/std:c17` / `/std:clatest` (cl 19.37 or later) | `/std:c11` / `/std:c17` / `/clang:-std=c2x` | `-std=c11` / `-std=c17` / `-std=c2x` | `-std=c11` / `-std=c17` / `-std=c2x` |
| `warnings` `default` / `all` / `pedantic` | None / `/W4` / `/W4 /permissive-` | None / `/W4` / `/W4 -Wpedantic` | None / `-Wall -Wextra` / `-Wall -Wextra -Wpedantic` | None / `-Wall -Wextra` / `-Wall -Wextra -Wpedantic` |
| `warnings-as-errors = true` | `/WX` | `/WX` | `-Werror` | `-Werror` |
| `defines`, test builds (`CUNO_TEST`) | `/D<define>` | `/D<define>` | `-D<define>` | `-D<define>` |
| Include paths (`src/`, the bundled header location, `include-dirs`, in that order) | `/I<dir>` | `/I<dir>` | `-I<dir>` | `-I<dir>` |
| `runtime` `dynamic` / `static` | `/MD` / `/MT` | `/MD` / `/MT` | `-fms-runtime-lib=dll` / `-fms-runtime-lib=static` | None (given when linking; see below) |
| `exceptions` (C++) `true` / `false` | `/EHsc` / `/we4530 /D_HAS_EXCEPTIONS=0` | `/EHsc` / `/D_HAS_EXCEPTIONS=0` | None / `-fno-exceptions -D_HAS_EXCEPTIONS=0` | None / `-fno-exceptions` |
| `rtti = false` (C++) | `/GR- /we4541` | `/GR- -Werror=rtti` | `-fno-rtti` | `-fno-rtti` |
| `opt-level` `0` / `1` / `2` / `3` / `"s"` | `/Od` / `/O2` / `/O2` / `/O2` / `/O1` | Same as MSVC | `-O0` / `-O1` / `-O2` / `-O3` / `-Os` | `-O0` / `-O1` / `-O2` / `-O3` / `-Os` |
| `debug-info = true` | `/Z7` | `/Z7` | `-g` | `-g` |
| `debug-assertions = false` | `/DNDEBUG` | `/DNDEBUG` | `-DNDEBUG` | `-DNDEBUG` |
| `lto = true` | `/GL` | `-flto` | `-flto` | `-flto -ffat-lto-objects` |
| `[build.<compiler>] flags` | After the flags made from settings, as given | After the flags made from settings, as given | After the flags made from settings, as given | After the flags made from settings, as given |

### Linking

Linking goes through the compiler driver (so that each driver adds the C runtime and other libraries itself). GCC's driver is `g++` for a package with C++ sources and `gcc` for a C-only package (only `g++` adds the C++ standard library).

| Setting | MSVC | clang-cl | clang | GCC |
|---|---|---|---|---|
| Always | `/nologo` | `/nologo` | `--target=x86_64-pc-windows-msvc` | — |
| Linker | `link.exe` (arguments after `/link`) | `-fuse-ld=lld`, `lld-link` (arguments after `/link`) | `-fuse-ld=lld` | GCC's `ld` (not specified) |
| Output | `/Fe:<exe>` | `/Fe:<exe>` | `-o <exe>` | `-o <exe>` |
| `links` | `<lib>.lib` | `<lib>.lib` | `-l<lib>` | `-l<lib>` |
| `lib-dirs` | `/LIBPATH:<dir>` | `/LIBPATH:<dir>` | `-L<dir>` | `-L<dir>` |
| `subsystem = "console"` | `/SUBSYSTEM:CONSOLE` | `/SUBSYSTEM:CONSOLE` | `-Wl,/subsystem:console` | `-mconsole` |
| `subsystem = "windows"` | `/SUBSYSTEM:WINDOWS /ENTRY:mainCRTStartup` | `/SUBSYSTEM:WINDOWS /ENTRY:mainCRTStartup` | `-Wl,/subsystem:windows -Wl,/entry:mainCRTStartup` | `-mwindows` |
| `runtime` `dynamic` / `static` | None (given when compiling) | None (given when compiling) | None (given when compiling) | Both: for C, `-static-libgcc -Wl,-Bstatic -lwinpthread -Wl,-Bdynamic`; for C++, `-static-libgcc -static-libstdc++ -Wl,-Bstatic -lstdc++ -lwinpthread -Wl,-Bdynamic` |
| `debug-info = true` | `/DEBUG /INCREMENTAL:NO` | `/DEBUG` | `-g` | `-g` |
| `lto = true` | `/LTCG` | `-flto`, `/opt:lldlto=<n>` | `-flto -Wl,/opt:lldlto=<n>` | `-flto=auto` |
| `link-flags` | Last after `/link`, as given | Last after `/link`, as given | Last, each as `-Xlinker <flag>` | Last, each as `-Xlinker <flag>` |

- `<n>` is `opt-level` (`0` / `1` / `2` / `3`; `"s"` is `2`).

### Rationale

- `/utf-8`: prevents UTF-8 sources from being read as CP932 on Japanese Windows.
- `/Zc:__cplusplus`: without it, cl sets `__cplusplus` to `199711` even with `/std:c++20`.
- `--target=x86_64-pc-windows-msvc`: builds with the same ABI as MSVC, so libraries built with MSVC can be linked in ([toolchain.md](toolchain.md)).
- `/TC` / `/TP`, `-x c` / `-x c++`: each source is compiled as the language its extension gives ([project-layout.md](project-layout.md#target-files)), stated explicitly. A file in a language other than the package's gets that language's default standard, and none of the C++-only settings (exceptions, RTTI). Every other setting (`defines`, warnings, the `flags` of `[build]` and `[build.<compiler>]`, and so on) goes to every source whatever its language; a C++-only flag written in `flags` is ignored by the compiler on C sources, or makes it report a warning or an error.
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
- With MinGW GCC, whatever `runtime` says, GCC's runtime (`libgcc`, `libstdc++`) and the threading library (`winpthread`) are linked statically, and the C runtime (UCRT) stays dynamic. `runtime = "static"` is the same as `dynamic`.
  - Rationale: the DLLs of GCC's runtime and of `winpthread` (`libstdc++-6.dll`, `libwinpthread-1.dll`, etc.) do not ship with Windows, and if they were linked dynamically, the built executable would not start outside cuno. As with MSVC's `dynamic`, the result is an executable that still runs when distributed.
  - With `-static-libgcc -static-libstdc++` alone, `libstdc++` loads `libwinpthread-1.dll`, so `winpthread` is also named and made static. `-lstdc++` comes first because `winpthread` resolves functions that `libstdc++` uses.
  - Why `static` is the same as `dynamic`: MinGW cannot link UCRT statically (even with `-static`, the UCRT DLL is loaded). On top of that, `-static` makes the import libraries of the DLLs in `links` (`libfoo.dll.a`) impossible to link.
  - The libraries written in `links` are not among those made static. What `ld` finds in the order above is linked. For example, `links = ["pthread"]` finds `libpthread.dll.a`, and the built executable loads `libwinpthread-1.dll`.
- GCC's `subsystem = "windows"` is `-mwindows`. MinGW's startup code calls `main` even for a GUI program, so no entry point needs to be given.
- GCC does not get `_HAS_EXCEPTIONS=0`. It is a macro of MSVC's standard library; GCC's standard library (`libstdc++`) uses its non-throwing implementation with `-fno-exceptions` alone.
- GCC's LTO uses the optimisation level of the compile at link time, so `<n>` is not passed.
  - Why the compile gets `-ffat-lto-objects`: without it, the objects hold only the intermediate representation, and the diagnostics raised during optimisation (`-Warray-bounds` and the like) appear neither when compiling nor when linking. A build that should fail with `warnings-as-errors` would succeed. The cost is slower compiles in LTO builds.
  - `-flto=auto` at link time runs the optimisation in parallel. With `-flto` alone, GCC warns every time that it is not running in parallel.

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
  - **Self-contained content**: the MSVC / Windows SDK include paths and the MSVC version in use (`-fms-compatibility-version=<cl version>`) are written explicitly, so that clangd running outside a Developer Command Prompt can still find the standard headers and Windows headers, and analyze with the same `_MSC_VER` as the build.
    - The include paths are written so that they are searched after the tool's own built-in headers (`/imsvc` in cl style, `-idirafter` for clang), because in the build, too, `INCLUDE` is searched after the built-in headers. They are also written before the compile's arguments: in the build, `INCLUDE` is searched before a `-idirafter` in the user's `flags`, so writing them after would search the user's directory first and could analyze different headers from the build's. With `-isystem`, the Windows SDK's `stddef.h` and the like would be found before clang's own, and a program different from the build's would be analyzed.
    - The clang form also gets `-nostdlibinc`. clang searches the headers of the `INCLUDE` environment variable (or, when it is absent, of the Visual Studio it finds itself) before `-idirafter`, so without it a tool would analyze another toolset's standard headers, depending on the environment it runs in. `-nostdlibinc` keeps clang's own built-in headers and drops only the other standard locations. The cl form's `/imsvc` is searched before `INCLUDE`, so it needs none.
    - For GCC, GCC's own include directories ([toolchain.md](toolchain.md#mingw-gcc)) are written in place of the MSVC and Windows SDK include paths, in the same form (`-nostdlibinc` and `-idirafter`), and the target `--target=x86_64-w64-mingw32` and the GCC version (`-fgnuc-version=<GCC version>`) are written too.
      - Rationale: clang-based tools do not learn the target from the program name `gcc`, so they read the MinGW headers as the default `x86_64-pc-windows-msvc` and fail with errors in `stdio.h` and the like. With the target written, they read GCC's headers as they are (checked with clang-tidy 22 and GCC 16). The version is written, as `-fms-compatibility-version` is for MSVC, so that analysis uses the same `__GNUC__` as the build. Without it, clang claims to be GCC 4.2.1 and analyzes the inside of `#if __GNUC__ >= 10` differently from the build.
  - **Written in the form clang-based tools read**: the file is read by clangd and clang-tidy, which read cl commands as clang-cl too. So only the following three points are rewritten, to keep the meaning the same as in the build.
    - `/c` is written `-c` (cl accepts it too), because when only analyzing, clang-cl warns that `/c` is an unused argument, and under `warnings-as-errors` that warning becomes an error.
    - MSVC standard switches that clang-cl does not know (`/std:c++23`, `/std:clatest`) are rewritten to clang-cl's switch for the same standard (the clang-cl column of the [Compiling](#compiling) table). Left as they are, clang-cl would ignore them and analyze with an older standard.
    - MSVC's `/GL` (from `lto`) is not written. It is a link-time optimization switch with no bearing on analysis, and when only analyzing, clang-cl warns that it is an unused argument, which under `warnings-as-errors` becomes an error.
  - When the content is the same as last time, the file is not rewritten, so that an editor watching it does not reanalyze after every build.
  - **Content is always fixed**: it contains the commands for "the `dev` profile with `CUNO_TEST` defined". The content does not change regardless of which profile is built. Each compile's output (`output`) names where that test build's object is (see [Output directory](#output-directory)). The normal build's object is compiled without `CUNO_TEST`, so naming it would point at something else. Entry files are not part of a test build ([project-layout.md](project-layout.md)), but they are listed so that editors and clang-tidy can analyse them; their `output` is where the test build's layout would put them (no build makes anything there). Files under `tests/` are listed too. The bundled runtime (`cuno/test_main.c`) is not listed, because it is not the user's source.
    - Reason: editor completion and clang-tidy can analyze in-source test code as well (just as rust-analyzer analyzes test code). If the content changed with every build, the editor's analysis results would fluctuate.
  - The header dependency arguments ([Always-on flags](#always-on-flags)) are left out of each compile's command. They make the compiler write a file that cuno uses to decide what changed, and have no bearing on what the source means.
  - If the file cannot be written, cuno prints a warning (`warning: could not write compile_commands.json: <reason>`) and the build continues.

### Output directory

- The default is `target/`. Its contents are organized by target.
  - Normal: `target/<profile name>/`. However, the `dev` profile uses `debug/`, as in Cargo (`target/debug/`, `target/release/`; custom profiles keep their name, e.g. `target/profiling/`).
  - When cross-compiling: `target/<target>/<profile name>/`
  - Test builds: `target/<profile name>/test/<package name>/`. Kept separate so that objects built with `CUNO_TEST` defined do not mix with normal build objects. The package name is included for the same reason as for intermediate files. As with intermediate files, objects are placed by the source's position in the project (`target/debug/test/<package name>/src/calc.cpp.obj`).
    - Test executables go in `test/<package name>/bin/`: `<package name>` for in-source tests, and `tests-foo` for `tests/foo.cpp` (each with the platform's executable suffix; `.exe` on Windows).
    - Objects of the bundled runtime ([testing.md](testing.md)) go in `test/<package name>/cuno/`.
    - The files through which `cuno test` receives test results go in a directory made for each run under `test/`, and are removed when the run ends. This keeps two `cuno test` running at the same time from reading each other's results.
    - Reason: so that the executables do not overlap with where objects go (`src/`, `tests/`).
    - When two test executables would have the same name (`tests/foo.c` and `tests/foo.cpp`, or a package named `tests-foo` and `tests/foo.cpp`, etc.), it is an error (exit code 3) that names both files (`` error: two test executables are named `tests-foo`: src/ and tests/foo.cpp ``; in-source tests are shown as `src/`). Names are compared the same way as for duplicate executable names ([project-layout.md](project-layout.md)): case-insensitively on Windows.
      - Reason: building them under the same name would let one link overwrite the other, and that test would never run while the run is still reported as a success.
  - Intermediate files (objects, etc.): `target/<profile name>/obj/<package name>/`. Prevents collisions when multiple projects share the output directory.
  - Within the output directory, the names of the files and directories that cuno makes from the names of sources and tests (objects, test executables, etc.) have each character that is not ASCII written as `%` followed by two uppercase hexadecimal digits for each of its UTF-8 bytes (`日本語.c` becomes `%E6%97%A5%E6%9C%AC%E8%AA%9E.c.obj`). `%` itself is also written as `%25`. A name that cannot be read as Unicode (one holding an unpaired surrogate) has each UTF-16 unit that is not ASCII, and `%`, written as `%u` followed by four uppercase hexadecimal digits. This is the same for every compiler.
    - Rationale: MinGW's GCC assembler and linker cannot open paths that are not ASCII ([toolchain.md](toolchain.md#mingw-gcc)). Because `%` is rewritten too, two different names never become the same name.
- `cuno clean` (with no arguments) deletes the entire output directory, as in Cargo.
- A build (`build` / `run` / `check`) puts a marker file `.cuno-target` directly in the output directory when it creates the directory, or when it starts using an empty one. It does not put one in an existing directory that has contents. `cuno clean` does not delete an output directory without this marker.
  - Rationale: even when the output directory points inside the project, it may be a directory cuno did not create, such as `src` or `.git`. Naming rules cannot fully guard against this, so cuno deletes only what it knows it created itself.
- Users can change the output directory. Precedence:
  1. Command-line argument `--target-dir`
  2. Environment variable `CUNO_TARGET_DIR`
  3. Project-level `.cuno/config.toml`
  4. `[build] target-dir` in `cuno.toml`
  5. User-level `<cuno home>/config.toml`
  6. Default value `target/`
- If the output directory is empty, or is the project root or a directory above it (outside a project, the current directory or a directory above it; the build commands and `cuno clean` report the missing project first outside a project, so it is `cuno doctor` that reports this, [commands.md](commands.md#cuno-build--cuno-check)), it is an error (exit code 3). This holds whichever layer sets it (an empty `CUNO_TARGET_DIR` environment variable counts as unset, [manifest.md](manifest.md#two-levels-of-settings)), and the value of a layer that loses on precedence is checked too (even with `--target-dir`, `target-dir = "."` in `cuno.toml` is an error).
  - Rationale: if the result depended on how the path is written (`""`, `"."`, `"a/.."`) or on which layer wins, the error would first appear only when another layer's setting is removed.
  - Rationale: `cuno clean` deletes the whole output directory. Allowing something like `target-dir = "."` would delete the project's sources with it.
- A `target-dir` in a layer kept in the project (`cuno.toml`, `.cuno/config.toml`) must point inside the project. Pointing outside it is an error (exit code 3), even in a layer that does not win. The error names the file and line where it was written, with ``error: `build.target-dir` in .cuno/config.toml points outside the project: <path>`` and `help: set it in the user-level config, CUNO_TARGET_DIR or --target-dir to put the output outside the project`. To put the output outside the project, set it in the user-level config, the `CUNO_TARGET_DIR` environment variable or `--target-dir`.
  - Rationale: these files can be distributed with a repository. `cuno clean` deletes the output directory without running anything, so a cloned project must not be able to make it delete a directory outside the project. On builds trusting a project's settings, see [manifest.md](manifest.md#a-projects-settings-and-trust).
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

- [manifest.md](manifest.md) is the source of truth for the exact item names of each setting (the examples above are an overview)
- All items of custom rules
- The cases in which custom rules and build scripts report an error according to [design principle 9](../concept.md#9-allow-complex-configuration-and-never-be-silently-wrong) (no matching files, a declared output not produced, overriding a declaration, an input missed by the change check), and the evaluation order (written on the same page as [manifest.md "Settings layering"](manifest.md#settings-layering))
- Build script file name, Lua version (Lua 5.4 / LuaJIT / Luau, etc.), and API
