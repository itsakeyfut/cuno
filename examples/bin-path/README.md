# bin-path

Executables listed with `[[bin]]`, each with its own `path`, here outside `src/`. Writing `[[bin]]` turns off finding executables by convention.

```sh
cuno build
cuno check
cuno run --bin server
```

What each executable prints is in `expected/`, which the repository's tests compare against.
