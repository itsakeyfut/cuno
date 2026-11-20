<!-- translated-from: docs/ja/specs/project-layout.md @ blob 6017835a550750c5444a72b9e3a0b25c70717672 -->

# Project Kinds and Layout

## Basic policy: layout and coding rules are left to the developer

- The directory layout and coding rules of C/C++ code are **freely decided by the developer**. cuno does not enforce a particular layout.
  - Examples: people who separate public headers into `include/`, people who keep headers and implementation together in `src/`, people who split into `Public/` / `Private/` in the Unreal Engine style. All of these are supported.
- Developers can **freely impose constraints** on their own project in `cuno.toml` (location of public headers, location of sources, etc.).

## Default layout + overridable

This reconciles "nothing is enforced" with "works without configuration (the cargo experience)".

- If nothing is written in `cuno.toml`, the project is **interpreted** as the following default layout (rules for what each location is treated as). For the files that `cuno new` actually generates, see "Generated files" in [commands.md](commands.md) (not everything in this diagram is generated).

```
mygame/
├── cuno.toml
├── include/mygame/   # public headers (libraries only)
├── src/
│   ├── main.cpp      # executable (default)
│   ├── lib.cpp       # library (once libraries are supported; in the MVP, treated as one file of the package body)
│   └── bin/*.cpp     # additional executables
├── tests/            # integration tests
├── examples/
└── benches/
```

- Every location can be overridden in `cuno.toml` (overriding via the library's `sources` / `public-headers` comes "Later", together with library support; in the MVP, only `[[bin]] path` and `include-dirs` can be overridden). Example (Unreal Engine style):

```toml
[lib]
kind = "static"
public-headers = ["Public"]
sources = ["Private"]
```

## Counting artifacts

- 1 package = **at most 1 library + multiple executables**.
- If multiple libraries are needed, split them into multiple packages in a **workspace**.
  - Example (game): three packages, `engine` (library), `game`, and `editor`, in one workspace.
- Creation: `cuno new` creates an executable (default); `cuno new --lib` creates a library.
- **The MVP supports executables only.** Libraries (`[lib]`, `cuno new --lib`) come "Later". Thanks to "Source collection and linking" below, integration tests in `tests/` work even without a library.

## Source collection and linking

### Target files

- All sources under `src/` are compiled.
- The language is determined by file extension.
  - `.c`: compiled as C.
  - `.cpp` / `.cc` / `.cxx`: compiled as C++.
- Mixing C and C++ is allowed. Files in a language different from `[package] lang` are compiled with that language's default standard (C: `c17`, C++: `c++20`).

### Entry files and package body

| Category | Files |
|---|---|
| **Entry files** | Files that contain `main`. The file specified by `path` in `[[bin]]` (default `src/main.cpp` / `src/main.c`), and C/C++ sources directly under `src/bin/` (automatic detection when `[[bin]]` is not written; see [manifest.md](manifest.md)) |
| **Package body** | Sources under `src/`, **excluding**: entry files, `src/main.*`, and everything under `src/bin/` |

- `src/main.*` and everything under `src/bin/` are **always excluded from the package body**, regardless of whether `[[bin]]` is written.
  - Rationale: writing `[[bin]]` disables automatic detection, so without this exclusion `src/main.cpp` and the like would be linked as package body into every executable and test, duplicating `main`.
  - When `[[bin]]` is written, files under `src/main.*` / `src/bin/` that are not listed in `[[bin]]` are not built (cuno warns about this).

### Linking rules

| Artifact | What is linked |
|---|---|
| Executable | Its entry file + package body |
| In-source test executable | Test runner + package body (entry files are not included) |
| Integration test executable in `tests/` (one per file) | Test runner + package body + that test file |

- **Entry files are not included in test builds**, to avoid a collision between the test runner's `main` and the user's `main`.
  - Other approaches considered and why they were rejected:
    - Renaming the user's `main` in test builds (`/Dmain=...`): SDL3 itself does `#define main SDL_main`, which collides. Fatal for game development.
    - Replacing the entry point via linker settings: entangled with C runtime initialization, and fragile across compilers.
  - If tests are written in an entry file, cuno warns that they will not be run.
- The library kind (static / shared / header-only) is specified with `[lib] kind` in `cuno.toml`.

## Open questions

- The full set of `cuno.toml` keys for specifying the layout
- How to describe workspaces
- Handling of DLLs for hot reloading (to be handled Later as shared libraries + reload support on the executable side)
