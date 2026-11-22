<!-- translated-from: docs/ja/specs/project-layout.md @ blob af136d7f30910b37819eb403e058d25ffca96f9c -->

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
  - Rationale: C++ projects often bundle a C library (`sqlite3.c` and the like). Compiled as C++, it either fails on C++'s stricter conversion rules or, with its names mangled, no longer links with callers that use it as C.
  - Headers (`.h`) are not compiled. `cuno fmt` treating `.h` as the package's language is a matter of formatting style and separate from this ([dev-tools.md](dev-tools.md)).

### Entry files and package body

| Category | Files |
|---|---|
| **Entry files** | Files that contain `main`. The file specified by `path` in `[[bin]]` (default `src/main.cpp` / `src/main.c`), and C/C++ sources directly under `src/bin/` (automatic detection when `[[bin]]` is not written; see [manifest.md](manifest.md)) |
| **Package body** | Sources under `src/`, **excluding**: entry files, `src/main.*`, and everything under `src/bin/` |

- `src/main.*` and everything under `src/bin/` are **always excluded from the package body**, regardless of whether `[[bin]]` is written.
  - Rationale: writing `[[bin]]` disables automatic detection, so without this exclusion `src/main.cpp` and the like would be linked as package body into every executable and test, duplicating `main`.
  - When `[[bin]]` is written, files under `src/main.*` / `src/bin/` that are not listed in `[[bin]]` are not built (cuno warns about this).
- The following are errors (exit code 3):
  - There is no executable (no default entry file, nothing in `src/bin/`, and no `[[bin]]` written).
  - A `[[bin]]` `path` has no file, or is not a C/C++ source (`.c` / `.cpp` / `.cc` / `.cxx`).
  - An entry file is outside the project, since where its object file goes is decided from the source's place in the project ([build.md](build.md#output-directory)).
  - Two executables get the same name (a `[[bin]]` `name` written twice, or an auto-detected `src/bin/app.cpp` beside the default executable `app`, for example), since they would be the same file in the output directory. On Windows names are compared without regard to case (`Tool` and `tool` are the same file too).
- On Windows, file and directory names are handled without regard to case: `src/Main.cpp` is the default entry file too, and a `[[bin]]` `path` written `SRC/Main.cpp` is the same file as `src/main.cpp`.
  - Rationale: counting the same file twice under two spellings would put an entry file in the package body as well, compiling it twice and defining `main` twice.
- A link to a directory inside `src/` (a symbolic link or a junction) is not read, and cuno warns about it.
  - Rationale: a link cycle would never let the search end, and files outside the project would silently join the package body.

### Where the package's code lives

The places where the package's code (the C/C++ sources and headers the user writes) lives are the following directories. What `cuno fmt` / `cuno lint` act on is decided from these ([dev-tools.md](dev-tools.md#target-files)).

- `src/`, `tests/`, `examples/`, `benches/`
- For a file given by `[[bin]] path` that is in none of the above, the directory that contains it

The following are excluded:

- The output directory (judged by the absolute path of the output directory actually used, including when it is changed with `--target-dir` and the like; [build.md](build.md#output-directory))
- A subdirectory with its own `cuno.toml` (another package)

- `include-dirs` is not part of these places.
  - Rationale: `include-dirs` often points at third-party headers (`third_party/include` and the like), and formatting them would rewrite files that do not belong to the user. Also, `include-dirs` can be added in `[platform.<OS>.build]` / `[profile.<name>.build]`, so deciding its value needs the compiler and profile selected, whereas `cuno fmt` must work without a compiler.
- These places are decided by directories alone, not by the selected compiler, profile or target.

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
