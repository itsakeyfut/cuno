# 設定ファイル

## ファイル構成

| ファイル | 役割 | cargo での対応 |
|---|---|---|
| `cuno.toml` | プロジェクト設定。言語・規格・コンパイラ・テストフレームワーク・依存などを記述 | `Cargo.toml` |
| `.cuno/config.toml` | マシン固有の設定（コンパイラのパス、出力先など） | `.cargo/config.toml` |

利用者が書く設定ファイルはこの 2 つに限る（[overview.md](overview.md) のアンチゴール参照）。ただし次は例外とする。

- ユーザー単位の `<管理ディレクトリ>/config.toml`（既定は `~/.cuno/config.toml`）: `.cuno/config.toml` と同じ形式で、そのマシンの全プロジェクトに効く（下記「2 階層の設定」）。
- ビルドスクリプト（Lua）: 宣言的な設定で書けない処理のための逃げ道。必要な場合だけ置く（[build.md](build.md)）。
- `cuno.lock`: cuno が生成するファイルで、利用者は書かない（[dependencies.md](dependencies.md)）。

## cuno.toml（MVP 範囲）

全体像。コメントは項目の説明と、取りうる値（選択肢がある場合）。

```toml
[package]
name = "mygame"            # パッケージ名（必須）
version = "0.1.0"          # パッケージのバージョン（必須）
lang = "c++"               # 言語（c / c++）
edition = "c++20"          # 言語規格（c++17 / c++20 / c++23 / c11 / c17 / c23）
description = ""           # 説明（任意）
license = ""               # ライセンス（任意、SPDX 表記）

[toolchain]
compiler = "msvc"          # 使用するコンパイラ（msvc / clang / clang-cl / gcc / safec）

[toolchain.version]        # コンパイラごとのバージョン制約（msvc / clang / clang-cl / gcc / safec）。任意
msvc = ">=19.29"
clang = ">=17"

[build]
warnings = "all"           # 警告レベル（default / all / pedantic）
warnings-as-errors = false # 警告をエラーとして扱うか（true / false）
defines = []               # プリプロセッサ定義
include-dirs = []          # 追加のインクルードパス
links = []                 # リンクするシステムライブラリ（例: ["d3d12", "dxgi"]）
lib-dirs = []              # ライブラリの検索パス
link-flags = []            # リンカにそのまま渡すフラグ
runtime = "dynamic"        # C ランタイムのリンク方式（dynamic / static）
exceptions = true          # C++ の例外を使うか（true / false）
rtti = true                # C++ の RTTI を使うか（true / false）
precompiled-header = ""    # プリコンパイルヘッダ（任意）
unity = false              # ユニティビルドを使うか（true / false）

[build.msvc]               # コンパイラごとの生フラグ（msvc / clang / clang-cl / gcc / safec）
flags = []

[platform.windows.build]   # OS ごとの設定（windows / linux / macos / android / wasm）
defines = ["NOMINMAX"]

[profile.dev.build]        # プロファイルごとの設定
defines = ["GAME_DEBUG"]

[[bin]]                    # 実行ファイル（複数可）
name = "mygame"
path = "src/main.cpp"
subsystem = "console"      # 起動形態（console / windows）

[lib]                      # ライブラリ（最大 1 つ）。MVP では未使用
kind = "static"            # ライブラリの種類（static / shared / header-only）
public-headers = ["include"]
sources = ["src"]

[test]
framework = "cuno"         # 使用するテストフレームワーク（cuno / gtest / catch2 / doctest）

[profile.dev]
opt-level = 0              # 最適化レベル（0 / 1 / 2 / 3 / "s"）
debug-info = true          # デバッグ情報を出すか（true / false）
debug-assertions = true    # assert を有効にするか（true / false）
lto = false                # リンク時最適化を使うか（true / false）

[profile.release]
opt-level = 3
debug-info = false
debug-assertions = false
lto = false
```

`[fmt]` / `[lint]` の項目は [dev-tools.md](dev-tools.md) を参照。

### MVP で実装する範囲

MVP 外の項目・値は、指定されたときの挙動で 2 種類に分ける。

**警告して無視する**（無視しても、ビルド結果の意味は変わらないもの）:

