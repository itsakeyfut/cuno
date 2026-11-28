# コマンド体系

cargo を基準に、C/C++ と cuno 固有の事情を加えたコマンド体系。

## 一覧

| 分類 | コマンド | 内容 | 時期 |
|---|---|---|---|
| プロジェクト | `new` | 新規ディレクトリにプロジェクトを作成 | MVP |
| | `init` | 既存ディレクトリをプロジェクト化 | MVP 直後 |
| ビルド | `build` | ビルドのみ | MVP |
| | `run` | ビルドして実行 | MVP |
| | `check` | 構文・型チェックのみ（フラグは [build.md](build.md#設定とフラグの対応)）。オブジェクトを作らず高速（MSVC だけは、ビルドと同じコンパイルをリンクなしで行う）。既定ではプログラムのソース（本体とエントリファイル）を確認し、`--tests` でテストのコード（`CUNO_TEST` を定義した本体と `tests/`）、`--all-targets` で両方を確認する | MVP |
| | `clean` | 出力先を削除 | MVP |
| テスト | `test` | テスト実行 | MVP |
| | `bench` | ベンチマーク実行 | 後 |
| 開発ツール | `fmt` | 整形。`--check` で整形済みか確認のみ（CI 用） | MVP |
| | `lint` | 静的解析。`--fix` で自動修正 | MVP |
| | `doc` | ドキュメント生成 | 将来 |
| | `ide vs` / `ide vscode` | IDE 用ファイルの生成（[dev-tools.md](dev-tools.md)） | 後 |
| 依存 | `add` / `remove` | 依存の追加・削除 | 依存管理 段階 1 |
| | `update` | ロックファイルの更新 | 依存管理 段階 1 |
| | `tree` | 依存ツリーの表示 | 依存管理 段階 1 |
| | `fetch` | 依存の事前ダウンロード（オフライン作業用） | 依存管理 段階 1 |
| | `vendor` | 依存をリポジトリ内にコピー（ゲーム開発で好まれる方式） | 依存管理 段階 2 |
| ツールチェーン | `toolchain list` / `toolchain default` | 検出したコンパイラの一覧・既定の選択 | MVP 直後 |
| | `toolchain install` | コンパイラの自動インストール（rustup 相当） | 将来 |
| 環境診断 | `doctor` | VS / Clang / clang-format 等が使えるかを診断し、対処法を案内 | MVP |
| 配布 | `install` | ビルドした実行ファイルをインストール | 後 |
| | `package` / `publish` | パッケージ化・公開 | 将来（依存管理 段階 3） |
| cuno 自体 | `self update` / `self uninstall` | cuno の自己更新・アンインストール（[distribution.md](distribution.md#自己更新)） | M5 |
| 情報 | `metadata` | プロジェクト情報を JSON で出力（IDE・外部ツール連携用） | 後 |

依存管理の段階は [dependencies.md](dependencies.md) を参照。

## cuno 固有のコマンド

- **`doctor`**: Windows での C/C++ 開発は最初の環境構築でつまずきやすい。足りないものと対処法をすぐ示し、「気軽に始められない」ペインを解消する。
- **`toolchain`**: ツールチェーン管理を rustup のような別ツールに分けず、cuno 本体に統合する。

## 共通オプション

### 全コマンドで使えるもの

| オプション | 説明 |
|---|---|
| `-v, --verbose` | 詳しく表示する。ビルド系コマンドでは実行したコンパイラのコマンドも表示する |
| `-q, --quiet` | 表示を最小限にする |
| `--color <いつ>` | 色付け（auto / always / never） |

- `-q` は状態の行だけを省く。状態の行とは、進み具合と要約の行（`Compiling`、`Running`、`Finished`、`Created`、`Removed`、`cuno doctor` の `Selected`・`Found`・`Info` など）。エラー・警告・`help:`、コマンドの結果（`cuno test` の各テストの行と `test result:` の行など）、コンパイラなどほかのプログラムの出力は残す。各コマンドの節にある `-q` の説明はこの規則を当てはめたもので、新しいコマンドもこれに従う。
- `-v` と `-q` は同時に指定できない（使い方の誤り、終了コード 2）。設定ファイルの `[term] verbose = true` と `-q` では、`-q` に従う。
  - 理由: `--color` と同じく、コマンド引数で明示した指定を設定ファイルより優先する。
- 設定ファイル（[manifest.md](manifest.md)）は、引数を解釈した後、どのコマンドもそのコマンドの処理を始める前に読む。そのため `[term] color` と `[term] verbose` はすべてのコマンドに効く（`--help`、コマンドを指定しない `cuno`、使い方の誤りは、下のとおり設定ファイルを読む前に出す）。読めない、または誤りのある設定ファイルはエラー（終了コード 3）で、コマンドの処理は始めない。ただし `cuno doctor` はそれを問題として示して診断を続ける（[`cuno doctor`](#cuno-doctor)）。
  - `cuno new` だけは、利用者の `<cuno home>/config.toml` だけを読み、カレントディレクトリの周りのプロジェクトの `Cuno.toml` と `.cuno/config.toml` は読まない。利用者の `config.toml` が読めない、または誤りがあるときは、ほかのコマンドと同じくエラー（終了コード 3）で、何も作らない。
  - 理由: `cuno new` が作るのは新しいパッケージで、周りのプロジェクトの設定はそのパッケージのものではない。そのプロジェクトの設定が壊れていても、新しいパッケージを作るのを止める理由にならない。

### ビルド系コマンド（`build` / `run` / `check` / `test`）で使えるもの

| オプション | 説明 |
|---|---|
| `--release` | release プロファイルでビルドする |
| `--profile <名前>` | 指定したプロファイルでビルドする。MVP では `dev` / `release` のみ（それ以外はエラー、終了コード 3） |

- `cuno lint` はこの 2 つを受け付けない（指定すると不明なオプションとして終了コード 2）。理由は [dev-tools.md](dev-tools.md#対象ファイル)。

### ビルド系コマンドと `lint`（`build` / `run` / `check` / `test` / `lint`）で使えるもの

| オプション | 説明 |
|---|---|
| `--compiler <名前>` | 使用するコンパイラ（msvc / clang / clang-cl / gcc / safec）。値の説明は [manifest.md](manifest.md) の `compiler`。`safec` を指定するとエラー（終了コード 3。まだ対応していないため） |
| `--target <ビルド対象環境>` | クロスコンパイル。MVP では予約のみで、指定するとエラー（終了コード 3）（[toolchain.md](toolchain.md)） |
| `--target-dir <パス>` | 出力先（[build.md](build.md)）。相対パスはカレントディレクトリ基準（環境変数 `CUNO_TARGET_DIR` も同じ） |
| `-j, --jobs <数>` | 並列数。既定は CPU コア数 |

`--color` の値:

- `auto`: 端末に出力しているときだけ色を付ける。既定値。
- `always`: 常に色を付ける。
- `never`: 色を付けない。

- 色付けは、次のうち上にあるものに従う。
  1. `--color`
  2. 設定ファイルの `[term] color`（[manifest.md](manifest.md#term)）
  3. 環境変数 `NO_COLOR`。値が空でも、設定されていれば `never` として扱う
  4. 既定値 `auto`
  - 理由: NO_COLOR の慣習では、利用者が引数や設定ファイルで明示した指定を NO_COLOR より優先する。環境全体で `NO_COLOR` を設定したまま、cuno だけ色を付けられる。
- 環境変数 `CLICOLOR` / `CLICOLOR_FORCE` は読まない。端末でない出力にも色を付けるには、`--color always` か `[term] color = "always"` を使う。
  - 理由: 常に色を付ける方法はすでに 2 つあり、読む環境変数と順位の規則を増やさないため。
- `--help`、コマンドを指定しない `cuno`、使い方の誤り（終了コード 2）の表示は、設定ファイルを読む前に出すため `[term] color` は効かず、`--color`、`NO_COLOR`、既定値 `auto` の順に従う。`--color` の値そのものが誤っている場合、その使い方の誤りの色は、`--color` がないものとして決める。
- 設定ファイルを読み終える前に出るエラー（設定ファイル自体の誤りなど）は、`--color` と `NO_COLOR` だけで決めた色で表示する。

### コマンドを指定しない場合（`--version` / `--help` / 引数なし）

| 実行 | 出力先 | 出力 | 終了コード |
|---|---|---|---|
| `cuno --version`（`-V`） | 標準出力 | 1 行目が `cuno <バージョン>` | `0` |
| `cuno --help`（`-h`） | 標準出力 | 使い方（英語） | `0` |
| `cuno`（コマンドなし） | 標準出力 | `--help` と同じ使い方 | `0` |

- コマンドなしの `cuno` は使い方の誤りとして扱わない（cargo と同じ）。
- `--version` の 1 行目の形式は固定する。スクリプトや `cuno self update` が読むため。コミットハッシュなどの追加情報を出す場合は、2 行目以降に置く。

## コマンド別オプション（MVP）

### `cuno new <パス>`

| オプション | 説明 |
|---|---|
| `--lang <言語>` | 言語（c / c++）。既定 c++ |
| `--edition <規格>` | 言語規格。値は [manifest.md](manifest.md) の `edition` |
| `--name <名前>` | パッケージ名。既定はディレクトリ名 |
| `--vcs <種類>` | バージョン管理（git / none）。既定 git |

- **パッケージ名の規則**: 英数字・`-`・`_` のみ。先頭は英字。違反すればエラー（終了コード 2）。`--name` を省略してディレクトリ名が規則に合わない場合もエラーにし、`--name` での指定を案内する。
  - Windows の予約名（`con`、`prn`、`aux`、`nul`、`com1`〜`com9`、`lpt1`〜`lpt9`。大文字・小文字を問わない）も、どの OS でもエラーにする。
    - 理由: パッケージ名はディレクトリ名と実行ファイル名になる。Windows ではこれらの名前のファイルやディレクトリを作れないか、作れても消しにくい。
- **作成先のディレクトリが既にある場合**はエラー（終了コード 2）。既存ディレクトリのプロジェクト化は [`cuno init`](#cuno-init-パス) で扱う。
- `--edition` と `--lang` が合わない場合（例: `--lang c++ --edition c17`）はエラー。`--edition` だけを指定した場合は、その規格から言語を決める（`c11` / `c17` / `c23` なら C）。
- `--lib`（ライブラリとして作成）は、ライブラリプロジェクトの対応と合わせて「後」に追加する（[project-layout.md](project-layout.md)）。

`--vcs` の値:

- `git`: Git リポジトリとして初期化し、`.gitignore` を生成する。既定値。
  - git がインストールされていない場合は、警告を出して `none` と同じ動作にする。
  - 既存の Git リポジトリの中に作る場合は `git init` しない（cargo と同じ）。`.gitignore` は生成する。
- `none`: バージョン管理の初期化をしない。

#### 生成するもの

| ファイル | 内容 |
|---|---|
| `Cuno.toml` | `[package]` の `name` / `version`（`0.1.0`）/ `lang` / `edition` のみ。コンパイラは書かない（自動選択に任せる） |
| `src/main.cpp`（C は `src/main.c`） | `src/calc.cpp` の関数を呼び、結果を出力する `main`。出力は規格に合わせる。C++17 / C++20 は `std::cout << ... << '\n'`、C++23 は `std::println`、C は `printf` |
| `src/calc.cpp` / `src/calc.h`（C は `.c` / `.h`） | 小さな関数と、そのソース内テスト（[testing.md](testing.md)）。標準ヘッダ（`math.h` 等）と同じ名前を避ける（`src/` はインクルードパスに入るため、衝突すると標準ライブラリ側の `#include <math.h>` がこちらを読み込んでしまう） |
| `.gitignore` | `target/`、`.cuno/`、`compile_commands.json`、`.clang-format`、`.clang-tidy`（後の 3 つは cuno の生成物。[dev-tools.md](dev-tools.md)） |

- 空の `tests/` / `examples/` / `benches/` は作らない。

#### 表示とエラー

- 作り終えたら ``     Created `<名前>` package`` を表示する（`-q` では出さない）。
- 下の表のエラーは終了コード 2。ファイルやディレクトリを作れない場合は終了コード 3。

| 場合 | メッセージ |
|---|---|
| 名前が規則に合わない | `` error: `<名前>` is not a valid package name `` と `help: a package name starts with a letter and has only letters, digits, - and _`。名前がディレクトリ名から来た場合は `help: name the package with --name` も出す |
| 名前が Windows の予約名 | `` error: `<名前>` is a reserved name on Windows `` と `help: choose another name` |
| 作成先がある | `` error: `<パス>` already exists `` |
| `--edition` が `--lang` と合わない | `` error: `<規格>` is not a standard of <言語> `` と `help: use --lang <規格の言語>, or an edition of <言語>: <その言語の規格の一覧>` |

- git が見つからない場合は `warning: git was not found; the package is not under version control` を出す。`git init` が失敗した場合も、`warning: git init failed: <git の出力>` の警告にとどめる（ファイルは作り終えているため、作ったものは消さない）。

### `cuno init [パス]`

既存のディレクトリ（既定はカレントディレクトリ）をパッケージにする。オプションは `cuno new` と同じ（`--lang`、`--edition`、`--name`、`--vcs`）。パッケージ名の規則と、`--lang` と `--edition` の扱いも `cuno new` と同じ。

ディレクトリの中身で、書くものを決める。

| ディレクトリの中 | 書くもの |
|---|---|
| `src/` にエントリファイル（`src/main.*` か `src/bin/` 直下のソース。[project-layout.md](project-layout.md#エントリファイルとパッケージ本体)）がある | `Cuno.toml` だけ |
| ソース（`.c` / `.cpp` / `.cc` / `.cxx`）が 1 つもない | `Cuno.toml` と、`cuno new` と同じ `src/main.*`・`src/calc.*` |
| ソースはあるが、上のどちらでもない（`src/` の外にだけある、`src/` にエントリファイルがない） | 何も書かずにエラー（終了コード 2） |

- 言語は、`--lang` / `--edition` がなければ、`src/` のソースから決める。C++ のソースが 1 つでもあれば C++、`.c` だけなら C、ソースがなければ C++（`cuno new` と同じ）。
- `Cuno.toml` の中身は `cuno new` と同じ（`[package]` の `name` / `version` / `lang` / `edition` のみ）。
- **既存のファイルは上書きしない。** 書くファイルのどれかがすでにあれば、何も書かずにエラーにする。ただし `.gitignore` は例外で、下のとおり足りない行を足す。
- `--vcs git`（既定）では、`cuno new` の `.gitignore` の行（`target/` など）のうち、すでにある `.gitignore` に書かれていないものを末尾に足す（既存の行、改行の形、BOM はそのまま残す）。`.gitignore` がなければ作る。
  - 理由: 既存のプロジェクトをパッケージにすると、cuno の生成物（`target/` など）が新たにできる。足さないと誤ってコミットしやすい。
- Git の扱いは `cuno new` と同じ（既存のリポジトリの中なら `git init` しない。git がない、`git init` が失敗した場合は警告にとどめる）。
- 書き終えたら ``     Created `<名前>` package`` を表示する（`-q` では出さない）。

| 場合 | メッセージ |
|---|---|
| ディレクトリがない | `` error: `<パス>` does not exist `` と `help: use cuno new to create a package in a new directory` |
| パスがディレクトリでない | `` error: `<パス>` is not a directory `` |
| `Cuno.toml` がすでにある | `` error: `<パス>` already has a Cuno.toml `` |
| ソースが `src/` の外にだけある | `` error: sources found outside src/: <見つけたソース> `` と `help: move them under src/, with the program's main in src/main.cpp (or src/main.c)` |
| `src/` にソースはあるが、実行ファイルになるもの（エントリファイル）がない | `` error: src/ has no executable to build `` と `help: put the program's main in src/main.cpp (or src/main.c), or each program in src/bin/` |
| 書くファイル、またはその上のディレクトリの場所に、別のものがすでにある（`src/main.cpp` を書くのに `src` がファイル、など） | `` error: <ファイル> already exists `` と `help: cuno init writes no file that is there; move it aside, or add Cuno.toml by hand` |
| `--vcs git` で、すでにある `.gitignore` に足せない（ファイルでない、リンク、UTF-8 として読めない） | `` error: cannot add to .gitignore: <理由> `` と `help: make it a text file in UTF-8, or use --vcs none` |

- 見つけたソースは、多ければ最初の 5 つと残りの数を示す（`and 2 more`）。
- ソースは、`target/`、名前が `.` で始まるディレクトリ、自前の `Cuno.toml` を持つディレクトリ（別のパッケージ）、リンクの中は探さない。`src/` かどうかは、ビルドと同じくファイルシステムの規則で決める（Windows では `Src/` も `src/`）。
- エラーはすべて、何かを書く前に判断する。エラーで止まったときに、書きかけのものは残らない。
- 理由（`src/` の外のソースを `[[bin]]` に書かない）: cuno はパッケージ本体を `src/` からしか集めないので、入口だけを書いても、ほかのソースはビルドされずにリンクで失敗する。

### `cuno run`

| オプション | 説明 |
|---|---|
| `--bin <名前>` | 実行する実行ファイル（複数ある場合は必須。省略するとエラーで候補を表示） |
| `-- <引数...>` | `--` 以降をプログラムに渡す |

- 実行時の作業ディレクトリは**プロジェクトのルート**（`Cuno.toml` のある場所）。ゲームのアセットを相対パスで読む場合の基準になる。
- `cuno build` と同じようにビルドしてから（表示と失敗時の扱いも同じ）、選んだ実行ファイルを実行する。
- 実行ファイルの選び方: 1 つだけならそれ。`--bin` があればその名前のもの。Windows では、実行ファイル名の重複を確かめるときと同じく、大文字と小文字を区別せずに比べる（`--bin APP` で `app` を実行する。[project-layout.md](project-layout.md#エントリファイルとパッケージ本体)）。どちらもビルドの前に確かめる。
  - 理由: 重複の確かめ方と同じ比べ方なら、当てはまる実行ファイルは必ず 1 つで、出力先でも同じファイルになる。
  - 複数あって `--bin` がない: `error: several executables can be run; choose one with --bin` と `help: available: app, tool`（候補の名前）、終了コード 2。
  - `--bin` の名前の実行ファイルがない: ``error: no executable named `x` `` と同じ `help:`、終了コード 2。
- 実行の前に `Running` の行に実行ファイルのパスを示す（カレントディレクトリからの相対パス。その下になければ絶対パス）。`-q` では出さない。

```
    Finished `dev` profile [unoptimized + debuginfo] in 1.24s
     Running `target\debug\app.exe`
```

- プログラムには cuno が受け取った環境変数をそのまま渡す（コンパイラ用に解決した Windows の環境ではない）。標準入力・標準出力・標準エラー出力はつないだまま。
- 終了コードはプログラムの終了コード（[終了コード](#終了コード)）。

### `cuno test [絞り込み]`

| オプション | 説明 |
|---|---|
| `[絞り込み]` | 名前に指定文字列を含むテストだけを実行する |
| `--nocapture` | テスト中の標準出力・標準エラー出力を常に表示する（既定は失敗したテストの分だけ） |

- ビルド系コマンドの共通オプション（`--release`、`--profile`、`--compiler`、`--target-dir`、`-j`）も使える。
- テストビルド（[build.md](build.md#出力先ディレクトリ)）を `cuno build` と同じようにビルドしてから（表示と失敗時の扱いも同じ）、テスト実行ファイルごとに、実行する前に `Running` の行で実行ファイルのパスを示し、結果を [testing.md](testing.md#出力) の形式で表示する。
- `Running` の行の実行ファイルのパスは、`cuno run` と同じくカレントディレクトリからの相対パス（その下になければ絶対パス）。`-q` では `Running` の行を出さない（テストの行、失敗の詳細、`test result:` の行は出す）。
- 絞り込みは、すべてのテスト実行ファイルに同じように当てる。当てはまるテストがない実行ファイルも `running 0 tests` と表示する。
- 終了コード: すべて成功（絞り込みで 0 件の場合も）は `0`。失敗したテストが 1 つでもあれば `1`。一覧を取れないテスト実行ファイル（[testing.md](testing.md#出力)）があっても `1`。ビルドの失敗も `1`（[終了コード](#終了コード)）。

### `cuno clean`

| オプション | 説明 |
|---|---|
| `--release` / `--profile <名前>` | そのプロファイルの出力だけ削除する。指定なしなら出力先全体を削除する |
| `--target-dir <パス>` | 削除対象の出力先（[build.md](build.md) の優先順位に従う） |

- プロファイルの指定がなければ出力先全体を、あれば `<出力先>/<プロファイルのディレクトリ>`（`dev` は `debug`）だけを削除する。出力先の外のものは削除しない。
- 削除の前に、出力先を [build.md](build.md#出力先ディレクトリ) の規則で確かめる。プロジェクトそのものやその上を指す出力先と、プロジェクトの設定（`Cuno.toml`、`.cuno/config.toml`）がプロジェクトの外を指す出力先はエラー（終了コード 3）で、何も削除しない。
- プロジェクトの外で実行した場合は、`cuno build` と同じエラー（終了コード 3）。
- 出力先があって、その直下に cuno の印（`.cuno-target`、[build.md](build.md#出力先ディレクトリ)）がなければ、何も削除せずにエラー。プロファイルを指定したときも、確かめるのは出力先の直下の印（``error: `<出力先>` was not made by cuno; not removing it``、`help: remove it yourself, or set target-dir to a directory cuno builds into`、終了コード 3）。
- 削除するディレクトリがなければ何もせず成功する。
- 削除に失敗した場合（ほかのプログラムが使っているファイルがある、など）はエラー（終了コード 3）。それまでに削除したものは戻らない。
- 終わりに、削除したファイルの数を示す（1 件なら `file`。`-q` では出さない）。

```
     Removed 42 files
```

### `cuno fmt`

| オプション | 説明 |
|---|---|
| `--check` | 整形せず、未整形のファイルがあれば失敗する（CI 用） |

- 最後に、書き換えたファイルの数を示す（1 件なら `file`。`-q` では出さない）。

```
   Formatted 3 files
```

- `--check` は何も書かない。未整形のファイルごとに ``error: `src/main.cpp` is not formatted`` を出し、最後に `help: run cuno fmt to format them` を添えて終了コード 1。すべて整形済みなら何も出さず終了コード 0。
  - 理由: clang-format 自身のチェックは崩れた箇所ごとに数行を出し、1 行の崩れでも十数行になる。直し方は `cuno fmt` の一つなので、ファイル名で足りる。
- 整形できなかったファイルは ``error: could not format `src/main.cpp`: <理由>`` で示し、元のまま残す。ほかのファイルは整形し、`Formatted` の行も出して、終了コード 1。整形中に変更されたファイルと、clang-format が何も出力しなかった空でないファイルも、書き換えずにこの形で示す。
  - 理由: ファイルは書き直さず、横に書いてから置き換える。途中で止まっても半端なファイルが残らず、整形の間に保存された変更も失わない。
- `[fmt.clang-format]` を clang-format が受け付けない（知らないキーなど）ときは、整形を始める前にエラー（終了コード 3）。

### `cuno lint`

| オプション | 説明 |
|---|---|
| `--fix` | 自動修正できる指摘を修正する |

- 指摘は clang-tidy の表示（`ファイル:行:列: warning: ... [チェック名]` とコードの抜粋）のまま出す。ソースごとの clang-tidy は `-j` に従って並列に動かし、表示はソースごとにまとめて、混ざらないようにする。
- clang-tidy の集計の行（`N warnings generated.` など）は出さない。同じヘッダの同じ指摘が複数のソースから出たときは 1 回だけ出す。パッケージの外のファイルの指摘は出さないが、コンパイルエラー（`clang-diagnostic-*` のエラー）はどのファイルのものでも出して数える（そのソースを解析できていないため）。
  - 理由: 指摘の形を変えないことで、エディタや CI がそのまま読める。作り直すと、clang-tidy の版で表示が変わったときに指摘を取りこぼすおそれがある。
- 解析するソースは、compile_commands.json にコマンドのあるもの（パッケージ本体、エントリファイル、`tests/` のソース）。`examples/` や `benches/` のように、まだビルドしないソースは解析しない。ヘッダは、それを読むソースを通して解析する。同じファイルは 1 回だけ解析し、数える（2 つの `[[bin]]` が同じファイルを指す場合も）。解析と表示はパスの順（`cuno fmt` と同じく、Windows では大文字と小文字を区別しない）。
- `--fix` は、ソースファイル自身への指摘だけを直す。ヘッダは書き換えず、ヘッダへの指摘も表示しない（`--fix` なしの `cuno lint` で確かめる）。ファイルへのリンクのソースも直さず、``warning: `src/x.cpp` is a link; cuno lint --fix leaves it`` を出す。`--fix` のときは、ソースを 1 つずつ解析する。直した指摘は表示するが、警告にもエラーにも数えない。修正がパッケージの外のファイル（`include-dirs` のヘッダの宣言など）に及ぶソースは直さず、``warning: `src/main.cpp` is not fixed: a fix would change `C:\vend\v.h`, which is not the package's`` を出す。修正がパッケージの中のリンク（リンクになっているヘッダなど）に及ぶソースも直さず、``warning: `src/main.cpp` is not fixed: a fix would change `C:\proj\include\v.h`, which is a link cuno does not write through`` を出す（リンクの先はプロジェクトの外でありうるため）。書き換えられなかったソース（読み取り専用など）は ``error: could not lint`` として示す。
  - 理由: clang-tidy の `--fix` は、表示を許したヘッダをすべて書き換える。パッケージのヘッダだけに絞る正規表現は、Windows の綴りの揺れ（大文字・小文字、区切り）で確実に作れず、`include-dirs` の第三者のヘッダを書き換えてしまう。並べて解析すると、同じファイルを 2 つの解析が同時に書き換えうる。
- 有効なチェックが 1 つもない（`preset = "none"` で `warn` / `deny` も空）ときは、clang-tidy を動かさず、``warning: no checks are enabled ([lint] preset = "none" and no warn / deny); nothing was analysed`` を出して終了コード 0。
- `allow` / `warn` / `deny` に書いた名前を、使う clang-tidy が知らない（ワイルドカードなら、どのチェックにも当てはまらない）ときは、``warning: `bugprone-typoo` in `lint.deny` names no check this clang-tidy has`` を出して解析は続ける。`clang-diagnostic-*`（コンパイラ自身の警告）は clang-tidy がチェックとして一覧に出さないので、この確かめの対象外。
  - 理由: clang-tidy は知らない名前を黙って無視するので、綴りを誤った `deny` が何もしないことに気づけない。
- clang-tidy は 1 ソースにつき 10 分で止め、そのソースは ``error: could not lint `src/x.cpp`: <理由>`` として示す（終了コード 1）。clang-tidy が 0 以外で終わり、それを説明するエラーの指摘がないときも同じ形で示す。
- 最後に、解析したファイルの数と指摘の数を示す（`-q` では出さない）。

```
      Linted 12 files: 3 warnings, 1 error
```

### `cuno build` / `cuno check`

`cuno build` は共通オプションのみ。`cuno check` は、確認する対象を選ぶ次のオプションも受け付ける。

| オプション | 説明 |
|---|---|
| （なし） | プログラムのソース（本体とエントリファイル）を確認する |
| `--tests` | テストのビルド（`CUNO_TEST` を定義した本体と `tests/` のソース。[testing.md](testing.md#実行)）だけを確認する |
| `--all-targets` | プログラムとテストの両方を確認する |

- 理由: Cargo の `cargo check` と同じ選び方にする。既定でテストのコードを確認しないのは、テストの確認には本体をもう一度（`CUNO_TEST` を定義して）確認する必要があり、日常の確認を遅くするため。
- `--tests` と `--all-targets` を両方指定した場合は `--all-targets` と同じ。
- テストのビルドの確認は、`cuno test` と同じ出力先（テストのビルドの場所）を使う。MSVC では `cuno test` と同じコンパイルを行うので、続く `cuno test` はリンクと実行だけを行う。

- プロジェクトの外で実行した場合はエラー（`error: could not find Cuno.toml in <directory> or any parent directory`、終了コード 3）。設定にも誤りがあるとき（出力先の指定の誤り、読めない、または誤りのある利用者の `config.toml` など）も、プロジェクトがないことだけを示す。`cuno run`、`cuno test`、`cuno clean` も同じ。
  - 理由: プロジェクトの外では、これらのコマンドは何もできない。直すべきこと（プロジェクトの中で実行する）を 1 つだけ示す。
- ビルドの前に、コンパイラの選択と標準ライブラリとの組み合わせを確かめる（[toolchain.md](toolchain.md#コンパイラの最低バージョン)）。満たさなければ何もビルドせずにエラー（終了コード 3）。
- 表示（[出力](#出力) の形式）:

```
   Compiling app v0.1.0 (msvc 19.52.37029)
     Running `cl.exe /nologo /utf-8 ... /c src\main.cpp /Fo...`
    Finished `dev` profile [unoptimized + debuginfo] in 1.24s
```

  - コンパイルを始める前に、`Compiling` の行（パッケージ名とバージョン、コンパイラとバージョン）を 1 行。すべて最新で何も実行しないときは出さず、`Finished` の行だけになる。
  - `-v` では、各手順（コンパイル、リンク）を始める前に、`Running` の行に実行するコマンドを示す。
  - 終わりに `Finished` の行。角括弧の中は、`opt-level` が `0` なら `unoptimized`、それ以外は `optimized` で、`debug-info` が有効なら `+ debuginfo` を続ける。
  - `-q` では、エラーと警告とコンパイラの出力だけを表示する。
- コンパイルかリンクが失敗した場合、コンパイラの出力の後に `error: could not compile` の行（リンクなら `could not link`。どちらもパッケージ名を続ける）を出し、終了コード 1。最初の失敗の後は新しい手順を始めず、実行中の手順は終わるのを待ってその出力も表示する。
- 並列に実行した手順の出力は、手順ごとにまとめて、その手順が終わったときに表示する（行が混ざらないように）。
- `--target` はクロスコンパイルのための予約で、指定するとエラー（`error: --target is not supported yet`、終了コード 3）。
- `cuno check` は、ビルドと同じソースを構文・型チェックだけのフラグ（[build.md](build.md#設定とフラグの対応)）で確認し、オブジェクトを作らずリンクもしない（MSVC は例外。下記）。表示は `Compiling` の代わりに `Checking` の行で、`Finished` の行は同じ。失敗したときはビルドのコンパイルの失敗と同じく `could not compile` の行（パッケージ名を続ける）、終了コード 1。
  - ビルドと同じ仕組みで、変更のないソースは確認し直さない（[build.md](build.md#変更の判定)）。
  - MSVC では、`check` はビルドと同じコンパイルをリンクなしで行い、ビルドと同じオブジェクトを作る（`/Zs` はコード生成の段階で出る診断を出さず、`check` が成功してもビルドが失敗し得るため。[build.md](build.md#常に付けるフラグ)）。続くビルドはリンクだけを行う。

### `cuno doctor`

環境を診断し、問題ごとに対処法を表示する。

| オプション | 説明 |
|---|---|
| `--compiler <名前>` | 診断するコンパイラ。ビルド系コマンドの `--compiler` と同じ。指定しなければ、ビルド系コマンドと同じ選択の優先順位で選ばれるコンパイラを診断する（[toolchain.md](toolchain.md#選択)） |

- プロジェクトの外（`Cuno.toml` が見つからない場所）でも実行できる。その場合はプロジェクトの層がない優先順位で選ぶ（[toolchain.md](toolchain.md#選択)）。
- 設定ファイル（`Cuno.toml`、`.cuno/config.toml`、ユーザー単位の config）に誤りがあっても、まだ対応していないコンパイラ（`safec`）が選ばれていても、`doctor` は止まらずに問題として報告し（終了コード `1`）、ほかの項目の診断を続ける。
  - 設定ファイルを読めないときは、`--compiler`、`CUNO_COMPILER` と自動選択だけでコンパイラを選ぶ。自動選択になった場合は、設定で選んだコンパイラと取り違えないよう、`Selected` の理由に `automatic selection; the configuration could not be read` と書く。
  - 理由: `doctor` は環境や設定がおかしいときに実行するものなので、設定の誤りで止まると役に立たない。

| 判定 | 条件 | 終了コード |
|---|---|---|
| 問題あり | 選択されるコンパイラでビルドできない（見つからない、バージョン制約を満たさない、Visual Studio の環境を解決できない等） | `1` |
| 問題あり | clang-format / clang-tidy が見つからない | `1` |
| 問題あり | 選ばれた Clang / clang-tidy が、使用する MSVC 標準ライブラリに受け付けられない、またはその判定ができない（C++ のプロジェクトとプロジェクトの外。[toolchain.md](toolchain.md#コンパイラの最低バージョン)） | `1` |
| 問題あり | 設定ファイルの誤り、未対応のコンパイラの選択 | `1` |
| 問題あり | `[toolchain.paths]` に書いたツールが使えない（選ばれていないコンパイラでも。[toolchain.md](toolchain.md#llvmclang--clang-cl--clang-format--clang-tidy)） | `1` |
| 情報 | 使用しないコンパイラが見つからない（例: MSVC を使う環境で Clang がない） | 影響しない |

問題がなければ終了コード `0`。

#### `cuno doctor` の出力

他のコマンドの状態行（[出力](#出力)）と同じ形で、1 項目を 1 行にする。すべて標準エラーに出す（標準出力は、将来プログラムが読む形式の出力のために空けておく）。

```
    Selected clang-cl 22.1.8 (--compiler)
             C:\Program Files\LLVM\bin\clang-cl.exe
       Using MSVC 19.52.37029 (C:\Program Files\Microsoft Visual Studio\18\Community)
       Using Windows SDK 10.0.28000.0
    Accepted by the MSVC standard library
       Found clang-format 22.1.8
       Found clang-tidy 22.1.8
        Info msvc 19.52.37029 (not selected)
        Info clang 22.1.8 (not selected)
    Finished no problems found
```

- `Selected`: 選ばれたコンパイラ、そのバージョン、選ばれた理由（`--compiler`、`CUNO_COMPILER`、設定ファイルのどれか、または `automatic selection`）。次の行に実行ファイルの場所。
- `Using`: 使う MSVC のツールセット（cl のバージョンと Visual Studio の場所。MSVC を選んだときは `Selected` の行が兼ねる）と Windows SDK。
- `Accepted`: Clang を選んだとき、MSVC 標準ライブラリがそれを受け付けたこと（[toolchain.md](toolchain.md#コンパイラの最低バージョン)）。
- `Found`: clang-format と clang-tidy。
- `Info`: 選ぼうとしたもの以外のコンパイラ。そのコンパイラを選んだら使われるもののバージョン、見つからなければ `not found`、見つかったが使えなければ `not usable` とその理由（cuno の最低バージョンを満たさない、など）。判定は選択と同じで、`[toolchain.version]` だけは当てない（実際に使うコンパイラにだけ当てるため）。終了コードに影響しない。ただし `[toolchain.paths]` に書いたものが使えない場合は、`Info` の代わりに問題として出す（そのコンパイラに切り替えたときに初めて分かるのを防ぐため）。
- 問題は、見つかった場所で `error:` と `help:` の行として出し、診断は続ける。設定ファイルの警告は `warning:` として最初に出す。
- `-q` では、設定ファイルの警告と問題（`error:` と `help:`）だけを表示する。問題がなければ何も表示しない（終了コードで分かる）。
- 最後の行は `Finished no problems found`、問題があれば `Finished <数> problems found`（1 件なら `problem`）。

### `cuno toolchain list`

cuno が検出したコンパイラと、clang-format / clang-tidy を一覧にする。

- プロジェクトの外でも実行できる。どのコンパイラが選ばれるかは、ビルド系コマンドと同じ選択の優先順位で決める（[toolchain.md](toolchain.md#選択)）。
- 種類ごとに 1 行（`msvc`、`clang`、`clang-cl`、`gcc`、`clang-format`、`clang-tidy` の順）。その種類を選んだときに使われるもの（`[toolchain.paths]` に書いたものがあればそれ、なければ使える中で最新のもの）のバージョンと場所を出す。
  - 選ばれるコンパイラの行は、選択と同じ判定（cuno の最低バージョンと `[toolchain.version]`）で選んだものを出し、末尾に `(selected: <理由>)` を付ける。理由は `cuno doctor` の `Selected` の行と同じ（`CUNO_COMPILER`、設定ファイルのどれか、または `automatic selection`。このコマンドに `--compiler` はない）。
  - ほかの行の判定は `cuno doctor` の `Info` の行と同じで、`[toolchain.version]` は当てない（実際に使うコンパイラにだけ当てるため）。
  - 見つからなければ `not found`、見つかったが使えなければ `not usable: <理由>`。`[toolchain.paths]` に書いたものが使えない場合も `not usable: <理由>` とする（問題として報告するのは `cuno doctor`）。
  - 選ばれるはずのコンパイラが選べない場合（見つからない、バージョンが条件に合わない）は、その行に理由を出し、末尾に `(would be selected: <理由>)` を付ける。
- 一覧は標準出力に出す（このコマンドの結果のため。`-q` でも消えない）。設定ファイルの警告などは、ほかのコマンドと同じく標準エラーに出す。
  - 理由: `cuno toolchain list > toolchains.txt` や、ほかのコマンドへのパイプで扱えるように。プログラムが読む形式が要るときは、別のオプション（`--format json` など）として足す。
- 終了コードは、一覧を出せれば `0`（何が見つからなくても）。設定ファイルを読めないときは、ほかのコマンドと同じくエラー（終了コード 3）。

```
msvc          19.52.37029  C:\Program Files\Microsoft Visual Studio\18\Community\VC\Tools\MSVC\14.52.37025\bin\Hostx64\x64\cl.exe  (selected: automatic selection)
clang         22.1.8       C:\Program Files\LLVM\bin\clang.exe
clang-cl      22.1.8       C:\Program Files\LLVM\bin\clang-cl.exe
gcc           not found
clang-format  22.1.8       C:\Program Files\LLVM\bin\clang-format.exe
clang-tidy    22.1.8       C:\Program Files\LLVM\bin\clang-tidy.exe
```

- 名前とバージョンは、最も長いものに揃えて空白で区切る。

### `cuno toolchain default <コンパイラ>`

既定のコンパイラを、ユーザー単位の `<管理ディレクトリ>/config.toml` の `[toolchain] compiler` に書く（[manifest.md](manifest.md#cunoconfigtoml)）。そのユーザーの、ほかの層がコンパイラを選んでいないすべてのビルドに効く。

- 書き換えるのは `[toolchain] compiler` だけ。ファイルのほかのキー、コメント、並びはそのまま残す。ファイルや管理ディレクトリがなければ作る。
- `<コンパイラ>` は `--compiler` と同じ名前。知らない名前と、まだ対応していないもの（`safec`）は、`--compiler` に指定したときと同じエラー（終了コード 3）で、何も書かない。
- 設定ファイルを読めないとき、`[toolchain]` が表でないときは、エラー（終了コード 3）で、何も書かない。
- 書いたら、状態行 `Updated` で、書いたキーとファイルを示す（標準エラー）。

```
     Updated [toolchain] compiler = "gcc" in C:\Users\me\.cuno\config.toml
```

- 次の場合は、書いたうえで警告する（終了コード `0`）。
  - 実行した場所では、より優先される層（`--compiler`、`CUNO_COMPILER`、プロジェクトの `.cuno/config.toml`・`Cuno.toml`）が別のコンパイラを選んでいる: `warning: gcc is the default, but here [toolchain] compiler in Cuno.toml selects clang`。
  - そのコンパイラが、このマシンで見つからない、または使えない: `warning:` に、ビルドしたときと同じ理由を書き、`help:` に入れ方を書く。
  - 理由: 既定はそのユーザーのすべてのビルドに効く設定で、これから入れるコンパイラを先に選んでおく使い方もあるため、書くこと自体は止めない。ただ、次のビルドが選ぶものと違うことには、書いた時点で気づけるようにする。

### `cuno self update`

cuno を最新のリリースに更新する。

- プロジェクトの外でも実行できる。確認はしない（同じ手順で入れ直せば戻せるため）。
- 入れ方（[distribution.md](distribution.md#自己更新)）ごとに次のようにする。
  - cuno 自身のインストーラで入れたもの: 最新の版を調べ、実行中の cuno より新しければ、zip と `.sha256` をダウンロードして照合し、`cuno.exe` を置き換える。
  - `cargo install` で入れたもの: 何もせず、`error: cuno was installed by cargo install, so cargo updates it` と `help: run cargo install cuno`（終了コード 3）。
  - それ以外: 何もせず、`error: <exe> was not installed by cuno's installer, so cuno cannot update it`（`<exe>` は実行中の `cuno.exe` の場所） と `help: install cuno with: powershell -c "irm https://github.com/itsakeyfut/cuno/releases/latest/download/install.ps1 | iex"`（終了コード 3）。
  - 理由: 終了コード 0 にすると、更新を任せたスクリプトが、更新されたと受け取る。
- 最新の版は、`https://github.com/itsakeyfut/cuno/releases/latest` の転送先（`…/releases/tag/v<version>`）から知り、`x.y.z` として実行中の版と比べる。`x.y.z` の形でなければエラー（終了コード 3）。
  - 実行中の版と同じなら `   Unchanged cuno 0.2.0 is the latest release`、実行中の方が新しければ（リリース前のビルドなど）`   Unchanged cuno 0.3.0 is newer than the latest release, 0.2.0` で、何もしない（終了コード 0）。古い版に戻すことはしない。
- 置き換え: 新しい `cuno.exe` を `bin\cuno.exe.new` に書いてから、`bin\cuno.exe` を `bin\cuno.exe.old` に名前を変えて退け（実行中のファイルは消せないが、名前は変えられる）、`cuno.exe.new` を `cuno.exe` に名前を変える。最後の名前の変更ができなければ `cuno.exe.old` を元の名前に戻し、それもできなければ、エラーに元の cuno が `cuno.exe.old` にあることを書く。`cuno.exe.old` は、次の `cuno self update` と `cuno self uninstall` が消す。
  - 理由: 途中で止まっても、`cuno.exe` がなくなる、または書きかけになる間を、名前の変更 2 回の間だけにする。
- 表示（標準エラー）:

```
 Downloading cuno 0.2.0
     Updated cuno 0.1.0 -> 0.2.0
```

- 次の場合はエラー（終了コード 3）で、`cuno.exe` を置き換えない。
  - `curl` を起動できない、ダウンロードに失敗した（[distribution.md](distribution.md#ダウンロード)）。
  - SHA-256 が合わない: `error: <zip> does not match its checksum`（`<zip>` は zip の名前）。
  - zip に `cuno.exe` がない、取り出せない、置き換えられない: 理由と場所を示す。

### `cuno self uninstall`

cuno の管理ディレクトリを消し、cuno 自身のインストーラで入れた cuno なら、その `cuno.exe` と `PATH` の項目も消す。

| オプション | 内容 |
|---|---|
| `-y` / `--yes` | 確認せずに消す |

- 消すもの: 管理ディレクトリ全体（`config.toml`、展開した同梱ヘッダ、自動インストールしたコンパイラなど）と、cuno 自身のインストーラで入れた場合は、ユーザーの `PATH` の `<管理ディレクトリ>\bin`。プロジェクトの中のもの（`.cuno/`、`target/`）と、ほかの場所のものには触れない。
- 管理ディレクトリが、ドライブか共有（`\\server\share`）のルートか、利用者のホームディレクトリ（`USERPROFILE`、`HOME`）か、それらを含むディレクトリなら（`\\?\` を付けた書き方でも同じ）、何も消さずに `error: refusing to remove <home>: it is not a directory of cuno's own` （`<home>` は管理ディレクトリ）でエラー（終了コード 3）。
  - 理由: `CUNO_HOME` を誤って設定していると、利用者のファイルをまとめて消してしまう。
- 管理ディレクトリか `<管理ディレクトリ>\bin` がリンク（ジャンクション、シンボリックリンク）なら、何も消さずに `error: refusing to remove <path>: it is a link; remove it and what it leads to by hand` でエラー（終了コード 3）。管理ディレクトリの中にあるリンクは、リンクそのものだけを消し、リンク先は辿らない。
  - 理由: リンクを辿ると、管理ディレクトリの外のファイルを消してしまう。
- 消す前に、消すものを 1 行ずつ示して `Continue? [y/N]` と聞く（標準エラー）。`y` か `yes`（大文字・小文字を問わない）なら続け、それ以外は何も消さずに終わる（終了コード 0）。
  - `-y` があれば聞かない。
  - `-y` がなく、標準入力が端末でなければ、何も消さずに `error: cuno self uninstall asks before deleting, and standard input is not a terminal` と `help: pass -y to remove without asking`（終了コード 2）。
  - 理由: 待っても答えが来ない入力で止まらないように。
- 入れ方ごとに次のようにする。
  - cuno 自身のインストーラで入れたもの: `PATH` から除き、管理ディレクトリを消す。実行中の `cuno.exe` 自身は消せないので、それ以外を消したうえで、cuno の終了を待って `cuno.exe` と残ったディレクトリを消す PowerShell のプロセスを起動して終わる。
  - `cargo install` で入れたもの: 管理ディレクトリだけを消し、`help: cuno itself was installed by cargo install; run cargo uninstall cuno to remove it`（終了コード 0）。
  - それ以外: 管理ディレクトリだけを消し、`help: cuno itself is at <exe>; remove it yourself`（終了コード 0）。
- 管理ディレクトリがなければ、`warning: nothing to remove: <home> does not exist` を示す（終了コード 0）。
- 表示（標準エラー）:

```
This removes:
  C:\Users\me\.cuno
  C:\Users\me\.cuno\bin from PATH
Continue? [y/N] y
     Removed C:\Users\me\.cuno\bin from PATH
     Removed C:\Users\me\.cuno
```

- 消せないもの（`cuno.exe` 以外）があれば、エラー（終了コード 3）で、その場所と理由を示す。

## 出力

### 言語

- 出力メッセージは**英語に統一**する。
  - 理由: 海外の利用者も見込み、エラーメッセージで検索しやすく、cargo 等とも揃うため。
  - 多言語対応は将来の検討事項。

### ほかのツールの名前

- cuno 自身の出力（`--help` の説明文、メッセージ）では、cuno が参考にしたツール（cargo、CMake など）の名前を出さない。
  - 理由: cuno を「何かに似た道具」ではなく、それ自体で説明が完結する道具として伝えるため。参考にしたツールを知らない利用者にも通じる説明にする。
  - cuno が実際に呼び出すコンパイラやツール（MSVC、clang、clang-format など）の名前は、診断や案内に必要なので出してよい。

### 形式

cargo に倣う。

```
   Compiling mygame v0.1.0 (msvc 19.40)
    Finished `dev` profile [unoptimized + debuginfo] in 1.24s
     Running `target/debug/mygame.exe`
```

- cuno 自身のエラーは `error: ...`、対処法は `help: ...` の形で示す。
- コンパイラのエラー・警告はそのまま表示する。最新で実行しなかった手順は、前回成功したときの出力（警告など）をそのまま表示する（[build.md](build.md#変更の判定)）。ただし、cuno が色を付けないとき（下の色の規則）は、その出力から色のエスケープシーケンスを取り除く。表示する位置は実行した手順と同じで、ほかの手順を実行するビルドでは `Compiling` の行の後、何も実行しないビルドでは `Finished` の行の前。
  - 理由: 入力もコマンドも同じなので、出力も同じ。表示しないと、何も変えずにビルドし直しただけで警告が消え、見落とした警告をもう一度見る方法がなくなる。
  - 例外として、cl.exe がコンパイルのたびに最初の行に出すソースのファイル名（`main.cpp` など）は表示しない。出力の最初の行が、コンパイルしたソースのファイル名と一致する場合だけ消す。ASCII 以外の文字を含むファイル名は、cl がコンソールのコードページ（日本語版 Windows では CP932）で出すため、名前の先頭と末尾の ASCII の部分が一致し、`:` を含まない行をファイル名の行とみなす（Windows のファイル名は `:` を含まず、cl の診断は必ず含む）。
    - 理由: エラーでも警告でもない行で、MSVC のときだけソースの数だけ並び、ほかのコンパイラと表示が揃わないため。
  - 翻訳はしない（日本語版 Visual Studio の cl.exe なら日本語のまま）。cuno 自身のメッセージだけを英語に統一する。
  - Clang のように色付きで出力できるコンパイラは色付きで表示する。MSVC は色を出さないため、そのまま色なしで表示する。
    - cuno の表示に色を付けるとき（上の「全コマンドで使えるもの」の順位）だけ、clang と clang-cl に `-fcolor-diagnostics -fansi-escape-codes`、GCC に `-fdiagnostics-color=always` を渡す。GCC も、パイプへの出力には指定しないと色を付けない。cuno はコンパイラの出力をパイプで受け取るため、指定しないと Clang は色を付けない。Windows では色をエスケープシーケンスで出すよう `-fansi-escape-codes` も要る。これらのフラグは引数の先頭に置く。`[build.clang] flags` などに利用者が書いた `-fno-color-diagnostics` は後ろにあるため、そちらが優先される。リンクには渡さない（リンカの出力に色はない）。`-v` で表示するコマンドには、これらのフラグも含める（実行したコマンドを表示するため）。
    - これらのフラグは、変更の判定に使うコマンドライン（[build.md](build.md#変更の判定)）にも `compile_commands.json` にも含めない。理由: 色の指定を変えただけで全体を再コンパイルしないため。生成されるコードは変わらない。ただし Clang と GCC はデバッグ情報にコマンドラインを記録するため、デバッグ情報を付けたビルドでは、これらのフラグの有無でオブジェクトファイルや PDB の中身が変わる（再コンパイルしないので、前のビルドのものが残る）。

## 終了コード

| コード | 意味 | 例 |
|---|---|---|
| `0` | 成功 | |
| `1` | 利用者のコード・成果物の問題 | コンパイルエラー、テスト失敗、`fmt --check` で未整形あり、lint の `deny` 違反、`doctor` で問題あり |
| `2` | コマンドの使い方の誤り | 不明なオプション、引数不足 |
| `3` | 設定・環境の問題 | `Cuno.toml` の誤り、コンパイラが見つからない、コンパイラのバージョンが条件に合わない |
| `101` | cuno 自身の内部エラー（バグ） | 想定外の失敗。Rust のパニックと同じ値 |

- `cuno run` は、実行したプログラムの終了コードをそのまま返す（cargo と同じ）。ビルドに失敗した場合は上の表に従う。
- 理由: CI で「コードの問題（1）」と「環境の問題（3）」を見分けられると、失敗の原因切り分けが速くなる。

## 未決

- MVP 外のコマンドのオプション
