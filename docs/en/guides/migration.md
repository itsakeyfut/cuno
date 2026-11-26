<!-- translated-from: docs/ja/guides/migration.md @ blob ba32cc3406bed4ee988703115a71c6c531bf0a7b -->

# Migrating from other tools

When you move to cuno from CMake, Visual Studio, ctest, clang-format and the like, the tables below show, tool by tool, how to do in cuno what you used to do before. Migrations often involve several tools at once, so everything is on one page so that you can read the relevant sections in sequence.

## How to read this page

The right-hand column of each table is one of the following three kinds.

- **Available now**: gives the cuno command, option or `cuno.toml` entry as is.
- **Not yet supported (Mn)**: something cuno does not have yet. Says what to do in the meantime and the milestone in which it is planned ([roadmap](../roadmap.md)).
- **No equivalent**: a concept cuno does not have. Says what to do in cuno instead.

cuno's configuration goes into a single `cuno.toml`. See [manifest.md](../specs/manifest.md) for details of the entries and [commands.md](../specs/commands.md) for details of the commands.

## From CMake

| In CMake | In cuno |
|---|---|
| Write `CMakeLists.txt` | Write `cuno.toml`. `cuno new <path>` creates a package that builds with no configuration |
| Add `CMakeLists.txt` to an existing directory | Not yet supported (M5, `cuno init`). In the meantime, place `cuno.toml` by hand |
| `cmake -S . -B build` → `cmake --build build` | `cuno build`. There is no generate step |
| `add_executable(app src/main.cpp)` | Write nothing. `src/main.cpp` (or `.c`) becomes the default executable. To change its name or path, use `[[bin]]` |
| Build several executables | Put sources in `src/bin/` (one per file), or write several `[[bin]]` |
| `add_library(...)` | Not yet supported (M6, `[lib]`). In the meantime, put the sources in `src/` as sources of a single package |
| List sources with `file(GLOB ...)` or an explicit list | Write nothing. Every source under `src/` is compiled |
| `target_include_directories` | `include-dirs` in `[build]` |
| `target_compile_definitions` | `defines` in `[build]` |
| `target_compile_options` | Warning level and similar go in `[build]` entries (`warnings`, `warnings-as-errors`, etc.). Raw flags go in `flags` per compiler, such as `[build.msvc]` |
| `target_link_libraries(app d3d12)` (system libraries) | `links` in `[build]`. Search paths go in `lib-dirs` |
| `PUBLIC` / `PRIVATE` / `INTERFACE` of `target_link_libraries` | No equivalent. Currently a package's settings apply to that whole package. A library's public headers are handled by `public-headers` in `[lib]` in M6 |
| `set(CMAKE_CXX_STANDARD 20)` | `edition` in `[package]` (`c++20`, etc.) |
| `-DCMAKE_BUILD_TYPE=Release` | `cuno build --release`. Settings go in `[profile.release]` |
| Generator expressions such as `$<CONFIG:Debug>` | No equivalent. Write per-configuration settings in `[profile.dev.build]` / `[profile.release.build]`, per-compiler settings in tables such as `[build.msvc]`, and per-OS settings in tables such as `[platform.windows.build]` |
| `if(WIN32)` / `if(MSVC)` | `[platform.windows.build]`, `[build.msvc]` |
| `option()` and cache variables | No equivalent. Write values in `cuno.toml`. Per-machine settings go in `.cuno/config.toml` |
| `CMakePresets.json` | No equivalent. `.cuno/config.toml`, and options such as `--release` and `--compiler` |
| `-G "Visual Studio 17 2022"` / `-T ClangCL` | `compiler` in `[toolchain]`, or `cuno build --compiler clang-cl` |
| `find_package(Foo)` | Not yet supported (M6 / M7, `cuno add`). In the meantime, specify them by hand with `include-dirs`, `links` and `lib-dirs` |
| `FetchContent` / `ExternalProject` | Not yet supported (Git / URL dependencies in M6; in M7, recipes invoke an existing CMake build behind the scenes) |
| `add_custom_command` / `add_custom_target` | Not yet supported (M7, `[[build.rule]]`) |
| `ctest` / `add_test` | See "From ctest and test frameworks" |
| `install()` / `export()` | Not yet supported (M10, `cuno install`) |
| `CMAKE_EXPORT_COMPILE_COMMANDS=ON` | Nothing to do. `compile_commands.json` is written to the project root on every build |
| `cmake --build build --verbose` | `cuno build -v` (shows the compiler commands that were run) |
| `rm -rf build` | `cuno clean` |

