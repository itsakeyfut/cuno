# 開発ツール

## コマンド

| コマンド | 中身 | cargo での対応 |
|---|---|---|
| `cuno fmt` | clang-format を包む | `cargo fmt` |
| `cuno lint` | clang-tidy を包む（compile_commands.json を利用） | `cargo clippy` |

## 設定

- フォーマッタ / リンタの設定は `cuno.toml` の `[fmt]` / `[lint]` に書く。利用者は `.clang-format` / `.clang-tidy` を書かない。cuno が設定を各ツールに渡す。
- **エディタ用の生成物**: cuno は `cuno.toml` の `[fmt]` / `[lint]` から `.clang-format` / `.clang-tidy` を生成し、プロジェクト直下に置く（`cuno new` が `.gitignore` に追加する）。
  - 理由: エディタが clangd 等で整形・解析すると、これらのファイルがない場合は LLVM の既定（インデント 2 等）で動き、`cuno fmt` / `cuno lint` と結果が食い違うため。
  - 正は常に `cuno.toml`。生成物を利用者が編集しても、次の生成で上書きされる。利用者が書く設定ファイルではないため、「設定ファイルを増やさない」方針には触れない（compile_commands.json と同じ扱い。[build.md](build.md)）。
  - `build` / `run` / `check` / `test` / `lint` / `fmt` の実行時に生成する（`fmt --check` は何も書き込まないので生成しない）。compile_commands.json を生成するコマンドは [build.md](build.md#出力) が決め、`fmt` は含まない。

## 実装方針

- **フォーマッタ**: 当初は clang-format を包む。将来、自作の実装に差し替えられる設計にする（字句解析 + 整形ルールで実現でき、C++ の完全な意味解析は不要なため、長期的には自作が現実的）。
- **リンタ**: clang-tidy を包み続ける（多くのチェックが C++ の完全な意味解析を必要とし、自作は非現実的なため）。cuno 独自の軽量チェックを追加できるようにする。

## 対象ファイル

| コマンド | 対象 |
|---|---|
| `cuno fmt` | パッケージのコードの置き場（[project-layout.md](project-layout.md#パッケージのコードの置き場)）にある C/C++ ファイル（`[fmt] exclude` を除く） |
| `cuno lint` | パッケージのコードの置き場にある C/C++ ファイル（`[lint] exclude` を除く）。ソースは compile_commands.json のコマンドで、ヘッダはそれを含むソースを通して解析する。テストコード（`CUNO_TEST` を有効にした状態）も解析する |

- 対象の拡張子: `.c` / `.h` / `.cpp` / `.cc` / `.cxx` / `.hpp` / `.hh` / `.hxx` / `.inl`。
- `.h` は `[package] lang` の言語として扱う（cuno 既定スタイルのポインタ位置が C と C++ で異なるため）。
- `cuno fmt` と `cuno lint` は、同じ「パッケージのコードの置き場」から対象を決める。`[[bin]] path` で `src/` の外に置いたファイルとその隣のヘッダも対象になる。対象の一覧は cuno 本体が作り、fmt / lint の実装に渡す（[architecture.md](architecture.md#クレート間の依存)）。
  - 理由: fmt と lint で対象の決め方が違うと、同じファイルが一方では解析され他方では整形されない。固定のディレクトリだけを見ると、`src/` の外に置いたファイルが `build` と `lint` では使われるのに、未整形のまま `fmt --check` を通る。配置を強制しない方針（[concept.md](../concept.md) の設計原則 3）とも合わない。
- `cuno fmt` はコンパイラなしで動く。対象を決めるのにコンパイラの検出も選択も使わない。
  - 理由: `fmt` は clang-format だけで動き、コンパイラが入っていない環境（整形だけを確かめる CI など）でも使えるべきため。compile_commands.json を `fmt` で生成しないのも同じ理由。
- `cuno lint` は `--release` / `--profile` を受け付けない。指定すると不明なオプションとして終了コード 2。`--compiler` は受け付け、そのコンパイラ向けのコマンドで解析する。
  - 理由: compile_commands.json の内容は常に dev 固定なので（[build.md](build.md#出力)）、受け付けても効かない。受け付けて何もしないと、利用者は release の設定で解析したと思い込む。拒否すればすぐ気づく。

## `[fmt]` / `[lint]` の項目

方針: rustfmt に倣い、**調整項目は少なく**する。clang-format の全オプションをそのまま公開しない（将来の自作フォーマッタへの移行を楽にするため）。

```toml
[fmt]
style = "cuno"         # 基本スタイル（cuno / llvm / google / microsoft / mozilla / webkit / chromium / gnu）
indent-width = 4       # インデント幅
column-limit = 100     # 1 行の最大文字数
use-tabs = false       # タブでインデントするか（true / false）
exclude = []           # 整形しないファイル（例: ["third_party/**"]）

[fmt.clang-format]     # 逃げ道: clang-format のオプションを直接指定（clang-format 使用時のみ有効）
# PointerAlignment = "Left"

[lint]
preset = "recommended" # チェックの基本セット（none / recommended / strict）
allow = []             # 無効にするチェック
warn = []              # 警告にするチェック
deny = []              # エラーにするチェック
exclude = []           # 解析しないファイル
```

### `[fmt]`

| 項目 | 既定値 | 説明 |
|---|---|---|
| `style` | `"cuno"` | 基本スタイル |
| `indent-width` | 選んだ `style` に従う（`cuno` なら 4） | インデント幅 |
| `column-limit` | 選んだ `style` に従う（`cuno` なら 100） | 1 行の最大文字数 |
| `use-tabs` | 選んだ `style` に従う（`cuno` なら `false`） | タブでインデントするか |
| `exclude` | `[]` | 整形しないファイルのパターン |

`style` の値:

- `cuno`: cuno の既定スタイル（下記）。既定値。
- `llvm`: LLVM プロジェクトのスタイル（インデント 2、80 文字）。
- `google`: Google のスタイル（インデント 2、80 文字）。
- `microsoft`: Microsoft のスタイル（インデント 4、波括弧を次の行に置く Allman 形式）。Visual Studio / Unreal Engine に近い見た目を好む人向け。
- `mozilla`: Mozilla のスタイル。
- `webkit`: WebKit のスタイル。
- `chromium`: Chromium のスタイル。
- `gnu`: GNU のスタイル。

`indent-width` / `column-limit` / `use-tabs` は、**書いた場合だけ**選んだ `style` の値を上書きする。書かなければ `style` の値を使う（例: `style = "llvm"` だけならインデント 2、80 文字）。上の TOML 例の 4 / 100 / false は `style = "cuno"` の値。

`use-tabs`:

- `true`: タブでインデントする。
- `false`: スペースでインデントする。`cuno` スタイルの値（GitHub やレビューツールなど、どこで見ても同じ見た目になるため）。

`[fmt.clang-format]` は、clang-format のオプションを直接指定する逃げ道。clang-format を使っている間だけ有効で、自作フォーマッタへの移行後は無視される（または対応する設定へ変換する）。

### cuno 既定スタイル（`style = "cuno"`）

| 項目 | 内容 | 理由 |
|---|---|---|
| ベース | LLVM | clang-format 本来のスタイルで最も検証されている。調整を加えたときの副作用が少ない |
| インデント | 4（スペース） | 入れ子の深さが分かりやすい。Visual Studio の既定・Rust と同じ |
| 1 行の最大文字数 | 100 | 80 はインデント 4 と C++ の長い型名で窮屈。120 は差分の左右表示ではみ出しやすい。rustfmt と同じ |
| 波括弧 | 同じ行（`if (x) {`） | 1 画面の情報量が増える。Rust / Go / Java / JS / Swift など現代の主要言語と同じで、cuno の「モダン」「cargo と同じ体験」に合う |
| ポインタ・参照 | C++: `int* p` / C: `int *p` | 言語ごとの慣習に合わせる。C++ は Stroustrup・C++ Core Guidelines の流儀（`int*` を型として読む）、C は K&R・Linux カーネル等の伝統的な流儀 |
| include | 自動で並べ替える | 順序が機械的に決まり、マージ時の衝突が減る。順序が意味を持つヘッダ（`windows.h` 等）は空行で区切れば、区切りをまたいで並べ替えない |

- `int* a, b;`（`b` はポインタにならない）の紛らわしさは、「1 行に 1 宣言」を lint で指摘して防ぐ。

### `[lint]`

| 項目 | 既定値 | 説明 |
|---|---|---|
| `preset` | `"recommended"` | チェックの基本セット |
| `allow` | `[]` | 無効にするチェック |
| `warn` | `[]` | 警告にするチェック |
| `deny` | `[]` | エラーにするチェック |
| `exclude` | `[]` | 解析しないファイルのパターン |

`preset` の値:

- `none`: チェックなし。`warn` / `deny` で指定したものだけをチェックする。
- `recommended`: バグの可能性（bugprone）、性能（performance）、モダン化（modernize の一部）など、誤検知が少ないもの。既定値。
- `strict`: `recommended` に加え、可読性（readability）や C++ Core Guidelines 系など厳しめのもの。

#### preset の中身

チェック名は clang-tidy のグループ名。

**recommended**（既定）: 誤検知が少なく、バグや性能に直結するもの。

| 含めるもの | 内容 |
|---|---|
| `bugprone-*` | バグの可能性（ムーブ後の使用、整数オーバーフローの疑い等） |
| `clang-analyzer-*` | 静的解析（ヌル参照、メモリリーク等）。遅いが価値が高い |
| `performance-*` | 不要なコピー等 |
| `modernize-*` の一部 | `use-nullptr`、`use-override`、`make-unique`、`use-emplace` など、意見が分かれないもの |
| `readability-isolate-declaration` | 1 行に 1 宣言（`int* a, b;` 対策。cuno 既定スタイルの前提） |

- 除外: `bugprone-easily-swappable-parameters`（誤検知が非常に多い）。

**strict**: recommended に加えて次のもの。

| 加えるもの | 内容 |
|---|---|
| `readability-*` | 可読性全般 |
| `cppcoreguidelines-*` | C++ Core Guidelines |
| `modernize-*` の残り | |

- 除外（好みの問題で、厳しさとは関係ないもの）:
  - `modernize-use-trailing-return-type`: `auto f() -> int` の書き方を強制する。
  - `readability-identifier-length`: 短い変数名を禁止する。

- C のプロジェクトでは、C++ 専用のチェックは自動的に対象外になる。
- 除外リストの細かな調整は実装時に行い、本仕様書に書き戻す。

#### allow / warn / deny

`allow` / `warn` / `deny` は clippy と同じ考え方。チェック名は clang-tidy の名前（例: `bugprone-use-after-move`）。ワイルドカード（例: `modernize-*`）も使える。

- `allow`: そのチェックを無効にする。
- `warn`: 指摘を警告として表示する。終了コードには影響しない。
- `deny`: 指摘をエラーとして表示し、終了コードを `1` にする。

**優先規則**（同じチェックが複数のリストに該当する場合）:

1. 具体的な名前（`modernize-use-auto`）は、ワイルドカード（`modernize-*`）より優先する。
2. 具体度が同じなら `deny` > `warn` > `allow` の順に優先する。

例: `warn = ["modernize-*"]` と `allow = ["modernize-use-auto"]` なら、`modernize-use-auto` だけ無効で、他の `modernize-*` は警告。

#### 終了コード

| 状況 | 終了コード |
|---|---|
| 指摘なし、または警告（preset のチェック・`warn`）のみ | `0` |
| `deny` のチェックに該当する指摘がある | `1` |
| clang-tidy 自身のコンパイルエラー（`clang-diagnostic-error`。標準ライブラリとの不一致（STL1000）は解析の前の確認で終了コード 3 になるので、ここには来ない） | `1` |

- preset に含まれるチェックは、`deny` に書かない限り**警告**扱い。

## ツールのバージョン

- clang-format / clang-tidy の最低バージョンは **17**。clang-tidy は MSVC の標準ヘッダを読むので、Windows ではさらに Clang と同じく、使用する MSVC 標準ライブラリが受け付けることを条件とする（[toolchain.md](toolchain.md#コンパイラの最低バージョン)）。clang-format は標準ライブラリを読まないので、固定の 17 だけを条件とする。バージョンによって整形結果が変わり、手元と CI で `fmt --check` の結果が食い違うのを防ぐため。
- `cuno doctor` で確認する。

## ツールが見つからない場合

- clang-format / clang-tidy が見つからない、または最低バージョン未満の場合は、インストール方法を案内するエラーメッセージを出し、終了コード `3`（設定・環境の問題）で終わる。

## ドキュメント生成（`cuno doc`、将来）

- 当初は Doxygen を包む。設定は `cuno.toml` の `[doc]` に書き、Doxyfile は利用者に書かせない。
- 自作は C++ の構文解析が必要になるため、当面しない。

## IDE / エディタ連携

| 対象 | 方式 | 時期 |
|---|---|---|
| clangd を使うエディタ（VS Code / CLion / Neovim 等） | compile_commands.json を常に出力（[build.md](build.md)） | MVP |
| Visual Studio | `cuno ide vs` で .sln を生成。VS からのビルドは cuno を呼ぶ。VS のデバッガを使える | 後 |
| VS Code | `cuno ide vscode` で launch.json / tasks.json を生成 | 後 |
| IDE プラグイン等 | `cuno metadata` の JSON でプロジェクト情報を提供 | 後 |

- ビルド設定の正は常に `cuno.toml`。IDE 用ファイルは生成物にすぎない。
- IDE 用の生成物は `target/` 内に置き、リポジトリを汚さない。例外として compile_commands.json・`.clang-format`・`.clang-tidy` はプロジェクト直下に置く（clangd 等のエディタが自動で見つけられるようにするため。いずれも `.gitignore` に追加する）。

## 未決

- preset の除外リストの細かな調整（実装時）
- 自作フォーマッタへの移行時期と、clang-format との互換性の扱い
- `[doc]` の項目
- VS ソリューションの具体的な生成内容（構成・プラットフォームの対応付け）
