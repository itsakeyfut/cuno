# ビルド

## ビルドエンジン

- **自前のビルドエンジンを持つ**。cuno 単体で動き、Ninja / CMake を必要としない。
- Ninja との選択式は採用しない（2 経路の挙動ずれ・テスト・保守の負担が見合わないため）。
- **ビルドグラフの構築と実行を分離する**。将来 Ninja バックエンドを追加できる余地を残す。

## 差分ビルド・並列ビルド

- ヘッダ依存は、GCC / Clang は `-MD` 系の依存ファイル、MSVC は `/sourceDependencies`（JSON 出力。VS 16.7 以降）から取得する。clang-cl も `/clang:` で clang の `-MD` を使う。
  - clang-cl で `/showIncludes` を使わないのは、`#embed` で読むファイルが出力に現れず、その変更を見逃すため（`-MD` の依存ファイルには現れる）。
  - MSVC で `/showIncludes` を使わないのは、日本語版 Visual Studio では出力が日本語（「メモ: インクルード ファイル:」）になり、言語に依存してしまうため。
- 変更のあったものだけ再コンパイルする。
- 並列コンパイルを行う。

### 変更の判定

- 各手順（ソース 1 つのコンパイル、実行ファイル 1 つのリンク）について、前回成功したときの次の内容を記録する。
  - コマンドライン（引数と、コンパイラの場所・バージョン、cuno が組み立てる環境変数 `INCLUDE` / `LIB` / `LIBPATH`）。環境変数を含めるのは、Windows SDK や MSVC のツールセットが更新されたとき、引数は同じでも読むヘッダとライブラリが変わるため
  - 入力ごとの更新時刻・サイズ・内容のハッシュ。入力は、コンパイルならソースとそれが読んだすべてのヘッダ（上のヘッダ依存）、リンクならオブジェクトファイルと、`links` のうち `lib-dirs` で見つかるライブラリ（`lib-dirs` を順に探し、最初に見つかった `<lib>.lib`）。見つからないもの（`kernel32` などシステムのライブラリ）は、上の `LIB` で識別する
- 次のビルドでは、次のどれかに当てはまる手順だけを実行する。
  - 記録がない、または出力がない
  - コマンドラインが記録と違う（設定、コンパイラ、プロファイルを変えた場合）
  - 入力の更新時刻かサイズが記録と違い、内容のハッシュも記録と違う
- 更新時刻かサイズだけが違い、ハッシュが同じ入力は、変更なしとして記録を更新する。
- 失敗した手順は記録しない。次のビルドで必ず実行する。
- 手順の実行中に更新された入力がある場合も記録しない（コンパイラが読んだのが更新の前か後か分からないため）。次のビルドで実行する。
- `cuno check` の手順（ソース 1 つの確認）も同じように判定する。確認は出力を作らないため、成功したときに cuno が中身のないファイルを出力の代わりに置く。このファイルは記録と同じく cuno 内部のもの。
- 理由:
  - 「入力が出力より新しいか」だけで判定すると、更新時刻の古いファイルに戻した場合（バックアップからの復元など）を見逃し、古いコードのまま成功と報告する。記録した値と比べれば、時刻が戻っても違いとして分かる。
  - 毎回すべてのハッシュを取ると、何も変えていないビルドでも、標準ヘッダや Windows のヘッダ（数千ファイル）を毎回読むことになる。更新時刻とサイズが同じものは読まない。
  - 見逃すのは次の場合で、そのときは古いコードのまま成功と報告する。
    - 更新時刻もサイズも同じで中身だけが違う場合。通常の編集では起きない。
    - インクルードの検索で、記録したヘッダより先に見つかる場所に、同じ名前のヘッダを新しく作った場合（ライブラリも同様）。記録は読んだファイルのもので、読まなかった場所は記録しないため。
