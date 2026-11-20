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

- Clang を 17 以降とする理由: VS 2022 の標準ライブラリ（MSVC STL）は Clang 17 未満をエラー（STL1000）にする（下の確認が扱う問題の一例）。また、GNU 形式の `clang` で C ランタイムの種類を指定する `-fms-runtime-lib` は Clang 16 以降でしか使えない（指定しないと既定が静的ランタイムになり、`runtime = "dynamic"` の既定と食い違う）。
- **Windows では、MSVC 標準ライブラリ（MSVC STL）が受け付ける Clang であることも条件とする。** MSVC STL は自分の下限より古い Clang をエラー（STL1000）にし、その下限は Visual Studio の更新とともに上がる（例: MSVC 14.52 は Clang 22 以上を要求する）。固定の数値では表せないため、cuno は標準ライブラリ自身に確かめる。
  - 方法: 選ばれた Clang と、解決した Windows の環境で、`<yvals_core.h>` だけを含むファイルを構文チェックだけでコンパイルする（`clang-cl` は `/Zs`、`clang` は `-fsyntax-only`）。STL1000 で失敗したら、その組み合わせは使えない。必要な Clang のバージョンは、標準ライブラリのメッセージ（`expected Clang N or newer`）から取り、`help:` で案内する。
  - 理由: ヘッダの条件式を読んだり、MSVC の版ごとの対応表を cuno が持ったりすると、新しい Visual Studio で書き方や版が変わったときに、使えない組み合わせを「問題なし」と判定しうる。試しのコンパイルは約 35 ミリ秒で、標準ライブラリの答えそのものを使える。
- 最低バージョンと上の確認は、`cuno doctor` とビルド系コマンドが同じ 1 つの判定を使う。`cuno doctor` は満たさなければ「問題あり」（終了コード 1）、ビルド系コマンドはコンパイルの前にエラー（終了コード 3）にする。

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
