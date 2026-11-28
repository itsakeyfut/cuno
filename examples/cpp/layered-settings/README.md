# layered-settings

Settings in three of the layers `Cuno.toml` stacks, `[build]`, `[platform.<OS>.build]` and `[profile.<name>.build]` (the fourth, `[build.<compiler>]`, is in `per-compiler-flags`): each adds a define, and the program prints which reached it. `[platform.linux.build]` does not apply on Windows. The release profile also turns warnings into errors: the program has none, so both profiles build, and a warning added to `main.cpp` would fail only `cuno build --release`. `runtime = "static"` links the C runtime into the executable on MSVC, clang and clang-cl. MinGW's GCC always links its own runtime in and uses Windows' C runtime, so there the setting changes nothing. Try `cuno run` and `cuno run --release`.

```sh
cuno build
cuno run
cuno run --release
```

What each executable prints is in `expected/`, which the repository's tests compare against; `expected/layered-settings.pe.txt` says what its executable shows: a console program that needs no runtime DLL shipped beside it.
