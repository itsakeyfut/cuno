# layered-settings

Settings in three of the layers `Cuno.toml` stacks, `[build]`, `[platform.<OS>.build]` and `[profile.<name>.build]` (the fourth, `[build.<compiler>]`, is in `per-compiler-flags`): each adds a define, and the program prints which reached it. `[platform.linux.build]` does not apply on Windows. The release profile also turns warnings into errors, and every build links the C runtime into the executable (`runtime = "static"`). Try `cuno run` and `cuno run --release`.

```sh
cuno build
cuno run
cuno run --release
```

What each executable prints is in `expected/`, which the repository's tests compare against; `expected/layered-settings.pe.txt` says what its executable's header shows: a console program with no C runtime DLL to ship.
