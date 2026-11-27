# per-compiler-flags

Flags for one compiler only, in `[build.msvc]`, `[build.clang]`, `[build.clang-cl]` and `[build.gcc]`. The program prints which compiler built it. Try `cuno run --compiler clang` or `cuno run --compiler gcc`.

```sh
cuno build
cuno check
cuno run
```

What each executable prints is in `expected/`, which the repository's tests compare against.