- 記録は出力先の中（`target/<プロファイル名>/`）に置く。形式は cuno 内部のもので、利用者が読むものではない。cuno のバージョンが変わった、または読めない記録は、ないものとして扱う（その手順を実行する）。

## 既定のインクルードパス

`cuno.toml` の `include-dirs` とは別に、cuno が常に追加するインクルードパス。

| パス | 理由 |
|---|---|
| `src/` | パッケージ本体のヘッダを `#include "calc.h"` のように参照できるように |
| `include/` | ライブラリ対応後（後）。公開ヘッダ用 |
| 同梱ヘッダの場所（`<管理ディレクトリ>/include/<cuno のバージョン>/`） | **通常ビルドでも常に追加**する。`#include <cuno/test.h>` を `#ifdef CUNO_TEST` で囲まずに書いても、通常ビルドがエラーにならないように |

## 設定とフラグの対応

`cuno.toml` の設定と、cuno が常に付けるフラグを、各コンパイラの引数にどう変換するかの一覧。フラグの対応はここだけに書き、他の文書はここを参照する。`clang` は GNU 形式の Clang（対象は `x86_64-pc-windows-msvc`）。どの対応も、実際の cl・clang-cl・clang で確かめたもの。

### 常に付けるフラグ

| 対象 | MSVC | clang-cl | clang |
|---|---|---|---|
| 常に | `/nologo` `/utf-8` | `/nologo` `/utf-8` `--target=x86_64-pc-windows-msvc` | `--target=x86_64-pc-windows-msvc` |
| C++ のとき | `/Zc:__cplusplus` | `/Zc:__cplusplus` | — |
| ビルド / `check` | `/c` / `/Zs` | `/c` / `/Zs` | `-c` / `-fsyntax-only` |
| 言語（ソースごと） | `/TC` / `/TP` | `/TC` / `/TP` | `-x c` / `-x c++` |
| オブジェクトファイル（ビルドのとき） | `/Fo<obj>` | `/Fo<obj>` | `-o <obj>` |
| ヘッダ依存（ビルドと `check` のとき） | `/sourceDependencies <file>` | `/clang:-MD /clang:-MF<file>` | `-MD -MF <file>` |

- ヘッダ依存のファイル `<file>` は、オブジェクトファイルの拡張子を `.json`（MSVC）または `.d`（clang-cl、clang）に替えたもの。`check` でも同じ名前を使う（オブジェクトを作らない `check` も、同じ仕組みで変更のないソースを確認し直さないため）。
- MSVC の `/Zs` は、コード生成の段階で出る診断を出さない。たとえば、値を返さない関数（C4716）は既定の設定で、未初期化の変数（C4700・C4701）と、値を返さない経路のある関数（C4715）は `warnings-as-errors` のときに、ビルドでは失敗になるが `check` では出ない。そのため MSVC では、`check` が成功してもビルドが失敗することがある。clang-cl と clang の `/Zs`・`-fsyntax-only` はこれらを出す。
  - 理由: `check` の速さは、コード生成をしないことから来る。MSVC で結果をビルドと揃えるにはコンパイルして捨てるしかなく、`check` がビルドとほぼ同じ時間になる。
- この表のフラグ（言語、ソース、オブジェクトファイル、ヘッダ依存）は、`[build.<compiler>] flags` の後に付ける。依存ファイルを cuno が読むため、利用者の `-MF` 等より cuno のものを優先する。依存ファイルに複数の規則がある場合（利用者の `-MP` など）は最初の規則だけを読む。

### コンパイル

