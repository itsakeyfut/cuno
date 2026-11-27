<!-- translated-from: docs/ja/specs/toolchain.md @ blob 4a724393016c96ac702b93209a1b140092f15598 -->

# Toolchain

## Supported compilers

All three major families are supported.

| Family | Compilers |
|---|---|
| GCC family | GCC, MinGW-w64 |
| Clang family | Clang, clang-cl, Apple Clang, Emscripten, Android NDK, Intel oneAPI (icx) |
| MSVC family | cl.exe |

- Option syntax is largely shared between the GCC and Clang families, and shared between MSVC and clang-cl.
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
- GCC is added right after MVP, starting with MinGW-w64 on Windows (target `x86_64-w64-mingw32`). MinGW does not use the MSVC toolset and runs on its own runtime, so it works without Visual Studio.
  - Rationale: cuno itself stays on Windows and only the set of compilers grows. GCC on Linux is handled in M8, where cuno itself runs on Linux.

## Language standards

| | Minimum supported | `cuno new` default |
|---|---|---|
| C++ | C++17 | C++20 |
| C | C11 | C17 |

- If the compiler in use does not accept the standard specified in `edition` (e.g. `c++23` below cl 19.52, `c23` below cl 19.37, `c++23` below GCC 11), it is an error, and cuno indicates the required compiler version. cl only warns about a `/std` it does not know and ignores it, so whether it is accepted is decided from the compiler's version.
- The mapping from `edition` to each compiler's standard flag is in [build.md](build.md#settings-and-flags).

### Minimum compiler versions

A compiler must accept a C++20 specification.

| Compiler | Minimum version |
|---|---|
| MSVC | VS 2019 16.11 or later (cl.exe 19.29 or later) |
| Clang | 17 or later, and on Windows accepted by the MSVC standard library in use |
| GCC | 10 or later |

- Why Clang 17 or later: a floor common to every OS, as the oldest Clang cuno supports. 17 is the version the VS 2022 17.10 to 17.12 standard library (MSVC STL) requires, and older Clang is not covered. Also, `-fms-runtime-lib`, which specifies the C runtime kind for GNU-style `clang`, is only available in Clang 16 and later (without it, the default is the static runtime, which conflicts with the `runtime = "dynamic"` default). 17 is a floor, and on Windows the check below can raise it.
- **On Windows, the Clang must also be accepted by the MSVC standard library (MSVC STL).** The MSVC STL errors out (STL1000) on a Clang older than its own floor, and that floor rises as Visual Studio is updated. It cannot be expressed as a fixed number, so cuno asks the standard library itself.
  - Examples of the floor (MSVC STL's `yvals_core.h`): VS 2022 17.0 requires Clang 12, 17.7 requires 16, 17.10 requires 17, 17.13 requires 18, 17.14 requires 19, MSVC 14.51 requires 20, and 14.52 requires 22.
  - **How it is checked**: a file that includes only `<yvals_core.h>` is syntax-checked **as C++** under the same conditions as the build's compile command (the same Clang, the resolved Windows environment, the target `x86_64-pc-windows-msvc`, and every compile flag of the build except those naming inputs and outputs, including the language standard, the RTTI and exception settings, the include paths, the `defines`, and the user's `flags`): `/TP /Zs` for `clang-cl`, `-x c++ -fsyntax-only` for `clang`. The file is placed outside the project (in a temporary directory), and the compiler runs in the project root as the build's does (outside a project, the current directory), so that relative paths in `flags` mean what they mean to the build.
    - Why as C++: treated as C, the standard library stops with a different error (STL1003).
    - Why the build's flags are passed: the MSVC STL's verdict depends on flags and macros. It does not emit STL1000 when `_ALLOW_COMPILER_AND_STL_VERSION_MISMATCH` is defined (a pairing the user has deliberately allowed passes, as the real build does), and a project with `rtti = false` that defines `_HAS_STATIC_RTTI=0` stops with another error without `/GR-`. Passing only some of the flags would give a verdict the real build does not.
  - **Verdict**: if the exit code is not 0 and `STL1000:` appears on either stdout or stderr, the pairing cannot be used. The Clang version needed is taken from `<version>` in the standard library's message `expected Clang <version> or newer` and shown in the form written (it may be `22` or `17.0.0`). If it cannot be extracted, the standard library's message is shown in `help:` as is.
  - **When no verdict is possible**: if the probe fails for a reason other than STL1000 (headers not found, another STL error, etc.), or does not finish within 30 seconds, whether the pairing works cannot be decided. It is not treated as passing; the compiler's output is shown as an environment problem (with the exit codes below).
  - Rationale: reading the header's condition, or keeping a table of MSVC versions in cuno, could judge an unusable pairing as fine once a new Visual Studio changes how the header is written or which version it requires. The probe compile takes tens of milliseconds and uses the standard library's own answer.
