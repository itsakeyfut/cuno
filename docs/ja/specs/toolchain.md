# ツールチェーン

## 対象コンパイラ

主要な 3 系統すべてを対象とする。

| 系統 | コンパイラ |
|---|---|
| GCC 系 | GCC、MinGW-w64 |
| Clang 系 | Clang、clang-cl、Apple Clang、Emscripten、Android NDK、Intel oneAPI (icx) |
| MSVC 系 | cl.exe |

- オプションの書き方は GCC 系と Clang 系がほぼ共通、MSVC と clang-cl が共通。
- NVCC（CUDA）は後回し。
- コンソール SDK（PS5 / Switch 等）は NDA のため公式対応しないが、拡張で対応できる設計にする。

### safec

- cuno の開発者が別途開発している実験的コンパイラ **safec**（https://github.com/itsakeyfut/safec）も、Clang / MSVC / GCC と並ぶコンパイラの選択肢の一つとする（`compiler = "safec"`）。
  - safec: 独自の Safety IR で C/C++ を再設計し、メモリ安全・スレッド安全・ライフタイム・所有権など Rust の安全性をどこまで C/C++ に持ち込めるかを実験するコンパイラ。
  - safec は開発者自身が設計を制御できるため、他のコンパイラより深い連携（診断の構造化出力など）も可能。
- 他のコンパイラと同じく、コンパイラ抽象化のインターフェースを実装する形で対応する。
- 時期: 主要コンパイラで cuno が成立した後。
- **保留: safec の出力形式**。現時点の safec は、他のコンパイラでビルドしたライブラリとリンクできない。出力形式は、safec 対応に着手する時点の safec の状態を見て決める。コンパイラ抽象化は次のどちらにも対応できる余地を残す:
  - ネイティブのオブジェクトファイルを直接出力する方式
  - C 等に変換し、後段で既存コンパイラにコンパイルさせる 2 段方式

### MVP の対象

- OS: Windows 11
- コンパイラ: **MSVC** と **Clang（clang-cl を含む）**
- GCC は MVP 直後に追加する。

## 言語規格

| | 最低サポート | `cuno new` の既定 |
|---|---|---|
| C++ | C++17 | C++20 |
| C | C11 | C17 |

- `edition` で指定した規格を、使用するコンパイラが受け付けない場合（例: 古い MSVC で `c++23`）はエラーにし、必要なコンパイラのバージョンを案内する。
- `edition` と各コンパイラの規格フラグ（`/std:c++20`、`-std=c++20` 等）の対応表は実装時に作り、本仕様書に書き戻す。

### コンパイラの最低バージョン

C++20 の指定を受け付けるものとする。

| コンパイラ | 最低バージョン |
|---|---|
| MSVC | VS 2019 16.11 以降（cl.exe 19.29 以降） |
| Clang | 17 以降、かつ Windows では使用する MSVC 標準ライブラリが受け付けるもの |
| GCC | 10 以降 |

- Clang を 17 以降とする理由: cuno が対応する Clang の最も古い版として、すべての OS で共通の下限を置く。17 は VS 2022 17.10〜17.12 の標準ライブラリ（MSVC STL）が要求する版で、それ以前の Clang を対象に含めない。また、GNU 形式の `clang` で C ランタイムの種類を指定する `-fms-runtime-lib` は Clang 16 以降でしか使えない（指定しないと既定が静的ランタイムになり、`runtime = "dynamic"` の既定と食い違う）。17 は下限であり、Windows では下の確認でさらに上がりうる。
- **Windows では、MSVC 標準ライブラリ（MSVC STL）が受け付ける Clang であることも条件とする。** MSVC STL は自分の下限より古い Clang をエラー（STL1000）にし、その下限は Visual Studio の更新とともに上がる。固定の数値では表せないため、cuno は標準ライブラリ自身に確かめる。
  - 下限の例（MSVC STL の `yvals_core.h`）: VS 2022 17.0 は Clang 12、17.7 は 16、17.10 は 17、17.13 は 18、17.14 は 19、MSVC 14.51 は 20、14.52 は 22。
  - **確認の方法**: `<yvals_core.h>` だけを含むファイルを、ビルドのコンパイルコマンドと同じ条件（同じ Clang、解決した Windows の環境、ターゲット `x86_64-pc-windows-msvc`、インクルードパス、プロジェクトの `defines`）で、**C++ として**構文チェックだけ行う（`clang-cl` は `/TP /Zs`、`clang` は `-x c++ -fsyntax-only`）。ファイルはプロジェクトの外（一時ディレクトリ）に置く。
    - C++ として行う理由: C として扱うと標準ライブラリが別のエラー（STL1003）で止まるため。
    - `defines` を渡す理由: MSVC STL は `_ALLOW_COMPILER_AND_STL_VERSION_MISMATCH` が定義されていれば STL1000 を出さない。利用者がこれを `defines` に書いて意図的に許した組み合わせは、実際のビルドと同じく通す。
  - **判定**: 終了コードが 0 でなく、標準出力・標準エラーのどちらかに `STL1000:` があれば、その組み合わせは使えない。必要な Clang の版は、標準ライブラリのメッセージ `expected Clang <バージョン> or newer` の `<バージョン>` から取り、そのままの形で示す（`22` のことも `17.0.0` のこともある）。取り出せなければ、標準ライブラリのメッセージをそのまま `help:` に出す。
  - **判定できない場合**: STL1000 以外の理由で失敗したとき（ヘッダが見つからない、他の STL エラー等）、または 30 秒以内に終わらないときは、組み合わせの可否を判定できない。通ったとは扱わず、環境の問題としてコンパイラの出力を示す（終了コードは下と同じ）。
  - 理由: ヘッダの条件式を読んだり、MSVC の版ごとの対応表を cuno が持ったりすると、新しい Visual Studio で書き方や版が変わったときに、使えない組み合わせを「問題なし」と判定しうる。試しのコンパイルは数十ミリ秒で、標準ライブラリの答えそのものを使える。
