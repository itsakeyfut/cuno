<!-- translated-from: docs/ja/specs/README.md @ blob 4faaca1d9a548043c0267347d912edb0cbc91050 -->

# cuno Specification

This set of documents describes the decided specifications of cuno. Design and implementation follow the contents of this directory. Each document is written to be readable on its own, and also records the rationale behind its decisions.

## Language and source of truth

- **The source of truth is the Japanese version (`docs/ja/specs/`).** The implementation follows it.
- This English version (`docs/en/specs/`) is a translation for public readers. If the two disagree, the Japanese version prevails.
- Whenever the Japanese version is changed, **the English version is updated in the same commit**.
- The top of each file in the English version records the source file and the content hash of the Japanese file it was translated from:
  ```
  <!-- translated-from: docs/ja/specs/overview.md @ blob <git hash-object of the Japanese file> -->
  ```
  A content hash (not a commit hash) is used because it is known before committing, so the Japanese and English files can be committed together. If this record differs from the current `git hash-object` of the Japanese file, the English version is out of date.

## Contents

| Document | Contents |
|---|---|
| [overview.md](overview.md) | Purpose, users, goals / non-goals / anti-goals, MVP scope |
| [commands.md](commands.md) | Command structure, options, output, exit codes |
| [project-layout.md](project-layout.md) | Project kinds and layout |
| [manifest.md](manifest.md) | Configuration files (`Cuno.toml` / `.cuno/config.toml`) |
| [toolchain.md](toolchain.md) | Supported compilers, language standards, compiler abstraction |
| [build.md](build.md) | Build engine |
| [dependencies.md](dependencies.md) | Dependency management |
| [testing.md](testing.md) | Testing |
| [dev-tools.md](dev-tools.md) | Development tools (fmt / lint / doc / IDE integration) |
| [distribution.md](distribution.md) | Distribution and operation of cuno itself, license, cuno home directory |
| [architecture.md](architecture.md) | Internal structure of cuno itself (crate layout, repository layout) |

Not a spec: for people coming from other tools (CMake, Visual Studio, clang-format and so on), the [migration guide](../guides/migration.md) maps what they did there to cuno.

## Conventions

### Open questions

Items in the "Open questions" section of each document do not have a decided specification yet. They are handled as follows.

- Items related to the MVP: decided during the design phase of implementation, and the decision is written back into the relevant document in this directory.
- Items related to stages after the MVP: decided before implementation of that stage begins, and written back in the same way.
- Content marked as an "initial proposal" (such as the mapping between settings and compiler flags) is verified and finalized during implementation, then written back.

### Timeline labels

| Label | Meaning |
|---|---|
| MVP | Included in the first version (scope in [overview.md](overview.md)) |
| Right after MVP | Top priority after the MVP (GCC support, `cuno init`, `cuno toolchain list` / `default`, `cuno self update`) |
| Dependency management stages 1–3 | Stages of dependency management ([dependencies.md](dependencies.md)). Stage 1 is started next, after the "Right after MVP" items |
| Later | After "Right after MVP". The order is decided when work begins |
| Future | No plans to start at present. Considered when the need arises |