| 設定 | MSVC | clang-cl | clang |
|---|---|---|---|
| `edition` `c++17` / `c++20` / `c++23` | `/std:c++17` / `/std:c++20` / `/std:c++23`（cl 19.52 以上） | `/std:c++17` / `/std:c++20` / `/clang:-std=c++23` | `-std=c++17` / `-std=c++20` / `-std=c++23` |
| `edition` `c11` / `c17` / `c23` | `/std:c11` / `/std:c17` / `/std:clatest`（cl 19.37 以上） | `/std:c11` / `/std:c17` / `/clang:-std=c2x` | `-std=c11` / `-std=c17` / `-std=c2x` |
| `warnings` `default` / `all` / `pedantic` | なし / `/W4` / `/W4 /permissive-` | なし / `/W4` / `/W4 -Wpedantic` | なし / `-Wall -Wextra` / `-Wall -Wextra -Wpedantic` |
| `warnings-as-errors = true` | `/WX` | `/WX` | `-Werror` |
| `defines`、テストのビルド（`CUNO_TEST`） | `/D<define>` | `/D<define>` | `-D<define>` |
| インクルードパス（`src/`、同梱ヘッダの場所、`include-dirs` の順） | `/I<dir>` | `/I<dir>` | `-I<dir>` |
| `runtime` `dynamic` / `static` | `/MD` / `/MT` | `/MD` / `/MT` | `-fms-runtime-lib=dll` / `-fms-runtime-lib=static` |
| `exceptions`（C++）`true` / `false` | `/EHsc` / `/we4530 /D_HAS_EXCEPTIONS=0` | `/EHsc` / `/D_HAS_EXCEPTIONS=0` | なし / `-fno-exceptions -D_HAS_EXCEPTIONS=0` |
| `rtti = false`（C++） | `/GR- /we4541` | `/GR- -Werror=rtti` | `-fno-rtti` |
| `opt-level` `0` / `1` / `2` / `3` / `"s"` | `/Od` / `/O2` / `/O2` / `/O2` / `/O1` | MSVC と同じ | `-O0` / `-O1` / `-O2` / `-O3` / `-Os` |
| `debug-info = true` | `/Z7` | `/Z7` | `-g` |
| `debug-assertions = false` | `/DNDEBUG` | `/DNDEBUG` | `-DNDEBUG` |
| `lto = true` | `/GL` | `-flto` | `-flto` |
| `[build.<compiler>] flags` | 設定から作るフラグの後にそのまま | 設定から作るフラグの後にそのまま | 設定から作るフラグの後にそのまま |

### リンク

リンクはコンパイラのドライバ経由で行う（各ドライバが C ランタイム等のライブラリを自分で加えるため）。

| 設定 | MSVC | clang-cl | clang |
|---|---|---|---|
| 常に | `/nologo` | `/nologo` | `--target=x86_64-pc-windows-msvc` |
| リンカ | `link.exe`（`/link` の後に引数） | `-fuse-ld=lld`、`lld-link`（`/link` の後に引数） | `-fuse-ld=lld` |
| 出力 | `/Fe:<exe>` | `/Fe:<exe>` | `-o <exe>` |
| `links` | `<lib>.lib` | `<lib>.lib` | `-l<lib>` |
| `lib-dirs` | `/LIBPATH:<dir>` | `/LIBPATH:<dir>` | `-L<dir>` |
| `subsystem = "console"` | `/SUBSYSTEM:CONSOLE` | `/SUBSYSTEM:CONSOLE` | `-Wl,/subsystem:console` |
| `subsystem = "windows"` | `/SUBSYSTEM:WINDOWS /ENTRY:mainCRTStartup` | `/SUBSYSTEM:WINDOWS /ENTRY:mainCRTStartup` | `-Wl,/subsystem:windows -Wl,/entry:mainCRTStartup` |
| `debug-info = true` | `/DEBUG /INCREMENTAL:NO` | `/DEBUG` | `-g` |
| `lto = true` | `/LTCG` | `-flto`、`/opt:lldlto=<n>` | `-flto -Wl,/opt:lldlto=<n>` |
| `link-flags` | `/link` の後に最後にそのまま | `/link` の後に最後にそのまま | ひとつずつ `-Xlinker <flag>` として最後に |

- `<n>` は `opt-level`（`0` / `1` / `2` / `3`、`"s"` は `2`）。

### 理由

