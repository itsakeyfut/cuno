<!-- translated-from: docs/ja/guides/game-development.md @ blob 949e9f89905f3cc299306f49345016723b2f2e1d -->
# Settings for game development

cuno's defaults are chosen so that any C/C++ project works as it is. Game development often picks settings other than the defaults, such as turning off exceptions and RTTI, or building the C runtime into the executable. This page shows how to write those settings in `Cuno.toml`, and what stops working once they are chosen.

Every setting here is an item of the `Cuno.toml` specification ([manifest.md](../specs/manifest.md)). This page decides nothing new; it only explains how to choose.

## Summary

The settings games often choose, written together:

```toml
[build]
exceptions = false    # no C++ exceptions
rtti = false          # no dynamic_cast / typeid
runtime = "static"    # build the C runtime into the executable

[profile.release]
lto = true            # link-time optimisation for release

[profile.release.build]
warnings-as-errors = true   # warnings are errors in release builds
```

A package made by `cuno new` builds, runs and tests with these settings as they are, with MSVC, clang-cl and clang. cuno's test header (`cuno/test.h`) works with exceptions and RTTI turned off too.

## Turning off exceptions (`exceptions = false`)

```toml
[build]
exceptions = false
```

- **What changes**: each compiler is given the flags that turn exceptions off (`/D_HAS_EXCEPTIONS=0` for MSVC, `-fno-exceptions` for GCC and Clang, and so on; the mapping is in [build.md](../specs/build.md#settings-and-flags)).
- **What stops working**:
  - Code with `try` / `catch` / `throw` fails to compile. cl would only warn, but cuno raises that warning to an error, so the build stops (only a `throw` with no `try` goes undetected by cl).
  - Standard library features that report failure with an exception (a failed allocation, `std::vector::at` out of range, and so on) stop the program instead of throwing.
  - The sources of a library that uses exceptions cannot be built as they are.
- **Handling errors instead**: return the failure. `std::optional` (C++17) returns that there is no value; `std::expected` (C++23) returns why as well.

## Turning off RTTI (`rtti = false`)

```toml
[build]
rtti = false
```

- **What changes**: each compiler is given the flags that turn RTTI off (`/GR-` for MSVC, `-fno-rtti` for GCC and Clang; the mapping is in [build.md](../specs/build.md#settings-and-flags)).
- **What stops working**: code with `dynamic_cast` or `typeid` fails to compile (for cl and clang-cl, cuno raises their warning to an error). Per-type behaviour is written with virtual functions, or with a value of your own that says the type.

## Building in the C runtime (`runtime = "static"`)

```toml
[build]
runtime = "static"
```

- **What changes**: with MSVC, clang-cl and clang, the C runtime (MSVC's CRT) is built into the executable. Shipping it then needs no runtime DLLs or redistributable package beside it.
- **No change with GCC**: MinGW's GCC always builds its own runtime (libgcc, libstdc++) into the executable whatever this setting says, and uses Windows' C runtime (UCRT). `static` and `dynamic` are the same ([build.md](../specs/build.md#settings-and-flags)).
- **What to watch**: the executable is larger. Code that relies on sharing one runtime, such as allocating memory in one DLL and freeing it in another, has to take care, since each DLL then has a runtime of its own. How it differs from the default `dynamic` is in [manifest.md](../specs/manifest.md#build).

## Link-time optimisation for release (`lto = true`)

```toml
[profile.release]
lto = true
```

- **What changes**: release builds (`cuno build --release`) optimise across translation units at link time.
- **What to watch**: linking takes longer. It is usual to set it for release only, not for the `dev` builds of everyday work.

## Warnings as errors (`warnings-as-errors = true`)

```toml
[profile.release.build]
warnings-as-errors = true
```

- **What changes**: a single warning fails the build (`/WX` for MSVC, `-Werror` for GCC and Clang).
- **Where to write it**: on every build, it stops half-written code from building too. Set for release only, as above, it leaves everyday work alone ([manifest.md](../specs/manifest.md#profilenamebuild)).

## When that is not enough (`[build.<compiler>]`)

A flag with no setting of its own can be written raw, per compiler.

```toml
[build.msvc]
flags = ["/GS-"]
```

cuno passes these flags without understanding them, so a setting that has an item above, such as exceptions or RTTI, is written with that item ([manifest.md](../specs/manifest.md#buildcompiler)).
