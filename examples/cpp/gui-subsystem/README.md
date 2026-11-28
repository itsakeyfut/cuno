# gui-subsystem

An executable linked for the Windows subsystem (`[[bin]] subsystem = "windows"`): started from Explorer, it opens no console window. It keeps a plain `main`, which cuno sets as the entry point, so no `WinMain` is needed. This one opens no window either and exits at once, so it prints nothing.

```sh
cuno build
cuno run
```

`expected/gui.txt` is empty, as the program prints nothing, and `expected/gui.pe.txt` says what the executable's header shows: the Windows subsystem. The repository's tests compare against both.