- `/utf-8`: 日本語版 Windows で UTF-8 のソースが CP932 として読まれるのを防ぐ。
- `/Zc:__cplusplus`: これがないと cl は `/std:c++20` でも `__cplusplus` を `199711` にする。
- `--target=x86_64-pc-windows-msvc`: MSVC と同じ ABI でビルドし、MSVC でビルドしたライブラリと混ぜてリンクできるようにする（[toolchain.md](toolchain.md)）。
- `/TC` / `/TP`、`-x c` / `-x c++`: 各ソースを、拡張子で決まる言語（[project-layout.md](project-layout.md#対象ファイル)）として明示してコンパイルする。パッケージの言語と違うファイルには、その言語の既定の規格を使い、C++ だけの設定（例外・RTTI）は付けない。それ以外の設定（`defines`、警告、`[build]` と `[build.<compiler>]` の `flags` など）は、言語に関係なくすべてのソースに渡す。C++ だけのフラグを `flags` に書くと、C のソースではコンパイラが無視するか、警告やエラーを出す。
- `c++23` と `c23` は、どのコンパイラでも規格を名前で指定する。「最新」（`/std:c++latest`、`/std:clatest` を C++ に使うこと）は使わない。cl 19.52 と clang-cl 22 の `/std:c++latest` はすでに C++26 の作業草案（`__cplusplus` が `202400`）で、同じ `cuno.toml` でも clang とは違う規格になり、通るプログラムも変わるため。
  - cl は知らない `/std` を警告だけ出して無視し、古い規格のままコンパイルする。そのため、規格を受け付けるかは cl のバージョンで判定し、足りなければエラーにする（[toolchain.md](toolchain.md)）。`/std:c++23` は確かめた最も古い cl 19.52 以上（受け付ける版が公式の文書にないため。より古い版で確かめられれば下げる）、`/std:clatest`（C の最新、C23 を含む）は Visual Studio 2022 17.7（cl 19.37）以上。
  - clang-cl は `/std:c++23` と `/std:c23` を無視するため、`/clang:` で clang の指定を渡す。C23 は Clang 17 でも通る `c2x` と書く（`c23` は Clang 18 から）。
- `-fms-runtime-lib`: GNU 形式の `clang` は、指定しないと静的な C ランタイムを使い、`runtime = "dynamic"` の既定と食い違う。効くのはコンパイル時だけ（どの C ランタイムを使うかはオブジェクトに記録され、リンカはそれに従う）なので、リンクには付けない。
- `/SUBSYSTEM:CONSOLE`: 指定しないと、リンカは `main` がなく `WinMain` があるプログラムを GUI として黙ってリンクし、コンソールのないプログラムができる。明示すると `main` がないことがリンクエラーとして分かる。
- `-Xlinker`: `-Wl,` は値をカンマで分けるため、`/OPT:REF,ICF` のようなカンマを含むリンカのフラグが壊れる。
- `/opt:lldlto=<n>`: LTO ではリンク時に最適化し直す。lld はこれを指定しないと `opt-level` によらず `2` で最適化する。
- `/INCREMENTAL:NO`: MSVC のリンカは `/DEBUG` で既定でインクリメンタルリンクになり、`.ilk` を作る（lld はしない）。コンパイラによらず同じ出力にするため。
- `_HAS_EXCEPTIONS=0`: 例外を無効にしたとき、コンパイラによらず標準ライブラリが例外を投げない実装を使うようにする。
- `/we4530`、`/we4541`、`-Werror=rtti`: 例外や RTTI を無効にしたとき、clang は `try` / `throw` や `typeid` / `dynamic_cast` をエラーにするが、cl と clang-cl は警告だけ出して、デストラクタが実行されない・実行時に落ちるプログラムを作る。どのコンパイラでもビルドの段階で止めるため。ただし cl は、`try` のない `throw` だけはどのフラグでも検出できない。
- `pedantic` は規格への厳密な準拠を求めるため、clang・clang-cl は Windows SDK の `IID_PPV_ARGS` 等が使う `__uuidof` を拡張として指摘する（`warnings-as-errors` と合わせるとエラー）。cl は指摘しない。cl では `/std:c++20` 以降と C の規格指定がすでに `/permissive-` を含むため、`pedantic` と `all` の違いは小さい。
- `-fuse-ld=lld`: clang-cl は指定しないと `link.exe` を使い、LTO を受け付けない。
- `debug-info` に `/Zi` を使わないのは、並列ビルドで PDB への書き込みが競合し、sccache でキャッシュできないため。`clang -g` は CodeView を出し、`/DEBUG` 相当でリンクする。
- `opt-level = 1` は MSVC に同等の段階がないため `/O2`。
- デバッグ版の C ランタイム（`/MDd`）は既定にしない。ビルド済みの依存と混ぜるとリンクエラーになるため（Rust と同じ方針）。

## カスタムビルド処理（3 層構造）

C/C++ のコンパイル以外の処理（シェーダのコンパイル、コード生成、アセット変換など）は次の 3 層で扱う。上の層で書けるものは上の層で書く。MVP は 1 層目のみ。2 層目（カスタムルール）と 3 層目（ビルドスクリプト）は「後」。

1. **`cuno.toml`（宣言的な設定）**: ソース、define、フラグ等。大半のプロジェクトはここだけで済む。
2. **カスタムルール（`cuno.toml`）**: 「このファイル群を、このコマンドで、この出力に変換する」を宣言的に書く。依存追跡（depfile 対応）・実行順序・差分判定は cuno のビルドエンジンが担う。
   ```toml
   [[build.rule]]
   inputs  = "shaders/**/*.glsl"
   command = "glslc {in} -o {out} -MD -MF {depfile}"
   output  = "{name}.spv"
   ```
3. **ビルドスクリプト（逃げ道）**: 1・2 で書けない処理のみ。**Lua** で書く（例: `build.lua`）。cuno 本体に Lua を組み込み、cuno が提供する API を通じてビルドに介入する。
   - スクリプトから実行できる操作は cuno が制限できる（サンドボックス）。
   - 依存パッケージのビルドスクリプトは、利用者が `cuno.toml` で許可したものだけ実行する。

### 採用理由

- 宣言的な設定だけでは複雑なケースに対応できないため、逃げ道が必要（Rust の build.rs、Zig の build.zig と同じ考え方）。
- CMake のカスタム処理のペイン（依存追跡、実行順序、「黙って何もしない」）は、2 層目で cuno が吸収する。
- 3 層目は「まれに使う逃げ道」なので、「新しい言語を覚えなくてよい」という C/C++ で書く利点は効きにくい。一方、Lua の利点はスクリプトを書かない利用者にも毎回効く:
  - **安全性**: 他人が作った依存パッケージのスクリプトを、操作を制限して安全に実行できる。
  - **待ち時間なし**: コンパイル不要ですぐ実行できる。
  - **クロスコンパイルが楽**: ビルドするマシン用のコンパイラを別途用意する必要がない。
  - **簡潔**: 文字列・ファイル操作を短く書ける。
  - **馴染み**: ゲーム開発で広く使われている言語。
  - **実装が単純**: Rust への組み込みが容易。

### 既知の代償と対策

| 代償 | 対策 |
|---|---|
| C/C++ とは別の言語を書く必要がある | 1・2 層で大半を済ませ、スクリプトを書く場面自体を減らす |
| 型チェック・補完が弱い | 型付き Lua 系（Luau 等）の採用や、API の型定義ファイル提供を検討 |
| C/C++ ライブラリをビルド処理で直接使えない | 外部ツールの実行は 2 層目のカスタムルール、またはスクリプトからのコマンド実行で行う |
| 言語を組み込むことによる将来のバージョン管理 | 採用する Lua の版を固定し、cuno の API はバージョン付きで提供する |

### 経緯

当初は Rust（build.rs）・Zig（build.zig）に倣い、プロジェクトと同じ言語（`build.cpp` / `build.c`）で書く案だった。C/C++ で書く利点は「新しい言語を覚えなくてよい」「型チェック・補完・デバッガが効く」こと。しかし 3 層構造では 3 層目は「まれに使う逃げ道」であり、書く機会が少ないためこの利点は効きにくい。一方 Lua の利点（安全性・待ち時間なし・クロスコンパイルが楽）は、スクリプトを書かない利用者にも毎回効く。この再評価により Lua に変更した。

## 出力

- **compile_commands.json を常に出力する**（clang-tidy とエディタ補完のため）。
  - 置き場所は**プロジェクト直下**（`<プロジェクト>/compile_commands.json`）。`cuno new` が `.gitignore` に追加する。
  - 理由: clangd が自動で探すのは「ソースの親ディレクトリ」とその `build/` サブディレクトリだけで、`target/` 内は探さない。直下に置けば、設定なしで clangd が動く（Rust の rust-analyzer と同じ体験）。利用者が書く設定ファイルではなく生成物なので、「設定ファイルを増やさない」方針には触れない。
  - 検討した他の方式: `target/` 内に置き cuno が `.clangd` を生成する（設定ファイルが増える）、利用者がエディタで場所を設定する（手間が利用者に移る）。
  - **生成はコンパイルと独立**: cuno がビルドグラフ（実行予定のコンパイルコマンド）から生成する。コンパイルの成否に関係なく、`build` / `run` / `check` / `test` / `lint` のいずれを実行しても最新の内容に更新される。`fmt` では生成しない（コンパイラなしで動くため。[dev-tools.md](dev-tools.md#対象ファイル)）。まだ一度もビルドしていなくても `cuno lint` が動く。
  - **単体で解決できる内容にする**: MSVC / Windows SDK のインクルードパス（clang-cl 形式なら `/imsvc` 等）を明示的に書く。Developer Command Prompt の外で動く clangd でも、標準ヘッダや Windows のヘッダを見つけられるようにするため。
  - **内容は常に固定**: 「`dev` プロファイル + `CUNO_TEST` を定義した状態」のコマンドを書く。どのプロファイルでビルドしても内容は変わらない。
    - 理由: エディタの補完・clang-tidy がソース内テストのコードも解析できる（rust-analyzer がテストコードも解析するのと同じ）。ビルドのたびに内容が変わると、エディタの解析結果が揺れるため。

### 出力先ディレクトリ

- 既定は `target/`。中身はビルド対象環境（target）ごとに整理する。
  - 通常: `target/<プロファイル名>/`。ただし `dev` プロファイルは cargo と同じく `debug/` とする（`target/debug/`、`target/release/`、独自プロファイルは `target/profiling/` など名前のまま）。
  - クロスコンパイル時: `target/<ビルド対象環境>/<プロファイル名>/`
  - テストビルド: `target/<プロファイル名>/test/<パッケージ名>/`。`CUNO_TEST` を定義したオブジェクトが通常ビルドのものと混ざらないように分ける。パッケージ名を入れるのは中間ファイルと同じ理由。
  - 中間ファイル（オブジェクト等）: `target/<プロファイル名>/obj/<パッケージ名>/`。出力先を複数プロジェクトで共有しても衝突しないようにする。
- `cuno clean`（指定なし）は、cargo と同じく出力先全体を削除する。
- ビルド（`build` / `run` / `check`）は、出力先を作ったとき、または空の出力先を使い始めたときに、その直下に印のファイル `.cuno-target` を置く。中身のある既存のディレクトリには置かない。`cuno clean` は、この印のない出力先を削除しない。
  - 理由: 出力先がプロジェクトの中を指していても、`src` や `.git` のように cuno が作ったのではないディレクトリかもしれない。名前の決まりでは防ぎきれないため、cuno が自分で作ったと分かるものだけを削除する。
- 出力先は利用者が変更できる。優先順位:
  1. コマンド引数 `--target-dir`
  2. 環境変数 `CUNO_TARGET_DIR`
  3. プロジェクト単位の `.cuno/config.toml`
  4. `cuno.toml` の `[build] target-dir`
  5. ユーザー単位の `<管理ディレクトリ>/config.toml`
  6. 既定値 `target/`
- 出力先が空、またはプロジェクトのルートかその上のディレクトリ（プロジェクトの外では、カレントディレクトリかその上）を指す場合はエラー（終了コード 3）。どの層で指定しても同じで、優先順位で採用されない層の値も確かめる（`--target-dir` があっても、`cuno.toml` の `target-dir = "."` はエラー）。
  - 理由: 書き方（`""`、`"."`、`"a/.."`）や、どの層が採用されるかで結果が変わると、ほかの層の指定を外したときに初めてエラーになる。
  - 理由: `cuno clean` は出力先全体を削除するため。`target-dir = "."` のような指定を許すと、プロジェクトのソースごと消えてしまう。
- プロジェクトに置かれた層（`cuno.toml`、`.cuno/config.toml`）の `target-dir` は、プロジェクトの中を指さなければならない。外を指す場合はエラー（終了コード 3）。採用されない層でも同じ。エラーは書いたファイルと行を示し、``error: `build.target-dir` in .cuno/config.toml points outside the project: <パス>`` と `help: set it in the user-level config, CUNO_TARGET_DIR or --target-dir to put the output outside the project` を出す。プロジェクトの外に置きたい場合は、ユーザー単位の config、環境変数 `CUNO_TARGET_DIR`、`--target-dir` で指定する。
  - 理由: これらのファイルはリポジトリと一緒に配られうる。`cuno clean` は何も実行せずに出力先を削除するため、クローンしたプロジェクトがプロジェクトの外のディレクトリを消させることを防ぐ。ビルドがプロジェクトの設定を信用することについては [manifest.md](manifest.md#プロジェクトの設定と信用) を参照。
- `cuno new` は出力先を `.gitignore` に追加する。

## プロファイル

- 標準で `dev`（既定）と `release` を持つ。`--release` で切り替える。
- MVP では `--profile` に `dev` / `release` だけを受け付ける。それ以外はエラー（終了コード 3）。独自プロファイルは「後」。
- 独自プロファイルを、既存のものを継承して作れる（cargo と同じ）。
  ```toml
  [profile.profiling]
  inherits = "release"   # 継承元のプロファイル（dev / release / 他の独自プロファイル）
  debug-info = true
  ```
- 各項目と値の説明は [manifest.md](manifest.md) の `[profile.<name>]` を参照。

## ビルド設定

- **define / include パス**: `cuno.toml` に書く。
  ```toml
  [build]
  defines = ["GAME_DEBUG"]
  include-dirs = ["third_party/include"]
  ```
- **生フラグの逃げ道**: 抽象化された設定で足りない場合、コンパイラ系統ごとに生のフラグを書ける。
  ```toml
  [build.msvc]
  flags = ["/Zc:preprocessor"]
  ```
- **プリコンパイルヘッダ**（後）: opt-in。コンパイラごとの違いは cuno が吸収する。
  ```toml
  [build]
  precompiled-header = "src/pch.h"
  ```
- **ユニティビルド**（後）: opt-in。大規模プロジェクトのフルビルド高速化用。
- **ビルドキャッシュ**（後）: sccache / ccache を自動で使う設定を持つ。MVP ではビルドキャッシュを使わない。独自キャッシュ（複数マシンでの共有など）は将来。

## 未決

- 各設定の正確な項目名は [manifest.md](manifest.md) を正とする（上の例は概要）
- カスタムルールの全項目
- ビルドスクリプトのファイル名・Lua の版（Lua 5.4 / LuaJIT / Luau 等）・API