- **clang-tidy にも同じ確認を適用する。** clang-tidy は compile_commands.json のコマンドで解析し、MSVC の標準ヘッダを読むので、古い clang-tidy は STL1000 で全ファイルの解析に失敗する。
  - 方法: 同じファイルを、clang-tidy に `--checks=-*,readability-braces-around-statements`（チェックが 1 つもないと解析せずに止まるため、影響のない 1 つだけを有効にする）と、`--` に続けて上と同じコンパイルの条件を渡して解析させる。プロジェクトの `.clang-tidy` は読ませない。STL1000 は clang-tidy では標準出力に出る。判定は上と同じ。
- **どこで確認するか**:

  | コマンド | 確認するもの | C だけのプロジェクト（`[package] lang = "c"`） |
  |---|---|---|
  | ビルド系コマンド | 選ばれた Clang | 行わない（C++ の標準ライブラリを使わないため） |
  | `cuno lint` | clang-tidy | 行わない |
  | `cuno doctor`（プロジェクトの中・外） | 選ばれた Clang と clang-tidy | 行わない |

  - 選ばれていない Clang（MSVC を選んでいるときの Clang 等）は確認しない。
- 最低バージョンと上の確認は、`cuno doctor` とビルド系コマンドが同じ 1 つの判定を使う。`cuno doctor` は満たさない・判定できない場合に「問題あり」（終了コード 1）、ビルド系コマンドと `cuno lint` は処理の前にエラー（終了コード 3）にする。
- 拒否されたときの表示:

  ```
  error: clang 20.1.8 is not accepted by the MSVC standard library in use (STL1000)
  help: install Clang 22 or newer, or select another compiler
  ```

  clang-tidy の場合は `error: clang-tidy 20.1.8 is not accepted by ...`、`help: install clang-tidy 22 or newer` とする。

- cuno 内部および `cuno.toml` の `[toolchain.version]`、出力メッセージでは、MSVC のバージョンを **cl.exe のバージョン**（例: `19.29`）で表す。Visual Studio の製品バージョン（例: `16.11`）は使わない。

## コンパイラ抽象化

- コンパイラ系統ごとの差（検出、`cuno.toml` 設定からフラグへの翻訳、依存ヘッダ情報の取得、診断出力の解釈）を **1 つのインターフェースの背後に閉じ込める**。
- 新しいコンパイラは、そのインターフェースの実装を 1 つ追加するだけで対応できる。他の開発者による追加（GCC 等）を想定し、追加手順をドキュメント化する。
- `cuno.toml` の `[build]` 設定（例: `warnings-as-errors = true`）を各系統のフラグ（`-Werror` / `/WX`）に翻訳する。

## Windows での環境解決

- 利用者が Developer Command Prompt を開かなくても MSVC を使えること。
- Visual Studio（または Build Tools）の場所と、必要な環境（環境変数等）は cuno が自動で解決する。
- **Clang（`clang` / `clang-cl`）にも同じ解決を適用する**。Windows の Clang も MSVC のヘッダ・ライブラリ・Windows SDK を使うため。
- Visual Studio / Build Tools が見つからない場合は、Clang を選んでいてもエラーにし、`cuno doctor` と同じ案内を出す。

### Windows の Clang は MSVC 互換で動かす

- `clang` / `clang-cl` とも、ビルド対象は `x86_64-pc-windows-msvc`（MSVC と同じ ABI）。MSVC でビルドしたライブラリと混ぜてリンクできる。
- MinGW 系の Clang は対象外（GCC / MinGW 対応時に扱う）。

### リンカ

| コンパイラ | リンカ | 理由 |
|---|---|---|
| `msvc` | `link.exe` | MSVC 標準 |
| `clang` / `clang-cl` | `lld-link`（LLVM 同梱） | `link.exe` より高速 |

