# testing-c

Tests in C, written with `cuno/test.h`, which cuno bundles. Unit tests sit in `src/stats.c` beside the code they check, inside `#ifdef CUNO_TEST`, so they can reach its `static` function and are left out of `cuno build`. The integration test in `tests/averages.c` uses the package through its header, and is a test executable of its own.

```sh
cuno build
cuno check
cuno run
cuno test
```

What each executable prints is in `expected/`, and `expected/test.txt` holds the `test result:` line of each test executable without its time; the repository's tests compare against both.