- `[build] unity`、`[build] precompiled-header`（既定値の `false` / `""` のままなら、何も無視しないので警告しない）
- `.cuno/config.toml` の `[alias]`、`[build] cache`（MVP ではビルドキャッシュを使わない。既定値 `auto` も MVP では効かない）

**エラーにする**（終了コード 3。無視すると、利用者の意図と違うものが黙ってビルドされるもの）:

- `compiler = "gcc"` / `"safec"`（`--compiler`、`CUNO_COMPILER`、config.toml での指定を含む）。使うコンパイラとして選ばれたときだけエラーにする。上位の層で別のコンパイラに上書きされていればエラーにしない（`[toolchain.version]` が使うコンパイラの制約だけを見るのと同じ）。
- `[test] framework = "gtest"` / `"catch2"` / `"doctest"`
- 独自プロファイル（`inherits`、および `dev` / `release` 以外の `--profile`）
- `[lib]`
- `[[build.rule]]`（カスタムルール。`[platform.<OS>.build]` / `[profile.<name>.build]` の下に書いた場合も同じ）
- `.cuno/config.toml` の `[target.<ビルド対象環境>]`、`--target`
- `[dependencies]`

上記以外（`[platform.<OS>.build]`、`[toolchain.version]` の `msvc` / `clang` / `clang-cl` を含む）は MVP で実装する。`[toolchain.version]` / `[build.<compiler>]` の `gcc` / `safec` キーは、そのコンパイラを使わない限り無害なので、書かれていてもエラーにしない。

### 未知のキー

- `cuno.toml` / `config.toml` に cuno が知らないキーがあれば、**警告**を出して続行する（エラーにしない）。
- 理由: 新しい cuno で追加された項目を古い cuno で読んでも止まらないようにするため（cargo と同じ）。
- 設定ファイルについての警告とエラーには、ファイルと行・列を示す（例: `--> cuno.toml:12:1`）。綴りの誤りなどをすぐ直せるようにするため。

### プロジェクトの探し方とパスの基準

- cuno はカレントディレクトリから親ディレクトリへさかのぼって `cuno.toml` を探し、最初に見つかった場所をプロジェクトのルートとする（cargo と同じ）。サブディレクトリから `cuno build` 等を実行できる。
- 設定ファイルに書いた相対パスは、**そのファイルが置かれている場所**を基準に解決する。
  - `cuno.toml` の `include-dirs` / `lib-dirs` 等: プロジェクトのルート基準。
  - `.cuno/config.toml` の `target-dir` 等: `.cuno/` の親（プロジェクトのルート）基準。
  - ユーザー単位の config.toml: `<管理ディレクトリ>` 基準。
