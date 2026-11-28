# gui-subsystem

An executable linked for the Windows subsystem (`[[bin]] subsystem = "windows"`): started from Explorer, it opens no console window. It keeps a plain `main`, so no `WinMain` is needed: cuno makes `main` the entry point on MSVC, clang and clang-cl, and MinGW's start-up code calls it for a GUI program anyway. This one opens no window either and exits at once, so it prints nothing.

```sh
cuno build
cuno run
```

`expected/gui.txt` is empty, as the program prints nothing, and `expected/gui.pe.txt` says what the executable's header shows: the Windows subsystem. The repository's tests compare against both.
