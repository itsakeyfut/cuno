# Changelog

What each release of cuno changed. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and cuno follows
[semantic versioning](https://semver.org/). A release's section becomes its
release notes on GitHub.

## [Unreleased]

## [0.1.0] - 2026-11-26

The first release: the whole C/C++ workflow, `cuno new` → `cuno fmt` →
`cuno lint` → `cuno test` → `cuno run`, from one `cuno.toml`, on Windows
x64 with MSVC, clang and clang-cl.

### Added

- `cuno new`: a C or C++ package with a sample, its tests, and a
  `.gitignore`.
- `cuno build`, `cuno run`, `cuno check` (`--tests`, `--all-targets`) and
  `cuno clean`: a built-in build engine with incremental, parallel builds,
  header dependency tracking, the `dev` and `release` profiles, and
  `compile_commands.json` for editors.
- `cuno test`: tests written beside the code or in `tests/`, with the bundled
  `cuno/test.hpp` and `cuno/test.h`; one process per test, run in parallel,
  output captured.
- `cuno fmt` and `cuno fmt --check`, with clang-format: the cuno style or a
  named one, `[fmt]` settings, and an `exclude` list.
- `cuno lint` and `cuno lint --fix`, with clang-tidy: the `recommended`,
  `strict` and `none` presets, `allow` / `warn` / `deny`, and an `exclude`
  list.
- `.clang-format` and `.clang-tidy` written from `cuno.toml` for editors.
- `cuno doctor`: what cuno finds on the machine, and a fix for each problem.
- Compiler detection and selection (Visual Studio and Build Tools, LLVM),
  the Windows build environment set up without a Developer Command Prompt,
  and a check that the compiler and clang-tidy pair with the MSVC standard
  library in use.
- `cuno.toml` and `.cuno/config.toml`, checked on reading: errors at their
  line, warnings for unknown keys.
