<!-- translated-from: docs/ja/specs/dev-tools.md @ blob c0b0f293640c504cbff7e022a088c9064b98254b -->

# Development Tools

## Commands

| Command | What it is | cargo equivalent |
|---|---|---|
| `cuno fmt` | Wraps clang-format | `cargo fmt` |
| `cuno lint` | Wraps clang-tidy (uses compile_commands.json) | `cargo clippy` |

## Configuration

- Formatter / linter settings are written in `[fmt]` / `[lint]` in `cuno.toml`. Users do not write `.clang-format` / `.clang-tidy`. cuno passes the settings to each tool.
- **Generated files for editors**: cuno generates `.clang-format` / `.clang-tidy` from `[fmt]` / `[lint]` in `cuno.toml` and places them in the project root (`cuno new` adds them to `.gitignore`).
  - Reason: when an editor formats or analyzes code through clangd etc., without these files it runs with LLVM defaults (indent 2, etc.), and the results diverge from `cuno fmt` / `cuno lint`.
  - The source of truth is always `cuno.toml`. Even if users edit the generated files, they are overwritten on the next generation. Since these are not configuration files written by users, they do not conflict with the "don't add more configuration files" policy (treated the same as compile_commands.json; see [build.md](build.md)).
  - They are generated when running `build` / `run` / `check` / `test` / `lint` / `fmt` (`fmt --check` writes nothing, so it does not generate them). The commands that generate compile_commands.json are decided by [build.md](build.md#output), and `fmt` is not among them.

## Implementation approach

- **Formatter**: Initially wraps clang-format. Designed so it can be replaced with our own implementation in the future (this can be achieved with lexical analysis + formatting rules and does not need full C++ semantic analysis, so in the long term our own implementation is realistic).
- **Linter**: Keeps wrapping clang-tidy (many checks need full C++ semantic analysis, so our own implementation is unrealistic). cuno's own lightweight checks can be added.

## Target files

| Command | Targets |
|---|---|
| `cuno fmt` | C/C++ files in the places where the package's code lives ([project-layout.md](project-layout.md#where-the-packages-code-lives)) (excluding `[fmt] exclude`) |
| `cuno lint` | C/C++ files in the places where the package's code lives (excluding `[lint] exclude`). Sources are analyzed with their commands from compile_commands.json, and headers through the sources that include them. Test code (with `CUNO_TEST` enabled) is also analyzed |

- Target extensions: `.c` / `.h` / `.cpp` / `.cc` / `.cxx` / `.hpp` / `.hh` / `.hxx` / `.inl`.
- `.h` is treated as the language given by `[package] lang` (because pointer placement in the cuno default style differs between C and C++).
- `cuno fmt` and `cuno lint` decide what they act on from the same "places where the package's code lives". A file placed outside `src/` with `[[bin]] path`, and the headers next to it, are included. `cuno-build` builds the list of targets with the same walk as classifying sources, and `cuno-cli` receives it and passes it to the fmt / lint implementation ([architecture.md](architecture.md#dependencies-between-crates)).
  - Rationale: if fmt and lint decided their targets differently, the same file could be analyzed by one and not formatted by the other. Looking only at fixed directories, a file placed outside `src/` would be used by `build` and `lint` yet pass `fmt --check` unformatted. It would also go against not enforcing a layout (design principle 3 in [concept.md](../concept.md)).
- `cuno fmt` works without a compiler. Deciding its targets uses neither compiler detection nor selection.
  - Rationale: `fmt` runs with clang-format alone and should work where no compiler is installed (such as a CI job that only checks formatting). The same reason is why `fmt` does not generate compile_commands.json.
- `cuno lint` does not accept `--release` / `--profile`. Specifying one is an unknown option, exit code 2. It accepts `--compiler` and analyzes using commands for that compiler.
  - Rationale: the contents of compile_commands.json are always fixed to dev ([build.md](build.md#output)), so they would have no effect. If they were accepted and did nothing, users would believe the analysis used the release settings. Rejecting them is noticed at once.

## `[fmt]` / `[lint]` keys

Policy: following rustfmt, **keep the number of tunable options small**. Do not expose all of clang-format's options as they are (to ease the future migration to our own formatter).

```toml
[fmt]
style = "cuno"         # base style (cuno / llvm / google / microsoft / mozilla / webkit / chromium / gnu)
indent-width = 4       # indentation width
column-limit = 100     # maximum characters per line
use-tabs = false       # whether to indent with tabs (true / false)
exclude = []           # files not to format (e.g. ["third_party/**"])

[fmt.clang-format]     # escape hatch: specify clang-format options directly (only effective while clang-format is used)
# PointerAlignment = "Left"

[lint]
preset = "recommended" # base set of checks (none / recommended / strict)
allow = []             # checks to disable
warn = []              # checks to report as warnings
deny = []              # checks to report as errors
exclude = []           # files not to analyze
```

### `[fmt]`

| Key | Default | Description |
|---|---|---|
| `style` | `"cuno"` | Base style |
| `indent-width` | Follows the chosen `style` (4 for `cuno`) | Indentation width |
| `column-limit` | Follows the chosen `style` (100 for `cuno`) | Maximum characters per line |
| `use-tabs` | Follows the chosen `style` (`false` for `cuno`) | Whether to indent with tabs |
| `exclude` | `[]` | Patterns of files not to format |

Values of `style`:

- `cuno`: The cuno default style (below). The default.
- `llvm`: The LLVM project's style (indent 2, 80 characters).
- `google`: Google's style (indent 2, 80 characters).
- `microsoft`: Microsoft's style (indent 4, Allman style with braces on the next line). For those who prefer a look close to Visual Studio / Unreal Engine.
- `mozilla`: Mozilla's style.
- `webkit`: WebKit's style.
- `chromium`: Chromium's style.
- `gnu`: GNU's style.

`indent-width` / `column-limit` / `use-tabs` override the chosen `style`'s values **only when written**. If not written, the `style`'s values are used (e.g. `style = "llvm"` alone gives indent 2, 80 characters). The 4 / 100 / false in the TOML example above are the values for `style = "cuno"`.

`use-tabs`:

- `true`: Indent with tabs.
- `false`: Indent with spaces. The value for the `cuno` style (because it looks the same everywhere, including GitHub and review tools).

`[fmt.clang-format]` is an escape hatch for specifying clang-format options directly. It is effective only while clang-format is used, and after migrating to our own formatter it is ignored (or converted to corresponding settings).

### cuno default style (`style = "cuno"`)

| Item | Setting | Reason |
|---|---|---|
| Base | LLVM | clang-format's native style and the most thoroughly tested. Fewer side effects when adjustments are added |
| Indentation | 4 (spaces) | Nesting depth is easy to see. Same as the Visual Studio default and Rust |
| Maximum characters per line | 100 | 80 is cramped with indent 4 and long C++ type names. 120 tends to overflow in side-by-side diffs. Same as rustfmt |
| Braces | Same line (`if (x) {`) | More information per screen. Same as major modern languages such as Rust / Go / Java / JS / Swift, and fits cuno's "modern" and "same experience as cargo" |
| Pointers and references | C++: `int* p` / C: `int *p` | Matches each language's convention. C++ follows Stroustrup and the C++ Core Guidelines (reading `int*` as a type); C follows the traditional convention of K&R, the Linux kernel, etc. |
| includes | Sorted automatically | The order is determined mechanically, reducing merge conflicts. For headers whose order matters (`windows.h`, etc.), separating them with a blank line prevents sorting across the separation |

- The confusion of `int* a, b;` (`b` does not become a pointer) is prevented by having lint flag "one declaration per line".

### `[lint]`

| Key | Default | Description |
|---|---|---|
| `preset` | `"recommended"` | Base set of checks |
| `allow` | `[]` | Checks to disable |
| `warn` | `[]` | Checks to report as warnings |
| `deny` | `[]` | Checks to report as errors |
| `exclude` | `[]` | Patterns of files not to analyze |

Values of `preset`:

- `none`: No checks. Only the checks specified in `warn` / `deny` are run.
- `recommended`: Checks with few false positives, such as bug-prone code (bugprone), performance (performance), and modernization (part of modernize). The default.
- `strict`: In addition to `recommended`, stricter checks such as readability (readability) and the C++ Core Guidelines family.

#### Contents of the presets

Check names are clang-tidy group names.

**recommended** (default): Checks with few false positives that directly relate to bugs or performance.

| Included | Description |
|---|---|
| `bugprone-*` | Potential bugs (use after move, suspected integer overflow, etc.) |
| `clang-analyzer-*` | Static analysis (null dereference, memory leaks, etc.). Slow but highly valuable |
| `performance-*` | Unnecessary copies, etc. |
| Part of `modernize-*` | Uncontroversial ones such as `use-nullptr`, `use-override`, `make-unique`, `use-emplace` |
| `readability-isolate-declaration` | One declaration per line (countermeasure for `int* a, b;`; a premise of the cuno default style) |

- Excluded: `bugprone-easily-swappable-parameters` (very many false positives).

**strict**: recommended plus the following.

| Added | Description |
|---|---|
| `readability-*` | Readability in general |
| `cppcoreguidelines-*` | C++ Core Guidelines |
| The rest of `modernize-*` | |

- Excluded (matters of taste, unrelated to strictness):
  - `modernize-use-trailing-return-type`: Enforces the `auto f() -> int` style.
  - `readability-identifier-length`: Forbids short variable names.

- In C projects, C++-only checks are automatically excluded.
- Fine-tuning of the exclusion lists is done during implementation and written back into this specification.

#### allow / warn / deny

`allow` / `warn` / `deny` follow the same idea as clippy. Check names are clang-tidy names (e.g. `bugprone-use-after-move`). Wildcards (e.g. `modernize-*`) can also be used.

- `allow`: Disables the check.
- `warn`: Displays findings as warnings. Does not affect the exit code.
- `deny`: Displays findings as errors and sets the exit code to `1`.

**Precedence rules** (when the same check matches multiple lists):

1. A specific name (`modernize-use-auto`) takes precedence over a wildcard (`modernize-*`).
2. At the same specificity, precedence is `deny` > `warn` > `allow`.

Example: with `warn = ["modernize-*"]` and `allow = ["modernize-use-auto"]`, only `modernize-use-auto` is disabled, and the other `modernize-*` checks are warnings.

#### Exit codes

| Situation | Exit code |
|---|---|
| No findings, or only warnings (preset checks / `warn`) | `0` |
| There are findings for `deny` checks | `1` |
| A compile error in clang-tidy itself (`clang-diagnostic-error`; a mismatch with the standard library (STL1000) is caught by the check before analysis with exit code 3, so it does not reach here) | `1` |

- Checks included in the preset are treated as **warnings** unless written in `deny`.

## Tool versions

- The minimum version of clang-format / clang-tidy is **17**. clang-tidy reads the MSVC standard headers, so on Windows it must also, like Clang, be accepted by the MSVC standard library in use ([toolchain.md](toolchain.md#minimum-compiler-versions)). clang-format does not read the standard library, so its only condition is the fixed 17. This prevents formatting results from changing across versions and `fmt --check` results from diverging between local machines and CI.
- Checked by `cuno doctor`.

## When a tool is not found

- If clang-format / clang-tidy is not found, or is below the minimum version, cuno prints an error message explaining how to install it and exits with exit code `3` (configuration / environment problem).

## Documentation generation (`cuno doc`, Future)

- Initially wraps Doxygen. Settings are written in `[doc]` in `cuno.toml`; users are not made to write a Doxyfile.
- Our own implementation would need C++ parsing, so it is not planned for now.

## IDE / editor integration

| Target | Approach | Timeline |
|---|---|---|
| Editors that use clangd (VS Code / CLion / Neovim, etc.) | Always output compile_commands.json ([build.md](build.md)) | MVP |
| Visual Studio | `cuno ide vs` generates a .sln. Builds from VS call cuno. The VS debugger can be used | Later |
| VS Code | `cuno ide vscode` generates launch.json / tasks.json | Later |
| IDE plugins, etc. | Provide project information as JSON via `cuno metadata` | Later |

- The source of truth for build settings is always `cuno.toml`. IDE files are merely generated files.
- Generated IDE files are placed inside `target/` so they do not clutter the repository. As exceptions, compile_commands.json, `.clang-format`, and `.clang-tidy` are placed in the project root (so that editors such as clangd can find them automatically; all of them are added to `.gitignore`).

## Open questions

- Fine-tuning of the preset exclusion lists (during implementation)
- When to migrate to our own formatter, and how to handle compatibility with clang-format
- Keys for `[doc]`
- Concrete contents of the generated VS solution (mapping of configurations and platforms)
