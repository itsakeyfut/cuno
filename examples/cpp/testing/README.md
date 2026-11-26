# testing-cpp

Tests in C++20, written with `cuno/test.hpp`, which cuno bundles. Unit tests sit in `src/text.cpp` beside the code they check, inside `#ifdef CUNO_TEST`, so they can reach its unnamed namespace and are left out of `cuno build`. The integration test in `tests/trim.cpp` uses the package through its header, and is a test executable of its own.

```sh
cuno build
cuno check
cuno run
cuno test
```

What each executable prints is in `expected/`, and `expected/test.txt` holds the `test result:` line of each test executable without its time; the repository's tests compare against both.
