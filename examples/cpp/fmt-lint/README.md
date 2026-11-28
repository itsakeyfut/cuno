# fmt-lint

`[fmt]` and `[lint]` written for a package that carries vendored code. `src/vendor/` is excluded from both: `cuno fmt --check` passes though `legacy.cpp` is not formatted, and `cuno lint` passes though it uses `0` for a null pointer, a `modernize-use-nullptr` finding that `deny` makes an error everywhere else.

```sh
cuno fmt --check
cuno lint
cuno run
```

What each executable prints is in `expected/`, which the repository's tests compare against.