## From Visual Studio projects (`.sln` / `.vcxproj`)

| In Visual Studio | In cuno |
|---|---|
| Build the solution (Ctrl+Shift+B) | `cuno build`. No Developer Command Prompt is needed: cuno finds Visual Studio and sets up the environment |
| Switch between the Debug / Release configurations | The default is dev (equivalent to Debug). `cuno build --release` |
| Switch platforms (x64, etc.) | Currently x64. Other targets are not yet supported (M8, `--target`) |
| Platform Toolset (v143, ClangCL) | `compiler` in `[toolchain]` (`msvc` / `clang-cl` / `clang`). The minimum version goes in `[toolchain.version]` |
| "Additional Include Directories" in the property pages | `include-dirs` in `[build]` |
| "Preprocessor Definitions" | `defines` in `[build]`. Per configuration: `[profile.dev.build]` / `[profile.release.build]` |
| "Warning Level", "Treat Warnings As Errors" | `warnings`, `warnings-as-errors` in `[build]` |
| "Runtime Library" (/MD, /MT) | `runtime` in `[build]` (`dynamic` / `static`) |
| "Additional Dependencies" (`d3d12.lib`) | `links` in `[build]` (`d3d12`, without `.lib`) |
| "Additional Options" (compiler) | `flags` in `[build.msvc]` |
| "SubSystem" (Console / Windows) | `subsystem` in `[[bin]]` (`console` / `windows`) |
| Share settings through property sheets (`.props`) | No equivalent. Write settings in `cuno.toml`, and per-machine settings in `.cuno/config.toml` |
| Debug with F5 | Not yet supported (M9, `cuno ide vs`). In the meantime, you can open the executable from `cuno build` in Visual Studio and debug it |
| Run (Ctrl+F5) | `cuno run`. The working directory is the project root |
| "Command Arguments" | `cuno run -- <args>` |
| Choose the Startup Project | `cuno run --bin <name>` |
| Put several projects in one solution | Not yet supported (M9, workspaces) |
| Test Explorer | `cuno test`. See "From ctest and test frameworks" |
| Investigate when the environment is incomplete | `cuno doctor` |

## From Ninja / Make

| In Ninja / Make | In cuno |
|---|---|
| `ninja` / `make` | `cuno build`. cuno builds by itself, so neither Ninja nor Make is needed |
| `make -j8` | `cuno build -j 8` (the default is the number of CPU cores) |
| Rebuild only the files that changed | Nothing to do. cuno tracks changed sources and the headers they include |
| `make clean` | `cuno clean` |
| Custom targets such as `make run` | `cuno run`. Custom steps are not yet supported (M7, `[[build.rule]]`) |
| `CFLAGS` / `CXXFLAGS` | Entries in `[build]`. Raw flags go in `flags` in `[build.msvc]` and the like |
| Handle dependency files (`-MMD`, `.d`) yourself | Nothing to do |

## From ctest and test frameworks

cuno tests are written with the bundled headers and run with `cuno test`. How to write them is in [testing.md](../specs/testing.md). Use `cuno/test.hpp` in C++20, and `cuno/test.h` in C and C++17.

