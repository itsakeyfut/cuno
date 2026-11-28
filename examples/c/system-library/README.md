# system-library

A Windows system library, linked by name in `[build] links`: `shlwapi` becomes `shlwapi.lib` for MSVC and clang-cl and `-lshlwapi` for clang and GCC, so the one name serves every compiler. The program prints the extension `PathFindExtensionA` finds in `notes.txt`.

```sh
cuno build
cuno run
```

What each executable prints is in `expected/`, which the repository's tests compare against.