## 検出

- PATH、Visual Studio のインストール先（vswhere）、LLVM の標準インストール先、環境変数から探す。
- `cuno toolchain list` で検出したコンパイラを一覧表示する。

### Visual Studio / Build Tools

- **vswhere だけで探す。** `%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe` を `-all -products * -prerelease -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -utf8 -format json` で実行し、C++ の x64 ツールを含むインストールをすべて得る。
  - 理由: vswhere は Visual Studio 2017 以降と Build Tools に必ず付き、インストール先を正しく知っている唯一の方法。`-utf8 -format json` で、コンソールのコードページ（日本語版 Windows では CP932）に左右されずに読める。
- 各インストールの MSVC ツールセットは、`VC\Tools\MSVC\<バージョン>` のうち `bin\Hostx64\x64\cl.exe` があるもの。既定のツールセットは `VC\Auxiliary\Build\Microsoft.VCToolsVersion.default.txt` に書かれた版。
- **cl.exe のバージョンは、cl.exe を引数なしで実行し、その表示の最初の `<数>.<数>.<数>` から読む。** 表示の言語（日本語版の Visual Studio を含む）に関係なく、数字の部分は同じだから。ツールセットのフォルダ名（例: `14.52.37025`）は cl.exe のバージョン（例: `19.52.37029`）と一致しないので使わない。
- プレビュー版も検出はする（`cuno doctor` で示せるように）。選ぶかどうかは「選択」が決め、選ばない。
- C++ の x64 ツールを含むインストールが 1 つもない、または vswhere がない場合はエラー（終了コード 3）にし、Build Tools の「C++ によるデスクトップ開発」ワークロードの入れ方を案内する。

## 選択

優先順位（上ほど優先）:

1. コマンド引数 `--compiler`
2. 環境変数 `CUNO_COMPILER`
3. プロジェクト単位の `.cuno/config.toml`
4. `cuno.toml` の `[toolchain]`
5. ユーザー単位の `<管理ディレクトリ>/config.toml`
6. 自動選択

- プロジェクトで決めた設定（`cuno.toml`）は、個人の全体設定（ユーザー単位の config）より優先する。
- プロジェクトの外（`cuno doctor` をプロジェクトの外で実行した場合など）では、プロジェクトの層（`.cuno/config.toml` と `cuno.toml` の `[toolchain]`）がないので、`--compiler` → `CUNO_COMPILER` → ユーザー単位の config → 自動選択 の順で選ぶ。
  - 理由: 新しいプロジェクトで `cuno build` したときに使われるコンパイラと一致させるため。自動選択だけを見ると、ユーザー単位の config で clang を選んでいる人に MSVC を診断して「問題なし」と答え、その後のビルドが失敗しうる。
- MVP で未対応のコンパイラ（`gcc` / `safec`）が選ばれた場合はエラー（終了コード 3）。警告して別のコンパイラでビルドすることはしない（黙って別のコンパイラでビルドされるのを防ぐため）。

自動選択の既定: Windows は MSVC、Linux は GCC、macOS は Apple Clang。

- Windows で Visual Studio / Build Tools が見つからない場合はエラーにする（Clang も Visual Studio を必要とするため、Clang への切り替えでは解決しない）。
- **同じコンパイラが複数バージョン入っている場合**は、バージョン制約を満たすもののうち最新を選ぶ。プレビュー版は選ばない。

## バージョン固定

- `cuno.toml` の `[toolchain.version]` に、コンパイラごとの範囲または厳密なバージョンを書く（[manifest.md](manifest.md)）。
- 実際に使用するコンパイラの制約だけを確認する。
- 満たさない場合はエラーにし、対処法を案内する。
- rust-toolchain.toml のような別ファイルは作らない（設定ファイルを増やさない方針）。

## 自動インストール（将来）

| コンパイラ | 方式 |
|---|---|
| LLVM / Clang | 公式配布物をダウンロードし、cuno の管理下に置く |
| MinGW（GCC） | 同上 |
| MSVC | ライセンス上 cuno からは再配布できない。公式 Build Tools インストーラを必要な構成で起動する |

## クロスコンパイル（後）

- MVP ではビルドするマシンと同じ環境向けのビルドのみ。`--target` オプションは予約のみ（[commands.md](commands.md)）。
- `--target <ビルド対象環境>` で指定する。名前は Rust / LLVM の表記に合わせる（例: `x86_64-pc-windows-msvc`、`aarch64-linux-android`、`wasm32-unknown-emscripten`）。
- 各環境のツール（Android NDK、Emscripten 等）の場所は `.cuno/config.toml` に書くか、自動検出する。
- ビルドスクリプトは Lua のため、ビルドするマシン用のコンパイラは不要。

## 未決

- MSVC 環境の具体的な解決方法（vswhere の利用など）
- 検出の詳細な探索順序
- 対応するビルド対象環境の一覧と、各環境のツールの検出方法
