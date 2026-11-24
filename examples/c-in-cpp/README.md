# c-in-cpp

A `.c` file in a C++ package is compiled as C, and called from C++ through `extern "C"`. Its tests are written with the C header, `cuno/test.h`, and run in the package's test executable like C++ ones would.

```sh
cuno build
cuno check
cuno run
cuno test
```

What each executable prints is in `expected/`, and `expected/test.txt` holds the `test result:` line of each test executable without its time; the repository's tests compare against both.
