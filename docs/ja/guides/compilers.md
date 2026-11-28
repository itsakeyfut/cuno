# コンパイラごとのガイド

cuno は Windows で、MSVC（cl）、Clang（clang と clang-cl）、GCC（MinGW-w64）を使えます。このページは、コンパイラごとに、cuno が見つけられるように入れる方法、cuno に場所を教える方法、ほかのコンパイラと違うところをまとめます。

ここに書くことはどれも仕様（[toolchain.md](../specs/toolchain.md)、[build.md](../specs/build.md#設定とフラグの対応)、[manifest.md](../specs/manifest.md)）にあることです。このページは新しいことを決めず、コンパイラの側から読み直すだけです。

## どのコンパイラが使われるか

コンパイラは、次の順に決まります。上にあるものほど優先されます（[toolchain.md](../specs/toolchain.md#選択)）。

1. コマンドの `--compiler`
2. 環境変数 `CUNO_COMPILER`
3. プロジェクトの `.cuno/config.toml` の `[toolchain] compiler`
4. `Cuno.toml` の `[toolchain] compiler`
5. ユーザー単位の `config.toml` の `[toolchain] compiler`
6. 自動選択（Windows では MSVC）

```toml
# Cuno.toml
[toolchain]
compiler = "clang-cl"   # msvc / clang / clang-cl / gcc
```

- `cuno toolchain list` は、見つかったコンパイラを一覧にし、実行した場所でビルドすると選ばれるものに印を付けます（[commands.md](../specs/commands.md#cuno-toolchain-list)）。

  ```
  msvc          19.52.37029  C:\Program Files\Microsoft Visual Studio\18\Community\VC\Tools\MSVC\14.52.37025\bin\Hostx64\x64\cl.exe  (selected: automatic selection)
  clang         22.1.8       C:\Program Files\LLVM\bin\clang.exe
  clang-cl      22.1.8       C:\Program Files\LLVM\bin\clang-cl.exe
  gcc           16.2.0       C:\msys64\ucrt64\bin\gcc.exe
  clang-format  22.1.8       C:\Program Files\LLVM\bin\clang-format.exe
  clang-tidy    22.1.8       C:\Program Files\LLVM\bin\clang-tidy.exe
  ```

- `cuno doctor --compiler <名前>` は、そのコンパイラでビルドできるかを診断し、問題ごとに対処法を出します（[commands.md](../specs/commands.md#cuno-doctor)）。入れた後は、まずこれで確かめてください。
- `cuno toolchain default <名前>` は、自分のすべてのプロジェクトで使う既定のコンパイラを、ユーザー単位の `config.toml` に書きます（[commands.md](../specs/commands.md#cuno-toolchain-default-コンパイラ)）。プロジェクトが `Cuno.toml` でコンパイラを選んでいれば、そちらが優先されます。

## 場所とバージョンを指定する

### 場所（`[toolchain.paths]`）

cuno はコンパイラを決まった場所から探すので、ふつうは何も書かなくて構いません。別の場所に入れたものを使うときは、マシンごとの設定ファイル（プロジェクトの `.cuno/config.toml` か、ユーザー単位の `config.toml`）に場所を書きます（[manifest.md](../specs/manifest.md#toolchainpaths)）。`Cuno.toml` には書きません。場所はマシンごとに違うためです。

```toml
# .cuno/config.toml
[toolchain.paths]
msvc = "C:/Program Files/Microsoft Visual Studio/18/Community"   # Visual Studio のインストール先
clang = "C:/LLVM/bin/clang.exe"
clang-cl = "C:/LLVM/bin/clang-cl.exe"
gcc = "C:/msys64/ucrt64/bin/gcc.exe"
```

- `msvc` には cl.exe ではなく、Visual Studio（または Build Tools）のインストール先を書きます。cuno がそこから環境を組み立てるためです。
- 書いたものだけが使われます。ほかにもっと新しいものがあっても使いません。書いたものが使えなければ（ファイルがない、バージョンが足りない、`clang` に clang-cl を書いたなど）、その設定を名指ししたエラーになります（[toolchain.md](../specs/toolchain.md#llvmclang--clang-cl--clang-format--clang-tidy)）。

### バージョン（`[toolchain.version]`）

プロジェクトが要るコンパイラのバージョンは、`Cuno.toml` に書きます（[manifest.md](../specs/manifest.md#toolchainversion)）。

```toml
[toolchain.version]
msvc = ">=19.37"   # cl.exe のバージョン
clang = ">=18"
gcc = ">=13"
```

- 確かめるのは、実際に使うコンパイラの制約だけです。`--compiler` で別のコンパイラに切り替えても、関係のない制約では失敗しません。
- 同じコンパイラが複数入っていれば、制約を満たすもののうち最新が選ばれます。プレビュー版とプレリリースは選ばれません（[toolchain.md](../specs/toolchain.md#選択)）。
- MSVC のバージョンは cl.exe のバージョン（`19.37` など）で書きます。Visual Studio の製品バージョン（`17.7` など）ではありません。

### 最低バージョン

| コンパイラ | 最低バージョン | `edition` で要るもの |
|---|---|---|
| MSVC | cl.exe 19.29（VS 2019 16.11） | `c++23` は cl 19.52 以上、`c23` は cl 19.37 以上 |
| Clang | 17、かつ使う MSVC 標準ライブラリが受け付けるもの | — |
| GCC | 10 | `c++23` は GCC 11 以上 |

足りないときは、ビルドの前にエラーになり、要るバージョンが示されます（[toolchain.md](../specs/toolchain.md#コンパイラの最低バージョン)）。

## どのコンパイラでも同じこと

- **コンパイラやリンカが自分で読む環境変数は使いません。** `CL` / `_CL_`（cl と clang-cl）、`LINK` / `_LINK_`（link.exe と lld-link）、`CPATH` / `C_INCLUDE_PATH` / `CPLUS_INCLUDE_PATH`（Clang と GCC）、`LIBRARY_PATH` / `GCC_EXEC_PREFIX` / `COMPILER_PATH`（GCC）、`DEPENDENCIES_OUTPUT` / `SUNPRO_DEPENDENCIES`（GCC）は、ビルド、標準ライブラリの確認、`cuno doctor`、GCC の検出の環境から取り除きます。ビルド系のコマンドは、設定されていたものを 1 つずつ警告します。コマンドラインに現れないフラグで、`Cuno.toml` と違うビルドをしないためです。フラグは `Cuno.toml` に書いてください（[toolchain.md](../specs/toolchain.md#解決の方法)）。

## MSVC（cl）

Windows で何も選ばなければ使われるコンパイラです。

### 入れ方

- **Build Tools for Visual Studio** を、**「C++ によるデスクトップ開発」** のワークロードで入れます（https://visualstudio.microsoft.com/visual-cpp-build-tools/ ）。Visual Studio 本体に同じワークロードを入れても構いません。
- cuno は Visual Studio を vswhere で見つけ、環境（`INCLUDE`、`LIB`、`PATH` など）を自分で組み立てます。Developer Command Prompt を開く必要はなく、`vcvars64.bat` も実行しません（[toolchain.md](../specs/toolchain.md#windows-での環境解決)）。
- 確かめる: `cuno doctor --compiler msvc`。

### ほかのコンパイラと違うところ

- **`cuno check` はビルドと同じコンパイルをします**（リンクだけしない）。cl の構文チェック（`/Zs`）では、値を返さない関数などのコード生成の段階で出る診断が出ず、`check` が成功と言ったのにビルドが失敗することになるためです。そのぶん clang・clang-cl の `check` より遅くなりますが、作ったオブジェクトは続くビルドがそのまま使います（[build.md](../specs/build.md#常に付けるフラグ)）。
- **`exceptions = false` でも、cl は `try` のない `throw` を検出できません。** cuno は、例外を切ったときの cl の警告をエラーに格上げして `try` / `catch` を止めますが、`try` のない `throw` だけは、cl ではどのフラグでも検出できません（[build.md](../specs/build.md#理由)）。
- **`warnings = "pedantic"` と `"all"` の違いは小さい**です。cl では、C++20 以降と C の規格指定がすでに `/permissive-` を含むためです。
- **リンカは `link.exe`** です。`debug-info = true` ではインクリメンタルリンクを切り（`/INCREMENTAL:NO`）、lld と同じく `.ilk` を作らないようにします。

### できないこと

- .NET Framework SDK のヘッダとライブラリ（`metahost.h`、`mscoree.lib` など）と、C++/CLI（`/clr`）・C++/CX（`/ZW`）に要る環境は組み立てません（[toolchain.md](../specs/toolchain.md#解決の方法)）。

## Clang（clang / clang-cl）

Windows の Clang は MSVC と同じ ABI（ビルド対象 `x86_64-pc-windows-msvc`）でビルドするので、MSVC でビルドしたライブラリと混ぜてリンクできます。2 つの名前で使えます。

| | フラグの書き方 | `[build.<compiler>]` のキー |
|---|---|---|
| `clang` | GCC 形式（`-O2`、`-Wall`） | `[build.clang]` |
| `clang-cl` | MSVC 形式（`/O2`、`/W4`） | `[build.clang-cl]` |

### 入れ方

- **LLVM の Windows 向けリリース**を、公式のインストーラで入れます（https://github.com/llvm/llvm-project/releases ）。既定の場所（`%ProgramFiles%\LLVM\bin`）なら、`PATH` に足さなくても見つかります。Visual Studio に同梱された Clang（`VC\Tools\Llvm\x64\bin`）も見つかります（[toolchain.md](../specs/toolchain.md#llvmclang--clang-cl--clang-format--clang-tidy)）。
- **Visual Studio（または Build Tools）も要ります。** Windows の Clang は MSVC のヘッダ、ライブラリ、Windows SDK を使うためです。上の MSVC と同じものを入れてください。
- 確かめる: `cuno doctor --compiler clang`、または `--compiler clang-cl`。Clang が使えれば、C++ のプロジェクトとプロジェクトの外では `Accepted by the MSVC standard library` の行が出ます。
- 既定のビルド対象が MSVC 向け（`*-windows-msvc`）でない Clang（MinGW 向けの Clang など）は使いません。MSVC と混ぜてリンクできないためです。

### ほかのコンパイラと違うところ

- **使える Clang は、Visual Studio の版で決まります。** MSVC の標準ライブラリは、自分が要求する版より古い Clang をエラー（STL1000）にし、その下限は Visual Studio の更新とともに上がります（例: MSVC 14.52 は Clang 22 を要求します）。cuno はビルドの前に標準ライブラリ自身に確かめ、受け付けられなければ止めます（[toolchain.md](../specs/toolchain.md#コンパイラの最低バージョン)）。

  ```
  error: clang 20.1.8 is not accepted by the MSVC standard library in use (STL1000)
  help: install Clang 22 or newer, or select another compiler
  ```

  Visual Studio を更新したら、LLVM も新しくすることになるかもしれません。C だけのプロジェクトでは、この確認をしません。
- **Clang でも MSVC のツールセットを 1 つ使います。** 完全でプレビュー版でない Visual Studio のツールセットのうち、cl が最新のものです。`[toolchain.version] msvc` は MSVC を選んだときだけ確かめるので、Clang が使うツールセットは縛りません。`[toolchain.paths] msvc` を書けば、Clang が使う Visual Studio もそこに限られます（[toolchain.md](../specs/toolchain.md#選択)、[manifest.md](../specs/manifest.md#toolchainpaths)）。
- **リンカは `lld-link`**（LLVM 同梱）です。
- **`warnings = "pedantic"` は、Windows SDK のヘッダを指摘することがあります。** `IID_PPV_ARGS` などが使う `__uuidof` を拡張として警告するので、`warnings-as-errors` と合わせるとエラーになります。cl は指摘しません（[build.md](../specs/build.md#理由)）。
- **`[toolchain.paths]` の `clang` と `clang-cl` は、取り違えるとエラーになります。** 中身は同じ実行ファイルでも、名前で振る舞いが変わり、もう一方の形式のフラグを渡すことになるためです（[toolchain.md](../specs/toolchain.md#llvmclang--clang-cl--clang-format--clang-tidy)）。

## GCC（MinGW-w64）

MSYS2 の UCRT64 の GCC を使います（ビルド対象 `x86_64-w64-mingw32`）。MSVC のツールセットを使わないので、**Visual Studio がなくても使えます**。

### 入れ方

- **MSYS2** を入れ（https://www.msys2.org/ ）、MSYS2 のシェルで UCRT64 の GCC を入れます。

  ```sh
  pacman -S mingw-w64-ucrt-x86_64-gcc
  ```

- 既定の場所（`C:\msys64\ucrt64\bin`）なら、`PATH` に足さなくても見つかります（[toolchain.md](../specs/toolchain.md#mingwgcc)）。
- 確かめる: `cuno doctor --compiler gcc`。
- **C ランタイムが MSVCRT の GCC**（MSYS2 の mingw64 や、古い配布物）は使いません。それしかなければエラーになり、UCRT64 の GCC を入れるよう案内されます。

### ほかのコンパイラと違うところ

- **GCC のランタイムは、いつも実行ファイルに取り込まれます。** GCC のランタイム（`libgcc`、`libstdc++`）とスレッドのライブラリ（`winpthread`）を静的にリンクし、C ランタイム（UCRT）は Windows のものを使います。これらの DLL は Windows に入っていないので、取り込まないと、作った実行ファイルが cuno の外で起動しないためです。`runtime = "static"` と `"dynamic"` は同じになります。MinGW では UCRT を静的にリンクできないためです（[build.md](../specs/build.md#理由)）。
- **`links` に書いたライブラリは静的にしません。** `ld` が見つけたものをリンクするので、たとえば `links = ["pthread"]` は `libpthread.dll.a` をリンクし、作った実行ファイルは `libwinpthread-1.dll` を要します。
- **GCC 14 以上の C++ では、`libstdc++exp` もリンクします。** MinGW の libstdc++ は、C++23 の `std::print` / `std::println` が端末に書く部分をこのライブラリに置いているためです。`std::stacktrace` などもリンクできるようになります。
- **`cuno check` はビルドと同じコンパイルをします**（MSVC と同じ）。配列の範囲外の参照（`-Warray-bounds`）などの診断は、最適化の段階でしか出ないためです。
- **`lto = true` ではコンパイルが遅くなります。** LTO でも最適化の段階の診断が出るように、`-ffat-lto-objects` を付けるためです。
- **`subsystem = "windows"` は `-mwindows`** で、入口の指定は要りません。
- **`exceptions = false` は `-fno-exceptions` だけです。** GCC の標準ライブラリは、それだけで例外を投げない実装を使います（MSVC の標準ライブラリのマクロ `_HAS_EXCEPTIONS` は定義しません）。
- **`g++` は、使う `gcc` と同じディレクトリのものを使います。** `g++` の場所を別に書く設定はありません（[toolchain.md](../specs/toolchain.md#mingwgcc)）。
- **GCC の DLL は、作った実行ファイルの環境に足しません。** cuno は GCC を起動するときだけ GCC のディレクトリを `PATH` に加えます。DLL を要する実行ファイルが、cuno の中でだけ動いてしまわないようにするためです。

### ASCII でないパス

MinGW のアセンブラ（`as`）とリンカ（`ld`）は ANSI コードページで動くため、ASCII でないパスのファイルを開けません。cuno は、プロジェクトの中のパスを相対で渡すなどして、日本語の名前のディレクトリでもビルドできるようにしています。それでも ASCII でないパスを渡すことになる場合は、コンパイルの前にエラーにし、そのパスを示します（[toolchain.md](../specs/toolchain.md#mingwgcc)）。

- 出力先（`target-dir`）のパスが、プロジェクトの中なら cuno がルートからの相対にしても、外ならそのままで、ASCII でない: ASCII のディレクトリにビルドしてください（`--target-dir`、`CUNO_TARGET_DIR`、`[build] target-dir`）。
- `lib-dirs` のディレクトリのパスが、cuno がプロジェクトのルートからの相対にしても ASCII でない: ライブラリを ASCII のディレクトリに置いてください。リンクするとき（`cuno check` 以外）だけです。
- `src/bin/` のファイル名から付く実行ファイルの名前が ASCII でない: ASCII の名前に変えるか、`[[bin]]` に ASCII の名前で書いてください。プログラムをリンクするとき（`cuno build`、`cuno run`）だけです。
- 一時ディレクトリ（`TMP`、なければ `TEMP`）が ASCII でなく 8.3 形式の短い名前もないうえに、LTO を使うか、そのパスを ANSI コードページで表せない: `TMP` と `TEMP` を ASCII のディレクトリにしてください。リンクするときだけです。

一時ディレクトリが ASCII でなくても、短い名前があれば cuno はそれを使います。短い名前がなく、ANSI コードページで表せるパス（日本語版の Windows の日本語のユーザー名など）なら、ふつうのリンクは通るので止めません。ただし、リンクのコマンドラインがとても長い（3 万文字ほど）と、`ld` の文字化けしたパスのエラーで失敗します。それを前もって確かめる確実な方法がないため、cuno はこの場合も止めません。

## fmt と lint

`cuno fmt` と `cuno lint` は、どのコンパイラを選んでいても LLVM の clang-format と clang-tidy を使います（[dev-tools.md](../specs/dev-tools.md)）。MSVC や GCC だけでビルドするプロジェクトでも、fmt と lint を使うなら LLVM を入れてください。clang-format と clang-tidy の最低バージョンは 17 です（[dev-tools.md](../specs/dev-tools.md#ツールのバージョン)）。C++ のプロジェクトで MSVC か Clang を選んでいるときは、clang-tidy にも上の標準ライブラリの確認が当たります（[toolchain.md](../specs/toolchain.md#コンパイラの最低バージョン)）。