- **The same check applies to clang-tidy.** clang-tidy analyzes with the commands in compile_commands.json and reads the MSVC standard headers, so an old clang-tidy fails on every file with STL1000.
  - Method: clang-tidy analyzes the same file with `--checks=-*,readability-braces-around-statements` (with no check enabled it stops without analyzing, so one harmless check is enabled) and, after `--`, the command compile_commands.json would write for that file. That is, the compile conditions of the `dev` profile with `CUNO_TEST` defined, plus the MSVC / Windows SDK include paths and the MSVC version (`-fms-compatibility-version`), rewritten into the form Clang-based tools read ([build.md](build.md#output)). The project's `.clang-tidy` is not read. clang-tidy reports STL1000 on stdout. The verdict is as above.
    - Unlike compile_commands.json, the command after `--` has no compiler program name. So a cl-style command (MSVC, clang-cl) is given `--driver-mode=cl` to be read as a clang-cl command, and when MSVC is selected the target `x86_64-pc-windows-msvc` is passed too (cl's conditions do not name a target).
    - Rationale: `cuno lint` runs clang-tidy with the commands in compile_commands.json. Checking under other conditions (for example the build's profile, no `CUNO_TEST`, or giving the include paths only through the `INCLUDE` environment variable) could let a clang-tidy that `cuno doctor` reported as fine fail in `cuno lint`, which analyzes with a different command.
- **Where it is checked**:

  | Command | What is checked | C-only project (`[package] lang = "c"`) |
  |---|---|---|
  | Build commands | The selected Clang | Not checked (the C++ standard library is not used) |
  | `cuno lint` | clang-tidy | Not checked |
  | `cuno doctor` (inside or outside a project) | The selected Clang and clang-tidy | Not checked |

  - When GCC is selected, none of these checks run, because GCC does not use the MSVC standard library.

  - A Clang that is not selected (Clang while MSVC is selected, etc.) is not checked.
- The minimum versions and the check above are one decision, used by both `cuno doctor` and the build commands. `cuno doctor` reports a "problem" when it is not met or cannot be decided (exit code 1); the build commands and `cuno lint` make it an error before doing anything (exit code 3).
- What is shown when a pairing is rejected:

  ```
  error: clang 20.1.8 is not accepted by the MSVC standard library in use (STL1000)
  help: install Clang 22 or newer, or select another compiler
  ```

  For clang-tidy: `error: clang-tidy 20.1.8 is not accepted by ...` and `help: install clang-tidy 22 or newer`.

- Inside cuno, in `[toolchain.version]` of `cuno.toml`, and in output messages, MSVC versions are expressed as the **cl.exe version** (e.g. `19.29`). The Visual Studio product version (e.g. `16.11`) is not used.

## Compiler abstraction

- Differences between compiler families (detection, translating `cuno.toml` settings into flags, obtaining header dependency information, parsing diagnostic output) are **confined behind a single interface**.
- The kinds of compiler are one enumeration (`CompilerKind`), and each kind's answers are written in `match`es that name every kind. Adding a kind makes every place that must answer for it a compile error. Since additions by other developers are expected, the steps are written below.
- `[build]` settings in `cuno.toml` (e.g. `warnings-as-errors = true`) are translated into each family's flags (`-Werror` / `/WX`).

### Adding a compiler

What adding GCC (MinGW-w64) needed, in order. The pull requests were three (and the fixes after them): steps 1 and 2, 3 to 6, and 7 to 9. Adding the kind makes the `match`es of steps 3 to 6 compile errors all at once, and tests run with the real compiler fail on CI unless CI installs it, so neither group can be split further.

1. **Write the spec first.** In this document, write where it is looked for and in what order, how its version is read, the conditions for choosing it, and its minimum version. Add a column to every table in [build.md](build.md#settings-and-flags), and check every flag against the real compiler. What the tables do not answer (how the runtime is chosen, how `check` works, LTO) is written with its reasons.
2. **Detection** (`cuno-toolchain`). Make a detection module for the compiler (GCC's is `gcc.rs`).
   - The search order: the `[toolchain.paths]` setting, `PATH`, then the distribution's standard places. The same file is listed once.
   - Read what choosing it needs, such as its version, whether it is a pre-release, its target and its runtime, from output that no language setting changes (GCC's `-dumpfullversion`, `-dumpmachine`). Run it in the environment the build gives it (GCC with its own directory added to `PATH` and `TOOL_OPTION_VARIABLES` removed), and in a working directory where it reads none of the project's files.
   - Look for it only when it is used: when it is chosen (`toolchain::ready` in `cuno-build`) and in `cuno doctor`. Builds with other compilers, `fmt` and `lint` do not start it.
   - The `[toolchain.paths]` setting is read through an accessor on `Settings` in `cuno-manifest` (GCC's is `Settings::gcc_path`). Check that `Tool` in `config.rs` has the tool, and add it to the settings table's test.
3. **The kind** (`compiler.rs` in `cuno-toolchain`). Add the kind to `CompilerKind` and to `CompilerKind::ALL`, and answer every `match` that no longer compiles. Add a family (`Family`), a syntax (`Syntax`) or a way of reporting header dependencies (`Dependencies`) if it needs a new one. The main answers:
   - Its name, its minimum version, whether it uses the MSVC toolset and the MSVC standard library, whether `check` compiles, and the driver it links with (`g++` for GCC's C++).
   - Its mapping to `CompilerName` in `cuno-manifest`, and where its `[toolchain.version]` lives (`Constraints::slot`).
   - Remove its name from the names `CompilerKind::parse` refuses as not supported yet.
4. **Selection** (`select.rs`). Add the detection results to `Found`, and write the conditions for choosing a copy in `copy_of`. A copy written in `[toolchain.paths]` is the only one used, and one that cannot be used is an error naming the setting. The `help:` for none found and none usable says how to install it (`error.rs`). For a compiler that uses no MSVC toolset, check that a missing Visual Studio does not stop its build (`toolchain::ready`) and that `cuno doctor` does not report it as a problem.
5. **Environment** (`prepare.rs`). Build the environment it runs in. Add the environment variables it reads for itself to `TOOL_OPTION_VARIABLES`, so they are left out. Give `Prepared` what identifies its commands for deciding what changed: its standard header directories (`Prepared::include_dirs`) and the runtime files it links into every program, so that a file of the same name appearing is not missed ([build.md](build.md#deciding-what-changed)).
6. **Flags** (`flags/`). Every row in `compile.rs` and `link.rs`, plus `library_files` (the file names its linker tries for a library, in the order it tries them), the color flags, the header dependency file, and the versions that accept each standard (`check_edition`). The command given to clang-based tools (`clang_tools.rs`) names the target and the version macro the build sees (`-fgnuc-version` for GCC; which version is told through which macro is answered in the per-family `match` of `ToolContext::of`). For a compiler that does not use the MSVC standard library, clang-tidy's standard library check (`toolchain::tidy_compile`) is not made.

7. **The test runtime** (`headers/cuno/` in `cuno-build`). Check that tests register with it (the `constructor` attribute, or MSVC's `.CRT$XCU`), that a normal build links without the runtime, that values such as a `long double` are shown right, and any `#pragma` it does not know.
8. **Tests.**
   - Add the compiler's row to each module's unit test tables.
   - Add it to `Compiler` in `tests/support/compilers.rs`, with a `CUNO_TEST_*` variable to point at it.
   - Add it to the tests run with the real compilers (`tests/compile_flags.rs`, `tests/examples.rs`, `tests/test_headers.rs`, `tests/lint.rs`), and take a package from `cuno new` through `lint` in a test of its own (GCC's is `tests/gcc.rs`).
   - Add its `[build.<compiler>]` to the `examples/cpp/per-compiler-flags` example.
9. **CI.** In the test job of `.github/workflows/ci.yml`, install it where users install it. The tests fail if it is not found.
10. **Documents.** Correct the lists of compiler names and what is "not supported yet" in [commands.md](commands.md) and [manifest.md](manifest.md).

## Environment resolution on Windows

- Users can use MSVC without opening a Developer Command Prompt.
- cuno automatically resolves the location of Visual Studio (or Build Tools) and the required environment (environment variables, etc.).
- **The same resolution also applies to Clang (`clang` / `clang-cl`)**, because Clang on Windows also uses the MSVC headers, libraries, and Windows SDK.
- If Visual Studio / Build Tools is not found, it is an error even if Clang is selected, and cuno shows the same guidance as `cuno doctor`.

### How it is resolved

- **cuno builds the environment from the toolset and Windows SDK directories.** `vcvars64.bat` is not run.
  - Rationale: running `vcvars64.bat` takes about 1.75 seconds each time, and its output is in the console code page (CP932 on Japanese Windows), which silently breaks Japanese paths unless decoded correctly. Building it takes around 10 milliseconds and has no encoding problem. The directory layout has not changed since Visual Studio 2017, and cuno's tests compare the result with `vcvars64.bat`'s, so a change of layout is caught on CI.
- Where the Windows SDK is: `KitsRoot10` under `HKLM\SOFTWARE\Microsoft\Windows Kits\Installed Roots` in the registry, then `%ProgramFiles(x86)%\Windows Kits\10`, in that order; the first that holds a usable version is used (the registry value can be stale, with no SDK left there). A location that is not an absolute path is not searched.
- Which Windows SDK version: the newest `Include\<version>` for which `um\windows.h`, `ucrt\stdio.h`, `Lib\<version>\um\x64` and `Lib\<version>\ucrt\x64` all exist (versions are compared as numbers, and a directory whose name is not numeric is not a version). Choosing a partly installed version would fail later with headers or libraries not found.
- The values built (`<MSVC>` is the MSVC toolset directory, `<SDK>` the SDK location, `<version>` the SDK version):

  | Variable | Value |
  |---|---|
  | `INCLUDE` | `<MSVC>\include`, Visual Studio's `VC\Auxiliary\VS\include` (if present), and `ucrt` / `um` / `shared` / `winrt` / `cppwinrt` under `<SDK>\Include\<version>\` |
  | `LIB` | `<MSVC>\lib\x64`, `<SDK>\Lib\<version>\ucrt\x64`, `<SDK>\Lib\<version>\um\x64` |
  | `LIBPATH` | `<MSVC>\lib\x64` |
  | `PATH` (prepended) | `<MSVC>\bin\Hostx64\x64`, `<SDK>\bin\<version>\x64` |

- **Environment variables compilers and linkers read for themselves are not passed on.** `CL` / `_CL_` (options of cl.exe and clang-cl), `LINK` / `_LINK_` (options of link.exe and lld-link), and `CPATH` / `C_INCLUDE_PATH` / `CPLUS_INCLUDE_PATH` (the include paths of Clang and GCC), `LIBRARY_PATH` / `GCC_EXEC_PREFIX` / `COMPILER_PATH` (where GCC looks for libraries and its own programs), and `DEPENDENCIES_OUTPUT` / `SUNPRO_DEPENDENCIES` (an additional header dependency file GCC writes) are removed from the environment of the build, the standard library check, `cuno doctor` and GCC's detection. The build commands warn about each one that was set (`warning: CL is set in the environment and is not used`, `help: write the flags in cuno.toml`).
  - Rationale: they add flags without appearing on the command line, so the build would run with flags other than `cuno.toml`'s and report success (with `_CL_=-DNDEBUG`, for example, `NDEBUG` is defined in the dev profile too), and `-v` would not show them. They are not an error so that a user who sets `CL=/MP` globally need not unset it every time they use cuno.

- **Developer Command Prompt variables are removed**: `VSINSTALLDIR`, `VCINSTALLDIR`, `VCToolsInstallDir`, `EXTERNAL_INCLUDE`, `WindowsSdkDir`, `WindowsSDKVersion`, `UniversalCRTSdkDir`, `UCRTVersion`. clang / clang-cl look for the toolset and linker through them, so running cuno from another Visual Studio's Developer Command Prompt would succeed with a toolset other than the one cuno chose.
- Of what `vcvars64.bat` sets, the following is not set (using it fails with an error):
  - The headers and libraries of the .NET Framework SDK (`NETFXSDK`), such as `metahost.h` and `mscoree.lib`.
  - The `LIBPATH` entries for C++/CLI (`/clr`) and C++/CX (`/ZW`).
- If no Windows SDK is found, it is an error (exit code 3) with guidance on installing the Windows SDK of Build Tools.

### Clang on Windows runs in MSVC-compatible mode

- For both `clang` / `clang-cl`, the target is `x86_64-pc-windows-msvc` (same ABI as MSVC). Libraries built with MSVC can be mixed and linked.
- MinGW-based Clang is out of scope.

### Linker

| Compiler | Linker | Reason |
|---|---|---|
| `msvc` | `link.exe` | MSVC standard |
| `clang` / `clang-cl` | `lld-link` (bundled with LLVM) | Faster than `link.exe` |

## Detection

- Searches PATH, Visual Studio installation locations (vswhere), LLVM's standard installation location, and environment variables.
- `cuno toolchain list` lists the detected compilers.
- **A tool cuno runs for detection and checking is stopped if it does not finish within 30 seconds** (vswhere, reading cl.exe's version, each tool's `--version`, the standard library probe compile). A tool cut off this way is stopped and treated as having failed to run (never as success).
  - Rationale: a tool stuck on an antivirus scan or a broken installation would otherwise stop cuno with it. These normally take under a second, so 30 seconds is long enough not to cut off a working tool on a machine whose first start is slow, and matches the probe compile's limit.

### Visual Studio / Build Tools

- **Found through vswhere only.** `%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe` is run with `-all -products * -prerelease -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -utf8 -format json`, which gives every installation that contains the C++ x64 tools.
  - Rationale: vswhere always comes with Visual Studio 2017 and later and with Build Tools, and is the one way that knows the installations correctly. `-utf8 -format json` makes the output readable regardless of the console code page (CP932 on Japanese Windows).
- An installation's MSVC toolsets are the directories `VC\Tools\MSVC\<version>` that contain `bin\Hostx64\x64\cl.exe`. The default toolset is the version written in `VC\Auxiliary\Build\Microsoft.VCToolsVersion.default.txt`.
- **The cl.exe version is read from the first `<n>.<n>.<n>` in what cl.exe prints when run with no arguments.** The digits are the same whatever the display language (including the Japanese edition of Visual Studio). The toolset directory name (e.g. `14.52.37025`) does not match the cl.exe version (e.g. `19.52.37029`), so it is not used.
  - The environment variables `CL` / `_CL_` are not passed to it then. cl.exe reads options from them, and a user's `CL=/nologo` would stop the banner from being printed.
  - A toolset whose cl.exe does not tell its version is left out as unusable, and detecting the other toolsets continues. If no usable toolset remains, it is an error.
- Preview installations and incomplete installations (those vswhere does not report as complete and launchable: an error in setup, or a reboot pending) are detected too (so `cuno doctor` can show them). Whether to choose one is decided by "Selection", which chooses neither. When vswhere does not report this information, the installation is treated as a preview and incomplete.
- If there is no installation with the C++ x64 tools, or no vswhere, it is an error (exit code 3) with guidance on installing the "Desktop development with C++" workload of Build Tools.

### LLVM (clang / clang-cl / clang-format / clang-tidy)

- Each of the four tools is looked for in the following order, and every one found is recorded.
  1. The path written in `[toolchain.paths]` ([manifest.md](manifest.md))
  2. Each directory on `PATH` (absolute ones only; an empty or relative entry would point at cuno's working directory, often the project, where a file named like the tool would then be used, so it is not searched)
  3. On Windows, `%ProgramFiles%\LLVM\bin` (the default location of the official LLVM installer)
  4. On Windows, `VC\Tools\Llvm\x64\bin` of each detected Visual Studio (the Clang bundled with Visual Studio)
- The same file found from several places counts as one.
- **The version is read from what the tool prints when run with `--version`**: the first `<n>.<n>.<n>` after `version` on the line that contains `version` (`clang version 22.1.8`; clang-tidy prints `LLVM version 22.1.8`). Numbers on later lines (an install path, etc.) are not read.
  - If characters follow the number directly (`19.1.0-rc1`, `23.0.0git`), it is recorded as a pre-release.
  - If there is a `Target:` line, its default target (such as `x86_64-pc-windows-msvc`) is recorded.
  - A tool that cannot be started, or exits with an error, is recorded as "failed to run", and no version is read. One with no version line is recorded as "no version". Both stay in the list (so that `cuno doctor` can show them).
- Which one is used when several are found is decided by "Selection". It chooses, among those that run, are not pre-releases and, on Windows, have an MSVC default target (`*-windows-msvc`), the newest that satisfies the version conditions (a Clang for MinGW cannot be linked together with MSVC; the list keeps this search order).
  - For clang-format and clang-tidy the default target does not matter (clang-tidy prints none).
- **A tool whose path is written in `[toolchain.paths]` is used from that file only.** Other copies found are not used, even if they are newer.
  - If the written path has no file, or the tool written does not meet the conditions above (it does not run, prints no version, is a pre-release, is a Clang not targeting MSVC, or does not satisfy the version conditions), it is an error (exit code 3; a problem in `cuno doctor`) naming the setting and the reason.
  - A written file that is a different LLVM tool than its key is likewise an error (e.g. a clang-cl path for `clang`). The tools are told apart as follows.
    - For clang-format the `--version` output contains `clang-format version`; for clang and clang-cl it contains `clang version`. One that contains neither but contains `LLVM (http://llvm.org` or `LLVM version` (with a vendor's name in front, such as `Ubuntu LLVM version`) is clang-tidy. `--version` is run once, and its output gives both the kind of tool and its version.
    - clang and clang-cl are the same executable whose behavior is decided by its file name, so `--version` cannot tell them apart. The written path is run with `/?`; if it succeeds and returns output starting with `OVERVIEW:`, it is clang-cl, otherwise it is clang.
    - Rationale: using clang-cl in place of clang (or the reverse) passes flags in the other form, so it either fails with an error that does not point at the setting, or compiles with flags that mean something else and reports success. The tools are told apart by actual behavior rather than by name so that a wrapper with another name (such as `ccache`) is not wrongly rejected. A tool found by detection is not checked, because it was searched for by name.
  - Rationale: silently using another tool in place of the one written would build or lint with a tool other than the one the user named and report success.
- A tool not being found is not itself an error in detection. Whether it is needed is decided by each command that uses it (`cuno doctor`, `cuno fmt` / `cuno lint`, the build commands).

### MinGW (GCC)

- GCC (`gcc` and `g++`) is looked for in the following order, and every one found is recorded.
  1. The path written in `[toolchain.paths] gcc` ([manifest.md](manifest.md))
  2. Each directory on `PATH` (absolute ones only, as for LLVM)
  3. On Windows, the standard MSYS2 locations (`C:\msys64\ucrt64\bin`, then `C:\msys64\mingw64\bin`)
  - Rationale: installing MSYS2 is enough to use it, without setting `PATH`. This is the same idea as searching the location of the official LLVM installer.
- The same file found from several places counts as one.
- For each GCC found, the following is recorded. Each is read from output that does not depend on the language setting.
  - Version: what `-dumpfullversion` prints (`14.2.0`).
  - Whether it is a pre-release: whether the first line of `--version` contains `experimental` or `prerelease`.
  - Target: what `-dumpmachine` prints (`x86_64-w64-mingw32`).
  - Whether the C runtime is UCRT: whether `_UCRT` is among the macros after MinGW-w64's header `_mingw.h` is read (what is printed by preprocessing an empty input with `-dM -E -include _mingw.h`). `_UCRT` is defined by `_mingw.h`, not by the compiler.
  - GCC is run in an environment with that GCC's directory added to the front of `PATH`. Only the environment of the GCC that cuno starts changes; the user's and the system's `PATH` do not.
    - Rationale: GCC's compiler proper (`cc1`) uses DLLs in the same directory. When that directory is not on `PATH`, preprocessing and compiling fail without printing anything (MSYS2 does not change `PATH` by default).
  - When these are read, GCC is run with that GCC's directory as its working directory.
    - Rationale: `-include` searches the working directory first. Run in the project's directory, a `_mingw.h` in the project would be read instead of GCC's, and the C runtime would be judged differently.
  - Detection, like the build, runs GCC in an environment without the variables compilers read for themselves ([Environment resolution on Windows](#environment-resolution-on-windows)).
    - Rationale: with `CPATH` or a similar variable pointing at another `_mingw.h`, a UCRT GCC would be judged MSVCRT, and the user told to install the GCC they already have. With `GCC_EXEC_PREFIX` or `COMPILER_PATH` set, detection would inspect a different `cc1` from the one the build runs.
  - A GCC whose preprocessing (`-dM -E`) fails is treated as one that cannot run, not as MSVCRT.
    - Rationale: a GCC that cannot preprocess cannot build either. Treating it as MSVCRT would tell a user who has a UCRT GCC installed to install a UCRT GCC.
  - Rationale: the first number on the first line of `--version` can be a number in the name the distributor gave (`2023.02.1` in `gcc (Buildroot 2023.02.1) 12.3.0`). The `gcc version` line of `-v` is translated according to the language setting.
- Which one is used when several are found is decided by "Selection", as for LLVM: among those that run, are not pre-releases, target `x86_64-w64-mingw32` and use the UCRT C runtime, the newest that satisfies the version conditions.
- A GCC whose C runtime is MSVCRT (MSYS2's mingw64, older distributions, etc.) is not used. If only such GCCs are found, it is an error (exit code 3) with guidance on installing a UCRT GCC (MSYS2's ucrt64).
  - Rationale: UCRT is the standard C runtime since Windows 10, and MSVC uses it too. An MSVCRT GCC and a UCRT GCC report the same target, so choosing the newest without telling them apart would make which runtime the executable is built against depend on whichever GCCs happen to be installed.
- GCC is looked for only when GCC is used (when choosing GCC, and `cuno doctor`). Building with MSVC or Clang, `cuno fmt` and `cuno lint` do not start GCC.
- `g++` is the one in the same directory as the `gcc` used. There is no separate setting for where `g++` is.
- Before building, cuno asks the chosen GCC for its include directories (between `#include <...> search starts here:` and `End of search list.` when an empty input is preprocessed with `-E -v -x c++`) and its library directories (the `libraries: =` line of `-print-search-dirs`). The language setting is `LC_ALL=C`, and GCC is run in that GCC's directory. If either cannot be read, it is an error (exit code 3).
  - Rationale: deciding what changed uses these as it uses MSVC's `INCLUDE` ([build.md](build.md#deciding-what-changed)). Without them, cuno would not notice when a `sys/stat.h` placed in the project starts being read instead of GCC's `sys/stat.h`, and would report success with the old code.
- When building (compiling and linking), too, GCC is run in an environment with its directory added to the front of `PATH` (for the same reason as above). The environment in which `cuno run` and the like run the built executable does not get it.
  - Rationale: with it added, the built executable would run inside cuno even if it needed GCC's DLLs, and that it does not start outside cuno would go unnoticed ([build.md](build.md#rationale)).
- **Paths that are not ASCII.** In MSYS2's GCC, `gcc` and the compiler proper (`cc1`) work in UTF-8, while the assembler (`as`) and the linker (`ld`) work in the ANSI code page (CP932 on Japanese Windows). `as` and `ld` read the UTF-8 paths that `gcc` passes them as different characters, so they cannot open files whose paths are not ASCII (a directory with a Japanese name, the temporary directory of a user with a Japanese user name). cuno handles this as follows.
  - Compiling gets `-pipe`. Without it, GCC writes the assembly to a file in the temporary directory and passes that file to `as`, so when the temporary directory is not ASCII (as happens with a Japanese user name), every compile fails.
  - Of the paths passed to GCC, those that `as` and `ld` open (objects, executables, `lib-dirs`) are written relative to the project root when they are inside the project (GCC is run in the root). This is so that a project whose directory name is not ASCII can still be built.
  - File names inside the output directory have their characters that are not ASCII rewritten ([build.md](build.md#output-directory)). This is so that sources and tests whose names are not ASCII can still be built.
  - If a path that is not ASCII would still have to be passed to `as` or `ld`, it is an error (exit code 3) before compiling, naming that path.
    - The path of the output directory is not ASCII, written relative to the root when it is inside the project, or as it is when outside: `error: the target directory <path> is not ASCII, which MinGW's assembler and linker cannot open`, `help: build into a directory whose path is ASCII (--target-dir, CUNO_TARGET_DIR or [build] target-dir)`.
    - The path of a `lib-dirs` directory is not ASCII even when written relative to the project root: `` error: `lib-dirs` names <path>, which is not ASCII, which MinGW's linker cannot open ``, `help: move the libraries to a directory whose path is ASCII`. Only when linking (not for `cuno check`).
    - The name of an executable, taken from a file name under `src/bin/`, is not ASCII: `error: <path> names an executable that is not ASCII, which MinGW's linker cannot write`, `help: rename the file to an ASCII name, or list it in [[bin]] with an ASCII name`. `<path>` is that file. Only when linking the program (`cuno build`, `cuno run`). The executable's name is the user's to use, so it is not rewritten.
    - When linking, the temporary directory (below) is not ASCII, has no ASCII short name, and either LTO is used or its path cannot be written in the ANSI code page: `error: the temporary directory <path> is not ASCII and has no short name, which GCC's linker cannot use`, `help: set TMP and TEMP to a directory whose path is ASCII`.
  - **The temporary directory.** The temporary directory GCC uses is, as Windows decides it, `TMP`, else `TEMP`, else `USERPROFILE`. GCC's link creates files there (`collect2` runs in the ANSI code page), and an LTO link or a long link hands such a file to `ld`. So when the temporary directory's path is not ASCII, cuno starts GCC with its short name (8.3 form, when ASCII) as `TMP` and `TEMP`. Short names exist when the volume is set to create them (usually so on the system drive).
    - Without a short name, a path the ANSI code page can hold (a Japanese name on Japanese Windows) is left as it is, since a plain link works. With a very long link command line (around 30,000 characters), though, GCC writes the arguments to a file in that temporary directory and hands it to `ld`, which then fails with `ld`'s error about a garbled path. There is no reliable way to tell beforehand, so cuno does not refuse it.
  - Rationale: failing silently would show only an error from GCC's assembler with a garbled path. When a source's name is not ASCII, `as` writes the object under a garbled name, and the object cuno recorded never appears, so cuno compiles and links again every time even when nothing has changed.
- GCC does not use the MSVC toolset, so choosing GCC does not need Visual Studio. Nor is the MSVC environment (`INCLUDE`, `LIB`, `LIBPATH`) built.
- When `[toolchain.paths] gcc` is written, only that file is used, as for LLVM, and if it does not meet the conditions it is an error (exit code 3).

## Selection

Precedence (higher wins):

1. Command-line argument `--compiler`
2. Environment variable `CUNO_COMPILER`
3. Project-level `.cuno/config.toml`
4. `[toolchain]` in `cuno.toml`
5. User-level `<cuno home>/config.toml`
6. Automatic selection

- Settings decided by the project (`cuno.toml`) take precedence over personal global settings (user-level config).
- Outside a project (for example when `cuno doctor` runs outside one), there are no project layers (`.cuno/config.toml` and `[toolchain]` in `cuno.toml`), so the compiler is chosen in the order `--compiler` → `CUNO_COMPILER` → user-level config → automatic selection.
  - Rationale: to match the compiler a `cuno build` in a new project would use. Looking only at automatic selection could diagnose MSVC for a user whose user-level config selects clang, answer "no problem", and leave the following build to fail.
- If a compiler not supported yet (`safec`) is selected, it is an error (exit code 3). cuno does not warn and build with another compiler (to prevent silently building with a different compiler).

Automatic selection defaults: MSVC on Windows, GCC on Linux, Apple Clang on macOS.

- On Windows, if Visual Studio / Build Tools is not found, it is an error (Clang also requires Visual Studio, so switching to Clang does not solve it).
- **When multiple versions of the same compiler are installed**, the newest one that satisfies the version constraint is selected. Preview versions are not selected.
- **When clang / clang-cl is selected on Windows, one MSVC toolset is still used** (for the headers, libraries and the resolved environment): the toolset with the newest cl.exe among the complete, non-preview Visual Studio installations. The MSVC version constraint is checked only when MSVC is selected ([manifest.md](manifest.md)).

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

- List of supported targets, and how to detect each target's tools
