<!-- translated-from: docs/ja/specs/dependencies.md @ blob 7ed62f7b915414e53e8df1e4fbe7e6f302331b6d -->

# Dependency Management

> Outside MVP scope. Implemented starting from "Dependency management stage 1" (see [README.md](README.md) for timeline labels).

## Policy: hybrid

- **cuno-format packages** (those with a `Cuno.toml`) are built by cuno itself.
- **Existing libraries** (SDL3, FFmpeg, etc.) are built by cuno invoking the existing build system (CMake, etc.) behind the scenes, following the build method described in a recipe.
- Users never see CMake.

## Stages

1. **Direct Git / URL**
   - Add dependencies with `cuno add --git <URL>` etc.
   - cuno fetches the hash automatically and records it in the lockfile.
2. **Recipe index**
   - A recipe index is hosted in a Git repository (equivalent to vcpkg's ports / crates.io-index).
   - A dependency can be added with just `cuno add sdl3`.
   - Recipes describe how to build existing libraries.
   - `cuno vendor`, which copies dependencies into the repository, is also provided at this stage ([commands.md](commands.md)).
3. **Public package service** (Future)
   - A service like crates.io will be considered once there are more users.

## Versions

- **One version per library**: in C/C++, having multiple versions of the same library coexist causes symbol conflicts, so the entire dependency graph is resolved to one version per library. If this cannot be satisfied, it is an error showing which dependencies conflict with each other.
- **Version requirements**: same syntax as Cargo (e.g. `"3.2"` means 3.2 or later and below 4.0).
- For libraries that do not follow Semantic Versioning, the recipe declares the compatible range.
- Git dependencies are pinned by tag / commit.

## Lockfile

- `Cuno.lock` (TOML). Records the resolved versions, sources, and hashes.
- Committing it to Git is recommended.

## Linking

- The default is **static linking**. It can be changed per dependency with `link`.
  ```toml
  [dependencies]
  ffmpeg = { version = "7.1", link = "dynamic" }   # Linking method (static / dynamic)
  ```
  - `static`: Static linking. Embeds the dependency in the executable. Default.
  - `dynamic`: Dynamic linking. Loads the dependency at run time as a DLL / shared library.
- **Licenses**: cuno displays dependency licenses. When distributing with static linking could be a problem, as with LGPL (FFmpeg, etc.), cuno warns.

## features (conditional features)

- Same mechanism as Cargo.
  ```toml
  [dependencies]
  sdl3 = { version = "3.2", features = ["audio", "vulkan"] }
  ```
- The recipe maps each feature to the library's build options.

## System libraries (Vulkan SDK, etc.)

- By default, they are built from source via a recipe, for reproducibility.
- `system` specifies whether to use the installed one.
  ```toml
  [dependencies]
  vulkan = { version = "1.4", system = true }   # Whether to use the installed one (true / false)
  ```
  - `true`: Uses the installed one (detected from environment variables such as `VULKAN_SDK`).
  - `false`: Builds from source via the recipe. Default.

## Binary cache

- Built dependencies are cached on the machine and reused across projects.
- A shared cache for teams / CI is Future.

## Open questions

- Recipe format (including how to split the declarative part from the Lua escape hatch)
- Version resolution algorithm, and error display on conflicts
- Concrete format of `Cuno.lock`
- Rules for deciding license warnings
- Binary cache storage location and cache key (compiler, version, flags, features, etc.)
