# fmt-lint

`[fmt]` and `[lint]` written for a package that carries vendored code. `src/vendor/` is excluded from both: `cuno fmt --check` passes though `legacy.cpp` is not formatted, and `cuno lint` prints `Linted 1 file: 0 warnings, 0 errors` though `legacy.cpp` uses `0` for a null pointer. That is a `modernize-use-nullptr` finding, which the default preset reports as a warning and `deny` makes an error everywhere outside `src/vendor/`.

```sh
cuno fmt --check
cuno lint
cuno run
```

What each executable prints is in `expected/`, which the repository's tests compare against.
