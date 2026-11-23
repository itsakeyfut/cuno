# c-in-cpp

A `.c` file in a C++ package is compiled as C, and called from C++ through `extern "C"`.

```sh
cuno build
cuno check
cuno run
```

What each executable prints is in `expected/`, which the repository's tests compare against.