- Windows で、ドライブを書かずに `\` や `/` で始まるパス（例: `/inc`）と、ドライブの後に区切りがないパス（例: `C:inc`）はエラー（終了コード 3）。絶対パスか相対パスで書く。
  - 理由: どちらも、どこを指すかが書いたファイルの場所ではなく、そのときのドライブやカレントディレクトリで変わる。黙って解決すると、意図しない場所を見たままビルドが成功しうる。

## 各項目の説明

### `[package]`

| 項目 | 必須 | 既定値 | 説明 |
|---|---|---|---|
| `name` | ○ | — | パッケージ名。既定の実行ファイル名にもなる |
| `version` | ○ | — | パッケージのバージョン（セマンティックバージョニング） |
| `lang` | | `"c++"` | 言語 |
| `edition` | | C++: `"c++20"` / C: `"c17"` | 言語規格 |
| `description` | | なし | 説明 |
| `license` | | なし | ライセンス（SPDX 表記。例: `"MIT OR Apache-2.0"`） |

`lang` の値:

- `c`: C 言語のプロジェクト。
- `c++`: C++ のプロジェクト。

`edition` の値:

- `c++17`: C++17。C++ の最低サポート規格。
- `c++20`: C++20。C++ の既定。
- `c++23`: C++23。コンパイラが対応している場合のみ。
- `c11`: C11。C の最低サポート規格。
- `c17`: C17。C の既定。
- `c23`: C23。コンパイラが対応している場合のみ。

`lang` と合わない `edition`（例: `lang = "c"` で `edition = "c++20"`）はエラー。

`edition` は `lang` の言語のソースに使う。もう一方の言語のソース（C++ のパッケージの `.c` など）は、その言語の既定の規格でコンパイルする（[project-layout.md](project-layout.md#対象ファイル)）。

### `[toolchain]`

| 項目 | 必須 | 既定値 | 説明 |
|---|---|---|---|
| `compiler` | | 自動選択 | 使用するコンパイラ。選択の優先順位は [toolchain.md](toolchain.md) |
| `version` | | 制約なし | コンパイラごとのバージョン制約（下記 `[toolchain.version]`） |

#### `[toolchain.version]`

コンパイラ名ごとにバージョン制約を書く。**実際に使用するコンパイラの制約だけ**を確認する（`--compiler` 等で別のコンパイラに切り替えても、無関係な制約で失敗しない）。

```toml
[toolchain.version]
msvc = ">=19.29"     # cl.exe のバージョン（VS の製品バージョンではない）
clang = ">=17"
```

- キー: `msvc` / `clang` / `clang-cl` / `gcc` / `safec`。
- 値: 比較演算子 `>=` / `>` / `<=` / `<` / `=` とバージョンの組。カンマ区切りで複数の条件を組み合わせられる（例: `">=17, <19"`）。
- 照合は**書かれた桁まで**比較する（例: `"=19.40"` は `19.40.33811` に一致する。`">=17"` は `17.0.6` に一致する）。
- 書かれていないコンパイラには制約をかけない（最低バージョンは [toolchain.md](toolchain.md) に従う）。

`compiler` の値:

- `msvc`: Microsoft Visual C++（cl.exe）。Windows の自動選択の既定。
- `clang`: Clang（GCC 形式のフラグを使う `clang` / `clang++`）。
- `clang-cl`: MSVC 互換モードの Clang（MSVC 形式のフラグを使う）。
- `gcc`: GCC。MVP 直後に対応。
- `safec`: cuno の開発者が別途開発している実験的コンパイラ（[toolchain.md](toolchain.md)）。主要コンパイラでの成立後に対応。

### `[build]`

| 項目 | 既定値 | 説明 |
|---|---|---|
| `warnings` | `"all"` | 警告レベル |
| `warnings-as-errors` | `false` | 警告をエラーとして扱うか |
| `defines` | `[]` | プリプロセッサ定義（例: `["GAME_DEBUG", "VERSION=3"]`） |
| `include-dirs` | `[]` | 追加のインクルードパス |
| `links` | `[]` | リンクするシステムライブラリ。名前だけ書けば、cuno が各コンパイラの形式に変換する（[build.md](build.md#設定とフラグの対応)）。`.lib` を付けて書くとエラー（終了コード 3） |
| `lib-dirs` | `[]` | ライブラリの検索パス |
| `link-flags` | `[]` | リンカにそのまま渡すフラグ（逃げ道） |
| `runtime` | `"dynamic"` | C ランタイムのリンク方式 |
| `exceptions` | `true` | C++ の例外を使うか（C では無視） |
| `rtti` | `true` | C++ の RTTI を使うか（C では無視） |
| `precompiled-header` | なし | プリコンパイルヘッダとして扱うヘッダのパス |
| `unity` | `false` | ユニティビルドを使うか |
| `target-dir` | `target/` | 出力先（[build.md](build.md)）。`[build]` にだけ書ける（`[platform.<OS>.build]` / `[profile.<name>.build]` には書けない） |

`warnings` の値（フラグは [build.md](build.md#設定とフラグの対応) を参照）:

- `default`: コンパイラの既定の警告のみ。
- `all`: 一般的に推奨される警告を有効にする。既定値。新規プロジェクトは厳しめから始める方が後で楽なため。
- `pedantic`: `all` に加え、規格への厳密な準拠を求める。

`warnings-as-errors`:

- `true`: 警告をエラーにする。CI での利用を想定。
- `false`: 警告は警告のまま。既定値。書きかけのコードでビルドが止まらないようにするため。

`runtime` の値:

- `dynamic`: C ランタイムを DLL としてリンクする。既定値。全プロファイルでリリース版の C ランタイムを使う（デバッグ版は既定にしない。理由は [build.md](build.md#設定とフラグの対応)）。
- `static`: C ランタイムを実行ファイルに取り込む。配布時にランタイムの同梱が不要になる。

`exceptions`:

- `true`: C++ の例外を使う。既定値。
- `false`: 例外を無効にする。ゲーム開発で多い設定。

`rtti`:

- `true`: RTTI（`dynamic_cast` / `typeid`）を使う。既定値。
- `false`: RTTI を無効にする。

### `[build.<compiler>]`

抽象化された設定で足りない場合の逃げ道。コンパイラ名ごとに生のフラグを書く。

| 項目 | 既定値 | 説明 |
|---|---|---|
| `flags` | `[]` | そのコンパイラにそのまま渡すフラグ |

`<compiler>` の値:

- `msvc`: MSVC 使用時のみ渡す。
- `clang`: Clang 使用時のみ渡す。
- `clang-cl`: clang-cl 使用時のみ渡す。MSVC 形式のフラグを受け付けるが、独自のフラグもあるため `msvc` とは分ける。
- `gcc`: GCC 使用時のみ渡す。
- `safec`: safec 使用時のみ渡す。

### `[platform.<OS>.build]`

OS ごとの設定。`[build]` と同じ項目を書ける。

```toml
[platform.windows.build]   # OS（windows / linux / macos / android / wasm）
defines = ["NOMINMAX"]
links = ["d3d12", "dxgi"]
```

`<OS>` の値:

- `windows`: Windows 向けのビルドのとき。
- `linux`: Linux 向けのビルドのとき。
- `macos`: macOS 向けのビルドのとき。
- `android`: Android 向けのビルドのとき。
- `wasm`: WebAssembly 向けのビルドのとき。

- 「どの OS 向けか」はビルド対象環境で判定する（ビルドするマシンの OS ではない）。
- Rust の `cfg(...)` のような条件式は使わない（読みにくくなるため）。名前付きの表だけにする。

### `[profile.<name>.build]`

プロファイルごとの設定。`[build]` と同じ項目を書ける。

```toml
[profile.dev.build]
defines = ["GAME_DEBUG"]   # dev プロファイルのときだけ追加
```

### 設定の重ね合わせ

次の順に重ねる。

1. `[build]`
2. `[platform.<OS>.build]`
3. `[build.<compiler>]`
4. `[profile.<name>.build]`

- リストの項目（`defines`、`include-dirs`、`links`、`lib-dirs`、`link-flags`、`flags`）は、後の設定の内容が**追加**される。
- 単一の値の項目（`warnings`、`warnings-as-errors`、`runtime`、`exceptions`、`rtti`、`precompiled-header`、`unity`）は、後の設定で**上書き**される。
- `flags` は `[build.<compiler>]` にだけ書ける項目。`[platform.<OS>.build]` / `[profile.<name>.build]` の下に `[build.<compiler>]` を入れ子にすることはできない（複雑化を避けるため）。

### `[[bin]]`

| 項目 | 既定値 | 説明 |
|---|---|---|
| `name` | `package.name` | 実行ファイル名 |
| `path` | `src/main.cpp`（C は `src/main.c`） | エントリポイントのソース |
| `subsystem` | `"console"` | 起動形態（Windows のみ意味を持つ） |

`subsystem` の値:

- `console`: コンソールアプリとして起動する。既定値。
- `windows`: コンソールウィンドウを出さずに起動する。ゲームで必須。`WinMain` ではなく通常の `main` のまま書けるよう、cuno がエントリポイントも指定する（[build.md](build.md#設定とフラグの対応)）。

- `[[bin]]` を 1 つも書かない場合: 既定の実行ファイル（`src/main.cpp` / `src/main.c`）と、`src/bin/` 直下の C/C++ ソース（`*.c` / `*.cpp` / `*.cc` / `*.cxx`。1 ファイル = 1 実行ファイル、ファイル名が実行ファイル名）を自動検出する。
- `[[bin]]` を 1 つでも書いた場合: **自動検出はしない**。書いたものだけが実行ファイルになる（既定の `src/main.cpp` が意図せずエントリファイル扱いにならず、`main` が重複するのを防ぐため）。
- 実行ファイルが複数あるとき、`--bin` を付けない `cuno run` はエラー（終了コード 2）にし、選べる名前を表示する。

### `[lib]`

MVP では使わない。項目名のみ予約。

| 項目 | 既定値 | 説明 |
|---|---|---|
| `kind` | `"static"` | ライブラリの種類 |
| `public-headers` | `["include"]` | 公開ヘッダのディレクトリ |
| `sources` | `["src"]` | ソースのディレクトリ |

`kind` の値:

- `static`: 静的ライブラリ（`.lib` / `.a`）。利用側の実行ファイルに取り込まれる。
- `shared`: 動的ライブラリ（`.dll` / `.so` / `.dylib`）。実行時に読み込まれる。
- `header-only`: ヘッダのみのライブラリ。コンパイル成果物を持たない。

### `[test]`

| 項目 | 既定値 | 説明 |
|---|---|---|
| `framework` | `"cuno"` | 使用するテストフレームワーク |

`framework` の値:

- `cuno`: cuno 独自のテスト（C++20 は `cuno/test.hpp`、C / C++17 は `cuno/test.h`）。導入作業なしで使え、ソース内テストにも対応する。既定値。詳細は [testing.md](testing.md)。
- `gtest`: GoogleTest。既存資産を持つ利用者向け。後。
- `catch2`: Catch2。後。
- `doctest`: doctest。後。

### `[profile.<name>]`

標準で `dev`（既定）と `release` を持つ。独自プロファイルは `inherits` で既存のものを継承して作る（[build.md](build.md)）。

| 項目 | `dev` の既定 | `release` の既定 | 説明 |
|---|---|---|---|
| `opt-level` | `0` | `3` | 最適化レベル |
| `debug-info` | `true` | `false` | デバッグ情報を出すか |
| `debug-assertions` | `true` | `false` | `assert` を有効にするか |
| `lto` | `false` | `false` | リンク時最適化を使うか |
| `inherits` | — | — | 継承元プロファイル（独自プロファイルのみ） |

`opt-level` の値（フラグは [build.md](build.md#設定とフラグの対応) を参照）:

- `0`: 最適化なし。
- `1`: 軽い最適化。
- `2`: 標準的な最適化。
- `3`: 最大の最適化。
- `"s"`: サイズ優先の最適化。

`debug-info`:

- `true`: デバッグ情報を出す。
- `false`: 出さない。

`debug-assertions`（cargo と同じ名前）:

- `true`: `assert` を有効にする（`NDEBUG` を定義しない）。
- `false`: `assert` を無効にする（`NDEBUG` を定義する）。

`lto`:

- `true`: リンク時最適化を使う。リンク時間が長くなるため opt-in。
- `false`: 使わない。既定値（cargo と同じ）。

## .cuno/config.toml

### 2 階層の設定

| 階層 | 場所 | 対象 | Git |
|---|---|---|---|
| プロジェクト単位 | `<プロジェクト>/.cuno/config.toml` | そのプロジェクト | 入れない（`cuno new` が `.gitignore` に追加） |
| ユーザー単位 | `<管理ディレクトリ>/config.toml` | そのマシンの全プロジェクト | — |

- どちらもマシン固有の設定を書く。チームで共有する設定は `cuno.toml` に書く。
- 優先順位（上ほど優先）:
  1. コマンド引数
  2. 環境変数
  3. プロジェクト単位の `.cuno/config.toml`
  4. `cuno.toml`（`cuno.toml` でも設定できる項目の場合。例: `compiler`）
  5. ユーザー単位の `<管理ディレクトリ>/config.toml`
  6. 既定値
- プロジェクトで決めた設定（`cuno.toml`）は、個人の全体設定（ユーザー単位の config）より優先する。
- 管理ディレクトリは [distribution.md](distribution.md) を参照。

### プロジェクトの設定と信用

- `cuno build` などのビルド系コマンドと `cuno doctor` は、プロジェクトに置かれた設定（`cuno.toml`、`.cuno/config.toml`）を信用する。`.cuno/config.toml` の `[toolchain.paths]` に書いたツールも、そのまま実行する。
  - 理由: `[build.<compiler>]` の生フラグだけでも、コンパイラにプラグインを読ませるなどして任意のコードを実行させられる。ビルドする時点で、そのプロジェクトはすでに信用されている。ツールのパスだけを制限しても安全にはならず、リポジトリに LLVM を同梱するプロジェクトが使えなくなる。
  - 他人のプロジェクトをビルドする前に、`cuno.toml` と `.cuno/config.toml` を確かめるのは利用者の責任とする。
- 例外として、プロジェクトに置かれた層の `target-dir` はプロジェクトの中に限る（[build.md](build.md#出力先ディレクトリ)）。`cuno clean` は何も実行せずに出力先を削除するため、ビルドとは別の扱いにする。

### 項目

```toml
[build]
target-dir = "D:/fast/target"  # 出力先
jobs = 16                      # 並列数
cache = "auto"                 # ビルドキャッシュ（auto / sccache / ccache / none）