| In ctest / a test framework | In cuno |
|---|---|
| `enable_testing()` and `add_test(...)` | Write nothing. Files directly under `tests/` and in-source tests wrapped in `#ifdef CUNO_TEST` are found |
| `ctest` | `cuno test` |
| `ctest -R pattern` | `cuno test pattern` (filters by a substring of the name) |
| `ctest -j8` | `cuno test -j 8`. Tests run in parallel (by default as many at once as there are CPU cores), each in its own process |
| `ctest --output-on-failure` | Nothing to do. The output of failed tests is shown by default. To always show it, use `cuno test --nocapture` |
| GoogleTest's `TEST(Suite, Name)` | `test("name", [] { ... })` (C++20), `cuno_test(name)` (C / C++17) |
| `EXPECT_EQ(a, b)` / `ASSERT_EQ(a, b)` | `expect(eq(a, b))` / `require(eq(a, b))`, or `cuno_check_eq(a, b)` / `cuno_require_eq(a, b)` |
| Catch2's `CHECK` / `REQUIRE`, doctest's `CHECK` / `REQUIRE` | `expect` / `require`, or `cuno_check` / `cuno_require` |
| Floating-point comparison (`EXPECT_NEAR`, `Approx`) | `approx(a, b, tolerance)`, `cuno_check_approx(a, b, tolerance)` |
| `EXPECT_THROW` | `throws<ExceptionType>(function)` |
| `GTEST_SKIP()` | `skip(reason)`, `cuno_skip(reason)` |
| Run existing GoogleTest / Catch2 / doctest tests as they are | Not yet supported (M10, `framework` in `[test]`). In the meantime, rewrite them as cuno tests, or put them in `src/bin/` as a separate executable and run them with `cuno run --bin` |
| Benchmarks (Google Benchmark, etc.) | Not yet supported (M10, `cuno bench`) |

## From clang-format

| In clang-format | In cuno |
|---|---|
| Write `.clang-format` | Write `[fmt]` in `cuno.toml`: `style` (`cuno`, `llvm`, `google`, `microsoft`, etc.), `indent-width`, `column-limit`, `use-tabs` |
| Other `.clang-format` options | Write clang-format options as they are in `[fmt.clang-format]` |
| `clang-format -i $(git ls-files '*.cpp' '*.h')` | `cuno fmt`. It covers the package's code (`src/`, `tests/`, `examples/`, `benches/`, etc.) |
| `clang-format --dry-run --Werror` in CI | `cuno fmt --check` (exit code 1 if any file is not formatted) |
| Exclude files from formatting | `exclude` in `[fmt]` |
| The editor reads `.clang-format` | Nothing to do. `cuno build`, `cuno fmt` and `cuno lint` write `.clang-format` from `cuno.toml` to the project root |

## From clang-tidy

| In clang-tidy | In cuno |
|---|---|
| Write `Checks:` in `.clang-tidy` | `[lint]` in `cuno.toml`. Choose a `preset` (`recommended` / `strict` / `none`) and adjust it with `allow` / `warn` / `deny` |
| `WarningsAsErrors:` | `deny` in `[lint]` |
| `run-clang-tidy -p build` | `cuno lint`. cuno provides `compile_commands.json` |
| `clang-tidy --fix` | `cuno lint --fix` |
| Exclude files from analysis | `exclude` in `[lint]` |
| The editor reads `.clang-tidy` | Nothing to do. `cuno build`, `cuno fmt` and `cuno lint` write `.clang-tidy` from `cuno.toml` to the project root |

## From vcpkg / Conan

Dependency management arrives in M6 and M7. Until then, use a library by writing the locations of its headers and library files in `[build]`.

| In vcpkg / Conan | In cuno |
|---|---|
| Declare dependencies in `vcpkg.json` / `conanfile.txt` | Not yet supported (M6, `[dependencies]` in `cuno.toml`) |
| `vcpkg install sdl3` / `conan install` | Not yet supported (Git / URL in M6, `cuno add sdl3` in M7) |
| The baseline in `vcpkg-configuration.json`, `conan.lock` | Not yet supported (M6, `cuno.lock`) |
| features (vcpkg) / options (Conan) | Not yet supported (M7) |
| Binary caching | Not yet supported (Future) |
| Keep dependency sources in the repository | Not yet supported (M7, `cuno vendor`). In the meantime, sources placed under `src/` are built along with the package |
| Use prebuilt libraries | `include-dirs`, `lib-dirs`, `links` in `[build]` |

## From Cargo

cuno takes Cargo as its model, so many things work the same way.

