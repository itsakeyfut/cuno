# multi-file-c

Every `.c` file under `src/` other than the entry file is the package body, compiled once and linked into the executable; `src/` is on the include path.

```sh
cuno build
cuno check
cuno run
```

What each executable prints is in `expected/`, which the repository's tests compare against.