[toolchain]
compiler = "clang"             # このマシンで使うコンパイラ（msvc / clang / clang-cl / gcc / safec）

[toolchain.paths]              # ツールの場所を明示（自動検出に任せるなら不要）
clang = "C:/LLVM/bin/clang.exe"
clang-format = "C:/LLVM/bin/clang-format.exe"
clang-tidy = "C:/LLVM/bin/clang-tidy.exe"

[target.aarch64-linux-android] # ビルド対象環境ごとの設定
ndk = "C:/Android/ndk/27.0"

[term]
color = "auto"                 # 色付け（auto / always / never）
verbose = false                # 詳しく表示するか（true / false）

[alias]                        # コマンドの別名
b = "build"
rr = "run --release"
```

#### `[build]`

| 項目 | 既定値 | 説明 |
|---|---|---|
| `target-dir` | `target/` | 出力先（[build.md](build.md)） |
| `jobs` | CPU コア数 | 並列数 |
| `cache` | `"auto"` | ビルドキャッシュ |

`cache` の値:

- `auto`: sccache → ccache の順に探し、見つかったものを使う。どちらもなければ使わない。既定値。
- `sccache`: sccache を使う。見つからなければエラー。
- `ccache`: ccache を使う。見つからなければエラー。
- `none`: ビルドキャッシュを使わない。

#### `[toolchain]`

| 項目 | 既定値 | 説明 |
|---|---|---|
| `compiler` | なし | このマシンで使うコンパイラ。値は `cuno.toml` の `compiler` と同じ。プロジェクト単位なら `cuno.toml` より優先、ユーザー単位なら `cuno.toml` より劣後する（[toolchain.md](toolchain.md)） |

#### `[toolchain.paths]`

ツール名ごとに場所を書く。書いたものは自動検出より優先される。

- `msvc`: Visual Studio（または Build Tools）の**インストールディレクトリ**を書く（例: `"C:/Program Files/Microsoft Visual Studio/2022/Community"`）。MSVC は cl.exe 単体では動かず、cuno がそのインストールから環境を解決するため（[toolchain.md](toolchain.md)）。
  - 書いた場合、Visual Studio はこのディレクトリのものだけを使う（clang / clang-cl が使う MSVC のツールセットも含む）。ここに C++ x64 ツールを持つ Visual Studio が見つからなければエラー（終了コード 3）。
- `clang` / `clang-cl` / `gcc` / `safec` / `clang-format` / `clang-tidy` / `sccache` / `ccache`: 実行ファイルのパスを書く。

#### `[target.<ビルド対象環境>]`

クロスコンパイル用。ビルド対象環境ごとに、必要なツールの場所などを書く（例: Android の `ndk`、WebAssembly の `emsdk`）。全項目は未決。

#### `[term]`

| 項目 | 既定値 | 説明 |
|---|---|---|
| `color` | `"auto"` | 色付け。値は `--color` と同じ（[commands.md](commands.md)） |
| `verbose` | `false` | 詳しく表示するか |

`verbose`:

- `true`: 常に `-v` 付きと同じ表示にする。
- `false`: 通常の表示。既定値。

#### `[alias]`

コマンドの別名（cargo と同じ）。`別名 = "展開後のコマンド"` の形で書く。既存のコマンド名は上書きできない。

## 未決

- `[target.<ビルド対象環境>]` の全項目
- `[dependencies]` の書式の詳細（方針は [dependencies.md](dependencies.md)）
- カスタムルール `[[build.rule]]` の全項目（方針は [build.md](build.md)）
- ワークスペースの記述方法
