# コマンド体系

cargo を基準に、C/C++ と cuno 固有の事情を加えたコマンド体系。

## 一覧

| 分類 | コマンド | 内容 | 時期 |
|---|---|---|---|
| プロジェクト | `new` | 新規ディレクトリにプロジェクトを作成 | MVP |
| | `init` | 既存ディレクトリをプロジェクト化 | MVP 直後 |
| ビルド | `build` | ビルドのみ | MVP |
| | `run` | ビルドして実行 | MVP |
| | `check` | 構文・型チェックのみ（フラグは [build.md](build.md#設定とフラグの対応)）。オブジェクトを作らず高速。テストのコード（`CUNO_TEST` を定義した状態、`tests/`）とエントリファイルも確認する | MVP |
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
| cuno 自体 | `self update` | cuno の自己更新（[distribution.md](distribution.md)） | 後 |
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
  - `cuno new` だけは、利用者の `<cuno home>/config.toml` だけを読み、カレントディレクトリの周りのプロジェクトの `cuno.toml` と `.cuno/config.toml` は読まない。利用者の `config.toml` が読めない、または誤りがあるときは、ほかのコマンドと同じくエラー（終了コード 3）で、何も作らない。
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
| `--compiler <名前>` | 使用するコンパイラ（msvc / clang / clang-cl / gcc / safec）。値の説明は [manifest.md](manifest.md) の `compiler`。MVP で `gcc` / `safec` を指定するとエラー（終了コード 3） |
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
- **作成先のディレクトリが既にある場合**はエラー（終了コード 2）。既存ディレクトリのプロジェクト化は `cuno init`（MVP 直後）で扱う。
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
| `cuno.toml` | `[package]` の `name` / `version`（`0.1.0`）/ `lang` / `edition` のみ。コンパイラは書かない（自動選択に任せる） |
| `src/main.cpp`（C は `src/main.c`） | `src/calc.cpp` の関数を呼ぶ `main` |
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

### `cuno run`

| オプション | 説明 |
|---|---|
| `--bin <名前>` | 実行する実行ファイル（複数ある場合は必須。省略するとエラーで候補を表示） |
| `-- <引数...>` | `--` 以降をプログラムに渡す |

- 実行時の作業ディレクトリは**プロジェクトのルート**（`cuno.toml` のある場所）。ゲームのアセットを相対パスで読む場合の基準になる。
- `cuno build` と同じようにビルドしてから（表示と失敗時の扱いも同じ）、選んだ実行ファイルを実行する。
- 実行ファイルの選び方: 1 つだけならそれ。`--bin` があればその名前のもの。どちらもビルドの前に確かめる。
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
- 削除の前に、出力先を [build.md](build.md#出力先ディレクトリ) の規則で確かめる。プロジェクトそのものやその上を指す出力先と、プロジェクトの設定（`cuno.toml`、`.cuno/config.toml`）がプロジェクトの外を指す出力先はエラー（終了コード 3）で、何も削除しない。
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

### `cuno lint`

| オプション | 説明 |
|---|---|
| `--fix` | 自動修正できる指摘を修正する |

### `cuno build` / `cuno check`

共通オプションのみ。

- プロジェクトの外で実行した場合はエラー（`error: could not find cuno.toml in <directory> or any parent directory`、終了コード 3）。設定にも誤りがあるとき（出力先の指定の誤り、読めない、または誤りのある利用者の `config.toml` など）も、プロジェクトがないことだけを示す。`cuno run`、`cuno test`、`cuno clean` も同じ。
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
- `cuno check` は、ビルドと同じソースを構文・型チェックだけのフラグ（[build.md](build.md#設定とフラグの対応)）で確認し、オブジェクトを作らずリンクもしない。表示は `Compiling` の代わりに `Checking` の行で、`Finished` の行は同じ。失敗したときはビルドのコンパイルの失敗と同じく `could not compile` の行（パッケージ名を続ける）、終了コード 1。
  - ビルドと同じ仕組みで、変更のないソースは確認し直さない（[build.md](build.md#変更の判定)）。
  - MSVC では、`check` が成功してもビルドが失敗することがある（コード生成の段階で出る診断を `/Zs` が出さないため。[build.md](build.md#常に付けるフラグ)）。

### `cuno doctor`

環境を診断し、問題ごとに対処法を表示する。

| オプション | 説明 |
|---|---|
| `--compiler <名前>` | 診断するコンパイラ。ビルド系コマンドの `--compiler` と同じ。指定しなければ、ビルド系コマンドと同じ選択の優先順位で選ばれるコンパイラを診断する（[toolchain.md](toolchain.md#選択)） |

- プロジェクトの外（`cuno.toml` が見つからない場所）でも実行できる。その場合はプロジェクトの層がない優先順位で選ぶ（[toolchain.md](toolchain.md#選択)）。
- 設定ファイル（`cuno.toml`、`.cuno/config.toml`、ユーザー単位の config）に誤りがあっても、MVP で未対応のコンパイラ（`gcc` / `safec`）が選ばれていても、`doctor` は止まらずに問題として報告し（終了コード `1`）、ほかの項目の診断を続ける。
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
- コンパイラのエラー・警告はそのまま表示する。最新で実行しなかった手順は、前回成功したときの出力（警告など）をそのまま表示する（[build.md](build.md#変更の判定)）。
  - 理由: 入力もコマンドも同じなので、出力も同じ。表示しないと、何も変えずにビルドし直しただけで警告が消え、見落とした警告をもう一度見る方法がなくなる。
  - 例外として、cl.exe がコンパイルのたびに最初の行に出すソースのファイル名（`main.cpp` など）は表示しない。出力の最初の行が、コンパイルしたソースのファイル名と一致する場合だけ消す。ASCII 以外の文字を含むファイル名は、cl がコンソールのコードページ（日本語版 Windows では CP932）で出すため、名前の先頭と末尾の ASCII の部分が一致し、`:` を含まない行をファイル名の行とみなす（Windows のファイル名は `:` を含まず、cl の診断は必ず含む）。
    - 理由: エラーでも警告でもない行で、MSVC のときだけソースの数だけ並び、ほかのコンパイラと表示が揃わないため。
  - 翻訳はしない（日本語版 Visual Studio の cl.exe なら日本語のまま）。cuno 自身のメッセージだけを英語に統一する。
  - Clang のように色付きで出力できるコンパイラは色付きで表示する。MSVC は色を出さないため、そのまま色なしで表示する。
    - cuno の表示に色を付けるとき（上の「全コマンドで使えるもの」の順位）だけ、clang と clang-cl に `-fcolor-diagnostics -fansi-escape-codes` を渡す。cuno はコンパイラの出力をパイプで受け取るため、指定しないと Clang は色を付けない。Windows では色をエスケープシーケンスで出すよう `-fansi-escape-codes` も要る。この 2 つは引数の先頭に置く。`[build.clang] flags` などに利用者が書いた `-fno-color-diagnostics` は後ろにあるため、そちらが優先される。リンクには渡さない（リンカの出力に色はない）。`-v` で表示するコマンドには、この 2 つも含める（実行したコマンドを表示するため）。
    - この 2 つは、変更の判定に使うコマンドライン（[build.md](build.md#変更の判定)）にも `compile_commands.json` にも含めない。理由: 色の指定を変えただけで全体を再コンパイルしないため。生成されるコードは変わらない。ただし Clang はデバッグ情報にコマンドラインを記録するため、デバッグ情報を付けたビルドでは、この 2 つの有無でオブジェクトファイルや PDB の中身が変わる（再コンパイルしないので、前のビルドのものが残る）。

## 終了コード

| コード | 意味 | 例 |
|---|---|---|
| `0` | 成功 | |
| `1` | 利用者のコード・成果物の問題 | コンパイルエラー、テスト失敗、`fmt --check` で未整形あり、lint の `deny` 違反、`doctor` で問題あり |
| `2` | コマンドの使い方の誤り | 不明なオプション、引数不足 |
| `3` | 設定・環境の問題 | `cuno.toml` の誤り、コンパイラが見つからない、コンパイラのバージョンが条件に合わない |
| `101` | cuno 自身の内部エラー（バグ） | 想定外の失敗。Rust のパニックと同じ値 |

- `cuno run` は、実行したプログラムの終了コードをそのまま返す（cargo と同じ）。ビルドに失敗した場合は上の表に従う。
- 理由: CI で「コードの問題（1）」と「環境の問題（3）」を見分けられると、失敗の原因切り分けが速くなる。

## 未決

- MVP 外のコマンドのオプション
