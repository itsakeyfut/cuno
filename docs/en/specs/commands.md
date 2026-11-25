<!-- translated-from: docs/ja/specs/commands.md @ blob 84a6a133f39d68277445fd52ed4897fed05d94d0 -->

# Commands

The command structure is based on cargo, with additions for C/C++ and for circumstances specific to cuno.

## List

| Category | Command | Description | Timeline |
|---|---|---|---|
| Project | `new` | Create a project in a new directory | MVP |
| | `init` | Turn an existing directory into a project | Right after MVP |
| Build | `build` | Build only | MVP |
| | `run` | Build and run | MVP |
| | `check` | Syntax and type check only (flags in [build.md](build.md#settings-and-flags)). Fast, since no object files are produced (MSVC alone runs the build's own compile without linking). By default checks the program's sources (the package body and entry files); `--tests` checks the test code (the package body with `CUNO_TEST` defined, and `tests/`), and `--all-targets` checks both | MVP |
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

- `-q` leaves out only status lines. Status lines are progress and summary lines (`Compiling`, `Running`, `Finished`, `Created`, `Removed`, `cuno doctor`'s `Selected`, `Found` and `Info`, and so on). Errors, warnings, `help:`, a command's results (`cuno test`'s line for each test and its `test result:` line, and so on), and the output of other programs such as the compiler are kept. The note on `-q` in each command's section applies this rule, and new commands follow it too.
- `-v` and `-q` cannot be given together (a usage error, exit code 2). Between `[term] verbose = true` in a configuration file and `-q`, `-q` wins.
  - Reason: as with `--color`, a setting given explicitly in command arguments takes precedence over the configuration files.
- After the arguments are parsed, every command reads the configuration files ([manifest.md](manifest.md)) before it starts its own work, so `[term] color` and `[term] verbose` apply to every command (`--help`, `cuno` with no command, and usage errors are printed before the files are read, as below). A configuration file that cannot be read, or has an error, is an error (exit code 3), and the command's work does not start; `cuno doctor`, however, reports it as a problem and goes on diagnosing ([`cuno doctor`](#cuno-doctor)).
  - `cuno new` alone reads only the user's `<cuno home>/config.toml`, and does not read the `cuno.toml` and `.cuno/config.toml` of a project around the current directory. When the user's `config.toml` cannot be read or has an error, it is an error (exit code 3) as for other commands, and nothing is created.
  - Reason: what `cuno new` creates is a new package, and the surrounding project's settings do not belong to that package. Even if that project's settings are broken, that is no reason to stop creating the new package.

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

- Coloring follows the highest of the following:
  1. `--color`
  2. `[term] color` in a configuration file ([manifest.md](manifest.md#term))
  3. The `NO_COLOR` environment variable. If it is set, it is treated as `never`, even when its value is empty
  4. The default `auto`
  - Reason: under the NO_COLOR convention, a setting the user gave explicitly in arguments or a configuration file takes precedence over NO_COLOR. You can keep `NO_COLOR` set for the whole environment and still color cuno alone.
- The `CLICOLOR` / `CLICOLOR_FORCE` environment variables are not read. To color output that is not a terminal, use `--color always` or `[term] color = "always"`.
  - Reason: there are already two ways to always color, and this avoids adding more environment variables to read and more precedence rules.
- `--help`, `cuno` with no command, and usage errors (exit code 2) are printed before the configuration files are read, so `[term] color` does not apply; they follow `--color`, `NO_COLOR` and the default `auto`, in that order. When the value of `--color` itself is invalid, the color of that usage error is decided as if `--color` were absent.
- Errors shown before the configuration files have been read (such as errors in a configuration file itself) are colored as decided by `--color` and `NO_COLOR` alone.

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
  - Windows reserved names (`con`, `prn`, `aux`, `nul`, `com1` to `com9`, `lpt1` to `lpt9`, in any case) are also an error, on every OS.
    - Reason: a package name becomes a directory name and an executable name. On Windows, files or directories with these names cannot be created, or are hard to remove once created.
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

#### Output and errors

- Once everything is created, ``     Created `<name>` package`` is printed (not under `-q`).
- The errors in the table below are exit code 2. A file or directory that cannot be created is exit code 3.

| Case | Message |
|---|---|
| The name does not satisfy the rules | `` error: `<name>` is not a valid package name `` and `help: a package name starts with a letter and has only letters, digits, - and _`. If the name came from the directory name, `help: name the package with --name` is also printed |
| The name is a Windows reserved name | `` error: `<name>` is a reserved name on Windows `` and `help: choose another name` |
| The target already exists | `` error: `<path>` already exists `` |
| `--edition` does not match `--lang` | `` error: `<standard>` is not a standard of <language> `` and `help: use --lang <language of the standard>, or an edition of <language>: <list of that language's standards>` |

- If git is not found, `warning: git was not found; the package is not under version control` is printed. If `git init` fails, this too is only a warning, `warning: git init failed: <git's output>` (the files have already been created, so what was created is not removed).

### `cuno run`

| Option | Description |
|---|---|
| `--bin <name>` | The executable to run (required if there are several; if omitted, an error lists the candidates) |
| `-- <args...>` | Everything after `--` is passed to the program |

- The working directory at run time is **the project root** (where `cuno.toml` is). This is the base for loading game assets with relative paths.
- Builds as `cuno build` does (with the same output and the same handling of failures), then runs the chosen executable.
- How the executable is chosen: if there is only one, that one; with `--bin`, the one of that name. On Windows, names are compared without regard to case, as when checking for duplicate executable names (`--bin APP` runs `app`; see [project-layout.md](project-layout.md#entry-files-and-package-body)). Both are checked before building.
  - Reason: comparing the same way as the duplicate check means exactly one executable matches, and it is the same file in the output directory too.
  - Several, and no `--bin`: `error: several executables can be run; choose one with --bin` and `help: available: app, tool` (the candidates' names), exit code 2.
  - No executable of the name given to `--bin`: ``error: no executable named `x` `` and the same `help:`, exit code 2.
- Before running, a `Running` line shows the executable's path (relative to the current directory; absolute if it is not under it). Not printed with `-q`.

```
    Finished `dev` profile [unoptimized + debuginfo] in 1.24s
     Running `target\debug\app.exe`
```

- The program is given the environment variables cuno received, as they are (not the Windows environment resolved for the compiler). stdin, stdout and stderr stay connected.
- The exit code is the program's exit code ([Exit codes](#exit-codes)).

### `cuno test [filter]`

| Option | Description |
|---|---|
| `[filter]` | Run only tests whose names contain the given string |
| `--nocapture` | Always show stdout and stderr during tests (by default, only for failed tests) |

- The common options for build commands (`--release`, `--profile`, `--compiler`, `--target-dir`, `-j`) are also available.
- cuno builds the test build ([build.md](build.md#output-directory)) the same way as `cuno build` (with the same output and the same handling of failures). Then, for each test executable, a `Running` line shows the executable's path before it runs, and the results are shown in the format of [testing.md](testing.md#output).
- The executable path on the `Running` line is relative to the current directory, as for `cuno run` (absolute if it is not under it). With `-q`, the `Running` line is not printed (the test lines, the failure details and the `test result:` line still are).
- The filter applies to every test executable alike. An executable with no matching tests still prints `running 0 tests`.
- Exit codes: `0` when everything passes (including when the filter matches no tests). `1` if even one test fails. A test executable whose list cannot be taken ([testing.md](testing.md#output)) also gives `1`. A build failure is also `1` ([Exit codes](#exit-codes)).

### `cuno clean`

| Option | Description |
|---|---|
| `--release` / `--profile <name>` | Delete only that profile's output. If not specified, the entire output directory is deleted |
| `--target-dir <path>` | The output directory to delete (follows the precedence in [build.md](build.md)) |

- Without a profile, the entire output directory is deleted; with one, only `<output dir>/<profile dir>` (`debug` for `dev`). Nothing outside the output directory is deleted.
- Before deleting, the output directory is checked by the rules in [build.md](build.md#output-directory). An output directory that is the project itself or above it, or one outside the project set by the project's own settings (`cuno.toml`, `.cuno/config.toml`), is an error (exit code 3), and nothing is deleted.
- Run outside a project, it is the same error as `cuno build` (exit code 3).
- If the output directory exists and has no cuno marker (`.cuno-target`, [build.md](build.md#output-directory)) directly in it, nothing is deleted and it is an error. With a profile too, it is the marker directly in the output directory that is checked (``error: `<output dir>` was not made by cuno; not removing it``, `help: remove it yourself, or set target-dir to a directory cuno builds into`, exit code 3).
- If the directory to delete does not exist, nothing is done and it succeeds.
- If a removal fails (a file is in use by another program, for example), it is an error (exit code 3). What was already removed stays removed.
- At the end, the number of files deleted is shown (`file` for one; not printed with `-q`).

```
     Removed 42 files
```

### `cuno fmt`

| Option | Description |
|---|---|
| `--check` | Do not format; fail if any file is not formatted (for CI) |

- At the end, the number of files rewritten is shown (`file` for one; not printed with `-q`).

```
   Formatted 3 files
```

- `--check` writes nothing. For each file that is not formatted it prints ``error: `src/main.cpp` is not formatted``, then ends with `help: run cuno fmt to format them` and exit code 1. If every file is formatted, it prints nothing and exits with code 0.
  - Reason: clang-format's own check prints several lines for each place that is off, so even a single misformatted line takes a dozen or more lines. There is only one way to fix it, `cuno fmt`, so the file names are enough.
- A file that could not be formatted is reported as ``error: could not format `src/main.cpp`: <reason>`` and left as it was. The other files are still formatted, the `Formatted` line is still printed, and the exit code is 1. A file that changed while it was being formatted, and a non-empty file for which clang-format printed nothing, are also reported this way and not rewritten.
  - Reason: a file is not rewritten in place; the new content is written next to it and then replaces it. If cuno stops partway, no half-written file is left behind, and changes saved during formatting are not lost.
- If clang-format does not accept `[fmt.clang-format]` (an unknown key, for example), it is an error (exit code 3) before any formatting starts.

### `cuno lint`

| Option | Description |
|---|---|
| `--fix` | Fix findings that can be fixed automatically |

- Findings are printed as clang-tidy shows them (`file:line:column: warning: ... [check name]` and an excerpt of the code). clang-tidy runs for each source in parallel according to `-j`, and its output is grouped per source so that it does not interleave.
- clang-tidy's summary lines (`N warnings generated.` and the like) are not printed. When the same finding in the same header comes from several sources, it is printed only once.
  - Reason: keeping the findings in their original form lets editors and CI read them as they are. Rebuilding them risks missing findings when a clang-tidy version changes how they are shown.
- At the end, the number of files analyzed and the number of findings are shown (not printed with `-q`).

```
      Linted 12 files: 3 warnings, 1 error
```

### `cuno build` / `cuno check`

`cuno build` takes common options only. `cuno check` also accepts the following options, which choose what to check.

| Option | Description |
|---|---|
| (none) | Check the program's sources (the package body and entry files) |
| `--tests` | Check only the test build (the package body with `CUNO_TEST` defined, and the sources in `tests/`; [testing.md](testing.md#execution)) |
| `--all-targets` | Check both the program and the tests |

- Reason: targets are chosen the same way as Cargo's `cargo check`. Test code is not checked by default because checking the tests means checking the package body again (with `CUNO_TEST` defined), which would slow down everyday checks.
- Giving both `--tests` and `--all-targets` is the same as `--all-targets`.
- Checking the test build uses the same output location as `cuno test` (where the test build goes). On MSVC it runs the same compile as `cuno test`, so a following `cuno test` only links and runs.

- Run outside a project, it is an error (`error: could not find cuno.toml in <directory> or any parent directory`, exit code 3). When the settings are also wrong (a wrong output directory setting, a user's `config.toml` that cannot be read or has an error, and so on), only the missing project is reported. `cuno run`, `cuno test` and `cuno clean` do the same.
  - Reason: outside a project these commands can do nothing. Only the one thing to fix (run inside a project) is reported.
- Before building, the compiler's selection and its pairing with the standard library are checked ([toolchain.md](toolchain.md#minimum-compiler-versions)); if they fail, nothing is built and it is an error (exit code 3).
- Output (in the form of [Output](#output)):

```
   Compiling app v0.1.0 (msvc 19.52.37029)
     Running `cl.exe /nologo /utf-8 ... /c src\main.cpp /Fo...`
    Finished `dev` profile [unoptimized + debuginfo] in 1.24s
```

  - One `Compiling` line (the package's name and version, the compiler and its version) before compiling starts. When everything is up to date and nothing runs, it is not printed, and only the `Finished` line is.
  - With `-v`, a `Running` line with the command before each step (a compile, a link) starts.
  - A `Finished` line at the end. Inside the brackets is `unoptimized` when `opt-level` is `0` and `optimized` otherwise, followed by `+ debuginfo` when `debug-info` is on.
  - With `-q`, only errors, warnings and the compilers' output are shown.
- When a compile or a link fails, the compiler's output is followed by an `error: could not compile` line (`could not link` for a link; both followed by the package's name), exit code 1. After the first failure no new step starts; the steps running are waited for and their output shown too.
- The output of steps run in parallel is shown step by step, each when it finishes, so their lines do not mix.
- `--target` is reserved for cross-compiling; giving it is an error (`error: --target is not supported yet`, exit code 3).
- `cuno check` checks the same sources as the build with syntax-and-type-check-only flags ([build.md](build.md#settings-and-flags)); it produces no object files and does not link (MSVC is the exception; see below). Its output has a `Checking` line instead of `Compiling`, and the same `Finished` line. A failure ends, as a failed compile in a build does, with a `could not compile` line (followed by the package's name), exit code 1.
  - By the same mechanism as the build, unchanged sources are not checked again ([build.md](build.md#deciding-what-changed)).
  - On MSVC, `check` runs the build's own compile without linking and produces the build's own objects (`/Zs` does not emit the diagnostics raised during code generation, so `check` could succeed where the build fails; [build.md](build.md#always-on-flags)). The build that follows only links.

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
- With `-q`, only the configuration file warnings and the problems (`error:` and `help:`) are printed. When there is no problem, nothing is printed (the exit code tells).
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
- Compiler errors and warnings are displayed as is. A step that is up to date and does not run shows the output of its last successful run (warnings, for example) as is ([build.md](build.md#deciding-what-changed)). When cuno does not color its output (the color rule below), the color escape sequences are removed from that output first. It is shown where a step that ran would be: after the `Compiling` line in a build that runs other steps, and before the `Finished` line in a build that runs none.
  - Rationale: the inputs and the command are the same, so the output is the same. If it were not shown, merely building again without changing anything would make the warnings disappear, leaving no way to see a missed warning again.
  - The exception is the source's file name (`main.cpp`, for example) that cl.exe prints as the first line of every compile, which is not shown. It is removed only when the first line of the output is the compiled source's file name. cl prints a name with non-ASCII characters in the console's code page (CP932 on Japanese Windows), so for such a name the line counts as the file name when the ASCII parts at its start and end match and it holds no `:` (no Windows file name has a `:`, and every cl diagnostic does).
    - Rationale: it is neither an error nor a warning, appears once per source with MSVC alone, and makes the output differ from the other compilers'.
  - They are not translated (cl.exe from a Japanese-language Visual Studio stays in Japanese). Only cuno's own messages are unified in English.
  - Compilers that can produce colored output, such as Clang, are displayed in color. MSVC does not produce color, so its output is shown as is, without color.
    - Only when cuno colors its output (by the order in "Available for all commands" above) does it pass `-fcolor-diagnostics -fansi-escape-codes` to clang and clang-cl. cuno receives the compiler's output through a pipe, so without them Clang does not color it. On Windows, `-fansi-escape-codes` is also needed so that the colors are written as escape sequences. These two are placed first among the arguments, so a `-fno-color-diagnostics` the user writes in `[build.clang] flags` or the like comes later and wins. They are not passed to a link (the linker's output has no color). The command `-v` shows includes them (it shows the command that was run).
    - These two are kept out of both the command line used to decide what changed ([build.md](build.md#deciding-what-changed)) and `compile_commands.json`. Rationale: changing only the color setting should not recompile everything. The generated code does not change. Clang does record its command line in debug info, though, so in a build with debug info the object files and PDB differ by whether these two were passed (and since nothing is recompiled, the previous build's stay).

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