| In Cargo | In cuno |
|---|---|
| `cargo new` | `cuno new` |
| `cargo init` | Not yet supported (M5, `cuno init`) |
| `cargo build` / `cargo build --release` | `cuno build` / `cuno build --release` |
| `cargo run -- args` / `cargo run --bin name` | `cuno run -- args` / `cuno run --bin name` |
| `cargo check` | `cuno check` (`--tests` and `--all-targets` work the same) |
| `cargo test filter` | `cuno test filter`. `#ifdef CUNO_TEST` instead of `#[cfg(test)]` |
| `cargo fmt` / `cargo fmt --check` | `cuno fmt` / `cuno fmt --check` |
| `cargo clippy` / `cargo clippy --fix` | `cuno lint` / `cuno lint --fix` |
| `cargo clean` | `cuno clean` |
| `[package]`, `[[bin]]`, `[profile.release]` in `Cargo.toml` | The tables of the same name in `cuno.toml` |
| `.cargo/config.toml` | `.cuno/config.toml` |
| `[dependencies]` | Not yet supported (M6) |
| `build.rs` | Not yet supported (M7, `[[build.rule]]` and Lua build scripts) |
| `rustup` | Not yet supported (M5, `cuno toolchain list`). Installing compilers comes later. For now, `cuno doctor` shows the compilers it found |
| `rustup update` (updating the tool itself) | Not yet supported (M5, `cuno self update`) |
| `cargo install --path .` | Not yet supported (M10, `cuno install`) |
| Workspaces | Not yet supported (M9) |

## From Premake

| In Premake | In cuno |
|---|---|
| Write `premake5.lua` | Write `cuno.toml`. No Lua script is needed |
| Generate projects with `premake5 vs2022` and build them | `cuno build`. There is no generate step. Generating Visual Studio projects is not yet supported (M9, `cuno ide vs`) |
| `kind "ConsoleApp"` / `"WindowedApp"` | `subsystem` in `[[bin]]` (`console` / `windows`) |
| `kind "StaticLib"` / `"SharedLib"` | Not yet supported (M6, `kind` in `[lib]`) |
| `files { "src/**.cpp" }` | Write nothing. Every source under `src/` is compiled |
| `includedirs` / `defines` / `links` / `libdirs` | `include-dirs` / `defines` / `links` / `lib-dirs` in `[build]` |
| `buildoptions` | `flags` in `[build.msvc]` and the like |
| `configurations { "Debug", "Release" }` and `filter "configurations:Debug"` | `[profile.dev.build]` / `[profile.release.build]` |
| `filter "system:windows"` | `[platform.windows.build]` |
| `cppdialect "C++20"` | `edition` in `[package]` |
| `staticruntime "On"` | `runtime = "static"` in `[build]` |
| `prebuildcommands` / custom Lua logic | Not yet supported (M7, `[[build.rule]]` and Lua build scripts) |

## From Meson

| In Meson | In cuno |
|---|---|
| Write `meson.build` | Write `cuno.toml` |
| `meson setup builddir` → `meson compile -C builddir` | `cuno build`. There is no setup step |
| `executable('app', sources)` | Write nothing. `src/main.cpp` becomes the default executable |
| `meson test` | `cuno test` |
| `--buildtype=release` | `cuno build --release` |
| `default_options: ['cpp_std=c++20']` | `edition` in `[package]` |
| `warning_level=3`, `werror=true` | `warnings`, `warnings-as-errors` in `[build]` |
| `meson_options.txt` | No equivalent. Write values in `cuno.toml`, and per-machine settings in `.cuno/config.toml` |
| `dependency('zlib')`, wraps | Not yet supported (M6 / M7) |
| `custom_target` | Not yet supported (M7, `[[build.rule]]`) |
| Cross files (`--cross-file`) | Not yet supported (M8, `--target`) |

## From Bazel

| In Bazel | In cuno |
|---|---|
| Write `BUILD` and `MODULE.bazel` | Write `cuno.toml` |
| `bazel build //app:main` | `cuno build` |
| `bazel run //app:main -- args` | `cuno run -- args` |
| `bazel test //...` | `cuno test` |
| `cc_binary` | `src/main.cpp` or `src/bin/`, or `[[bin]]` |
| `cc_library` | Not yet supported (M6, `[lib]`) |
| `copts` / `defines` / `includes` | `flags` in `[build.msvc]` and the like, `defines` and `include-dirs` in `[build]` |
| `select({"@platforms//os:windows": ...})` | `[platform.windows.build]`. Per compiler: `[build.msvc]` and the like |
| `-c opt` | `cuno build --release` |
| `genrule` | Not yet supported (M7, `[[build.rule]]`) |
| Remote caching / remote execution | No equivalent. Build caching comes in M9 |
| `bazel query`, tools that extract `compile_commands.json` | `compile_commands.json` is written to the project root on every build. JSON output of package information is not yet supported (M9, `cuno metadata`) |
