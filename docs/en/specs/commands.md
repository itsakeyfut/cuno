<!-- translated-from: docs/ja/specs/commands.md @ blob dadac57333eac02449f3b74a4c276d00f9d249ff -->

# Commands

The command structure is based on cargo, with additions for C/C++ and for circumstances specific to cuno.

## List

| Category | Command | Description | Timeline |
|---|---|---|---|
| Project | `new` | Create a project in a new directory | MVP |
| | `init` | Turn an existing directory into a project | Right after MVP |
| Build | `build` | Build only | MVP |
| | `run` | Build and run | MVP |
| | `check` | Syntax and type check only (flags in [build.md](build.md#settings-and-flags)). Fast, since no object files are produced. Also checks test code (with `CUNO_TEST` defined, and `tests/`) and entry files | MVP |
| | `clean` | Delete the output directory | MVP |
| Test | `test` | Run tests | MVP |
| | `bench` | Run benchmarks | Later |
| Development tools | `fmt` | Format. `--check` only verifies that files are formatted (for CI) | MVP |
| | `lint` | Static analysis. `--fix` applies automatic fixes | MVP |
| | `doc` | Generate documentation | Future |
| | `ide vs` / `ide vscode` | Generate IDE files ([dev-tools.md](dev-tools.md)) | Later |
| Dependencies | `add` / `remove` | Add / remove dependencies | Dependency management stage 1 |
| | `update` | Update the lock file | Dependency management stage 1 |
| | `tree` | Display the dependency tree | Dependency management stage 1 |
| | `fetch` | Download dependencies in advance (for offline work) | Dependency management stage 1 |
| | `vendor` | Copy dependencies into the repository (an approach favored in game development) | Dependency management stage 2 |
| Toolchain | `toolchain list` / `toolchain default` | List detected compilers / select the default | Right after MVP |
| | `toolchain install` | Automatic compiler installation (like rustup) | Future |
| Environment diagnostics | `doctor` | Diagnose whether VS / Clang / clang-format, etc. are usable, and suggest fixes | MVP |
| Distribution | `install` | Install built executables | Later |
| | `package` / `publish` | Package / publish | Future (Dependency management stage 3) |
| cuno itself | `self update` | Self-update cuno ([distribution.md](distribution.md)) | Later |
| Information | `metadata` | Output project information as JSON (for IDE and external tool integration) | Later |

See [dependencies.md](dependencies.md) for the stages of dependency management.

## cuno-specific commands

- **`doctor`**: C/C++ development on Windows often stumbles on the initial environment setup. This command immediately shows what is missing and how to fix it, addressing the "can't start casually" pain point.
- **`toolchain`**: Toolchain management is integrated into cuno itself rather than split into a separate tool like rustup.

## Common options

### Available for all commands

| Option | Description |
|---|---|
| `-v, --verbose` | Verbose output. For build commands, also prints the compiler commands that were run |
| `-q, --quiet` | Minimal output |
| `--color <when>` | Coloring (auto / always / never) |

### Available for build commands (`build` / `run` / `check` / `test`)

| Option | Description |
|---|---|
| `--release` | Build with the release profile |
| `--profile <name>` | Build with the specified profile. In the MVP, only `dev` / `release` (anything else is an error, exit code 3) |

- `cuno lint` does not accept these two (specifying one is an unknown option, exit code 2). The reason is in [dev-tools.md](dev-tools.md#target-files).

### Available for build commands and `lint` (`build` / `run` / `check` / `test` / `lint`)

| Option | Description |
|---|---|
| `--compiler <name>` | Compiler to use (msvc / clang / clang-cl / gcc / safec). See `compiler` in [manifest.md](manifest.md) for the values. Specifying `gcc` / `safec` in the MVP is an error (exit code 3) |
| `--target <target>` | Cross-compilation. Reserved only in the MVP; specifying it is an error (exit code 3) ([toolchain.md](toolchain.md)) |
| `--target-dir <path>` | Output directory ([build.md](build.md)). Relative paths are resolved against the current directory (the same applies to the `CUNO_TARGET_DIR` environment variable) |
| `-j, --jobs <n>` | Number of parallel jobs. Defaults to the number of CPU cores |

Values of `--color`:

- `auto`: Color only when writing to a terminal. The default.
- `always`: Always color.
- `never`: Never color.

- If the `NO_COLOR` environment variable is set, it is treated as `never` unless `--color` / `[term] color` is specified (following the NO_COLOR convention).

### Without a command (`--version` / `--help` / no arguments)

| Invocation | Stream | Output | Exit code |
|---|---|---|---|
| `cuno --version` (`-V`) | stdout | First line is `cuno <version>` | `0` |
| `cuno --help` (`-h`) | stdout | Usage (English) | `0` |
| `cuno` (no command) | stdout | The same usage as `--help` | `0` |

- `cuno` with no command is not treated as a usage error (same as Cargo).
- The format of the first line of `--version` is fixed, because scripts and `cuno self update` read it. Any additional information, such as a commit hash, goes on the second and later lines.

## Per-command options (MVP)

### `cuno new <path>`

| Option | Description |
|---|---|
| `--lang <language>` | Language (c / c++). Default c++ |
| `--edition <standard>` | Language standard. See `edition` in [manifest.md](manifest.md) for the values |
| `--name <name>` | Package name. Defaults to the directory name |
| `--vcs <kind>` | Version control (git / none). Default git |

- **Package name rules**: Only alphanumerics, `-`, and `_`. Must start with a letter. Violations are an error (exit code 2). If `--name` is omitted and the directory name does not satisfy the rules, it is also an error, and the user is directed to specify the name with `--name`.
- **If the target directory already exists**, it is an error (exit code 2). Turning an existing directory into a project is handled by `cuno init` (Right after MVP).
- If `--edition` and `--lang` do not match (e.g. `--lang c++ --edition c17`), it is an error. If only `--edition` is specified, the language is determined from that standard (`c11` / `c17` / `c23` mean C).
- `--lib` (create as a library) will be added "Later", together with support for library projects ([project-layout.md](project-layout.md)).

Values of `--vcs`:

- `git`: Initialize as a Git repository and generate `.gitignore`. The default.
  - If git is not installed, a warning is printed and the behavior is the same as `none`.
  - When creating inside an existing Git repository, `git init` is not run (same as cargo). `.gitignore` is still generated.
- `none`: Do not initialize version control.

#### Generated files

| File | Contents |
|---|---|
| `cuno.toml` | Only `name` / `version` (`0.1.0`) / `lang` / `edition` under `[package]`. No compiler is written (left to automatic selection) |
| `src/main.cpp` (`src/main.c` for C) | A `main` that calls a function in `src/calc.cpp` |
| `src/calc.cpp` / `src/calc.h` (`.c` / `.h` for C) | A small function and its in-source tests ([testing.md](testing.md)). Names that clash with standard headers (`math.h`, etc.) are avoided (since `src/` is on the include path, a clash would cause the standard library's `#include <math.h>` to pick up this file instead) |
| `.gitignore` | `target/`, `.cuno/`, `compile_commands.json`, `.clang-format`, `.clang-tidy` (the last three are generated by cuno; see [dev-tools.md](dev-tools.md)) |

- Empty `tests/` / `examples/` / `benches/` directories are not created.

### `cuno run`

| Option | Description |
|---|---|
| `--bin <name>` | The executable to run (required if there are several; if omitted, an error lists the candidates) |
| `-- <args...>` | Everything after `--` is passed to the program |

- The working directory at run time is **the project root** (where `cuno.toml` is). This is the base for loading game assets with relative paths.

### `cuno test [filter]`

| Option | Description |
|---|---|
| `[filter]` | Run only tests whose names contain the given string |
| `--nocapture` | Always show stdout and stderr during tests (by default, only for failed tests) |

### `cuno clean`

| Option | Description |
|---|---|
| `--release` / `--profile <name>` | Delete only that profile's output. If not specified, the entire output directory is deleted |
| `--target-dir <path>` | The output directory to delete (follows the precedence in [build.md](build.md)) |

### `cuno fmt`

| Option | Description |
|---|---|
| `--check` | Do not format; fail if any file is not formatted (for CI) |

### `cuno lint`

| Option | Description |
|---|---|
| `--fix` | Fix findings that can be fixed automatically |

### `cuno build` / `cuno check`

Common options only.

### `cuno doctor`

Diagnoses the environment and shows a fix for each problem.

| Option | Description |
|---|---|
| `--compiler <name>` | The compiler to diagnose. Same as `--compiler` of the build commands. If not specified, diagnoses the compiler chosen by the same selection precedence as the build commands ([toolchain.md](toolchain.md#selection)) |

- Can be run outside a project (where no `cuno.toml` is found). In that case, the compiler is chosen by the precedence without the project layers ([toolchain.md](toolchain.md#selection)).
- Even if a configuration file (`cuno.toml`, `.cuno/config.toml`, the user-level config) has an error, or a compiler not supported in the MVP (`gcc` / `safec`) is selected, `doctor` does not stop: it reports it as a problem (exit code `1`) and continues diagnosing the other items.
  - When a configuration file cannot be read, the compiler is chosen from `--compiler`, `CUNO_COMPILER` and automatic selection only. When that comes to automatic selection, the reason on the `Selected` line says `automatic selection; the configuration could not be read`, so it is not taken for the compiler the configuration chose.
  - Rationale: `doctor` is what users run when the environment or settings are wrong, so stopping at a configuration error would make it useless.

| Result | Condition | Exit code |
|---|---|---|
| Problem | Cannot build with the selected compiler (not found, does not satisfy the version requirement, Visual Studio environment cannot be resolved, etc.) | `1` |
| Problem | clang-format / clang-tidy not found | `1` |
| Problem | The selected Clang / clang-tidy is not accepted by the MSVC standard library in use, or this cannot be decided (in C++ projects and outside a project; [toolchain.md](toolchain.md#minimum-compiler-versions)) | `1` |
| Problem | An error in a configuration file, or an unsupported compiler selected | `1` |
| Problem | A tool written in `[toolchain.paths]` cannot be used (even a compiler that is not selected; [toolchain.md](toolchain.md#llvm-clang--clang-cl--clang-format--clang-tidy)) | `1` |
| Info | A compiler that is not used is not found (e.g. no Clang in an environment that uses MSVC) | No effect |

If there are no problems, the exit code is `0`.

#### What `cuno doctor` prints

One line per item, in the same form as the status lines of the other commands ([Output](#output)). Everything goes to stderr (stdout is kept for a future output that programs read).

```
    Selected clang-cl 22.1.8 (--compiler)
             C:\Program Files\LLVM\bin\clang-cl.exe
       Using MSVC 19.52.37029 (C:\Program Files\Microsoft Visual Studio\18\Community)
       Using Windows SDK 10.0.28000.0
    Accepted by the MSVC standard library
       Found clang-format 22.1.8
       Found clang-tidy 22.1.8
        Info msvc 19.52.37029 (not selected)
        Info clang 22.1.8 (not selected)
    Finished no problems found
```

- `Selected`: the compiler chosen, its version, and why it was chosen (`--compiler`, `CUNO_COMPILER`, one of the configuration files, or `automatic selection`). Its executable on the next line.
- `Using`: the MSVC toolset used (cl's version and the Visual Studio location; when MSVC is selected, the `Selected` line serves for it) and the Windows SDK.
- `Accepted`: when Clang is selected, that the MSVC standard library accepts it ([toolchain.md](toolchain.md#minimum-compiler-versions)).
- `Found`: clang-format and clang-tidy.
- `Info`: the compilers other than the one asked for: the version of the copy selecting it would use, `not found` when there is none, or `not usable` with the reason when one is found but cannot be used (below cuno's minimum, for example). It is judged as selection judges, except that `[toolchain.version]` is not applied (it applies only to the compiler actually used). They do not affect the exit code. But when the one written in `[toolchain.paths]` cannot be used, it is printed as a problem instead of `Info` (so that it is not first found when switching to that compiler).
- A problem is printed where it is found, as an `error:` and a `help:` line, and diagnosis continues. Warnings about configuration files come first, as `warning:`.
- The last line is `Finished no problems found`, or `Finished <n> problems found` (`problem` for one).

## Output

### Language

- Output messages are **unified in English**.
  - Rationale: international users are expected, error messages are easier to search for, and this is consistent with cargo and others.
  - Localization is a future consideration.

### Names of other tools

- cuno's own output (the `--help` description, messages) does not name the tools cuno was modeled on (cargo, CMake, etc.).
  - Rationale: cuno is presented as a tool whose description stands on its own, not as "something like" another tool, so that the description makes sense to users who do not know the tools it was modeled on.
  - The compilers and tools cuno actually invokes (MSVC, clang, clang-format, etc.) may be named, since diagnostics and guidance need them.

### Format

Follows cargo.

```
   Compiling mygame v0.1.0 (msvc 19.40)
    Finished `dev` profile [unoptimized + debuginfo] in 1.24s
     Running `target/debug/mygame.exe`
```

- cuno's own errors are shown as `error: ...`, and fixes as `help: ...`.
- Compiler errors and warnings are displayed as is.
  - They are not translated (cl.exe from a Japanese-language Visual Studio stays in Japanese). Only cuno's own messages are unified in English.
  - Compilers that can produce colored output, such as Clang, are displayed in color. MSVC does not produce color, so its output is shown as is, without color.

## Exit codes

| Code | Meaning | Examples |
|---|---|---|
| `0` | Success | |
| `1` | Problem with the user's code or artifacts | Compile error, test failure, unformatted files with `fmt --check`, lint `deny` violation, problems found by `doctor` |
| `2` | Incorrect command usage | Unknown option, missing argument |
| `3` | Configuration or environment problem | Error in `cuno.toml`, compiler not found, compiler version does not satisfy the requirement |
| `101` | Internal error in cuno itself (bug) | Unexpected failure. Same value as a Rust panic |

- `cuno run` returns the exit code of the program it ran as is (same as cargo). If the build fails, the table above applies.
- Rationale: being able to distinguish "code problems (1)" from "environment problems (3)" in CI speeds up isolating the cause of a failure.

## Open questions

- Options for commands outside the MVP
