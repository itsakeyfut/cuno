# several-bins

Each file in `src/bin/` is an executable of its own, named after the file, beside the default `src/main.cpp`; all of them link the package body. Run one with `cuno run --bin <name>`.

```sh
cuno build
cuno check
cuno run --bin tool
```

What each executable prints is in `expected/`, which the repository's tests compare against.
