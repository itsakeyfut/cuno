# cuno 本体の内部構成

## クレート構成

Rust のワークスペースで、役割ごとにクレートを分ける。

| クレート | 役割 | 時期 |
|---|---|---|
| `cuno` | CLI。コマンドを解釈し、各クレートを呼び出す。単一バイナリの本体 | MVP |
| `cuno-core` | 複数のクレートが共通で使う 3 つだけ：エラーと終了コードの分類、出力（cargo 風の表示・色・詳細度）、cuno home の解決 | MVP |
| `cuno-manifest` | `cuno.toml` / `config.toml` の読み込みと検証（[manifest.md](manifest.md)） | MVP |
| `cuno-toolchain` | コンパイラ抽象化（インターフェース）、検出、各コンパイラの実装、フラグ翻訳（[toolchain.md](toolchain.md)） | MVP |
| `cuno-build` | ビルドグラフの構築・実行、差分判定、compile_commands.json の出力（[build.md](build.md)） | MVP |
| `cuno-test` | テストの検出・実行・結果表示（[testing.md](testing.md)） | MVP |
| `cuno-tools` | fmt / lint（clang-format / clang-tidy の呼び出し）（[dev-tools.md](dev-tools.md)） | MVP |
| `cuno-script` | Lua ビルドスクリプトの組み込み | 後 |
| `cuno-deps` | 依存の解決・取得・ロックファイル（[dependencies.md](dependencies.md)） | 依存管理 段階 1 |

- `cuno-build` の中で、「ビルドグラフの構築」と「グラフの実行」をモジュールとして分ける（将来の Ninja バックエンドの余地）。
  - グラフのノード（コンパイルなどの 1 手順）は、実行するプログラム・引数・環境変数・作業ディレクトリ・入力・出力だけを持つデータにする。理由: compile_commands.json と将来の Ninja バックエンドを、グラフを実行せずにグラフから作れるようにするため。

### `cuno-core` に置くもの

次の 3 つに限る。ほかのものを足すときは、本仕様書を先に変える。

| もの | 内容 |
|---|---|
| エラーの分類 | 終了コードの分類（1: 利用者のコード・成果物の問題、2: 使い方の誤り、3: 設定・環境の問題、101: cuno 自身の内部エラー。[commands.md](commands.md#終了コード)）と、各クレートのエラー型が実装する共通のトレイト（分類と、任意の `help:` を返す）。表示と終了は cuno 本体が 1 か所で行う |
| 出力 | cargo 風の進捗表示（`Compiling` / `Finished` / `Running`）、警告とエラー、色と詳細度（`[term]`、`NO_COLOR`）。各クレートは直接表示せず、これを通して書く |
| cuno home | cuno home の場所の決定（`CUNO_HOME`、なければ `%USERPROFILE%\.cuno`。[distribution.md](distribution.md)） |

- 理由: ライブラリのクレートは cuno 本体に依存できない。共通の置き場がないと、各クレートが終了コードや表示を独自に決めるか、本体が全クレートのエラーを知って終了コードに対応づけることになる。後者では、対応を書き忘れたエラーが黙って終了コード 1 になる。

## クレート間の依存

依存してよい向きを次に限る。`tests/architecture.rs` が各クレートの `Cargo.toml` を検査し、表にない依存や、表にないクレートがあると失敗する。

| クレート | 依存してよいクレート |
|---|---|
| `cuno-core` | なし |
| `cuno-toolchain` | `cuno-core` |
| `cuno-manifest` | `cuno-core`、`cuno-toolchain` |
| `cuno-build` | `cuno-core`、`cuno-manifest`、`cuno-toolchain` |
| `cuno-test` | `cuno-core` |
| `cuno-tools` | `cuno-core`、`cuno-manifest`、`cuno-toolchain` |
| `cuno-deps`（後） | `cuno-core`、`cuno-manifest`、`cuno-toolchain` |
| `cuno-script`（後） | `cuno-core`、`cuno-build` |
| `cuno`（本体） | すべて |

- **`cuno-manifest` が `cuno-toolchain` に依存し、逆はない。** コンパイラの種類と、フラグ翻訳が受け取る設定の型は `cuno-toolchain` が定義し、`cuno-manifest` は TOML をその型へ読み込む（`[build.<compiler>]` もコンパイラの種類で引く）。
  - 理由: コンパイラの抽象化に TOML の構造を持ち込まないため。コンパイラの追加が `cuno-toolchain` の変更だけで済む（[concept.md](../concept.md) の設計原則 5）。
- `cuno-test` はテストの実行と結果の表示だけを持つ。テストのビルドは `cuno-build` の 1 つのモードとする。
- `cuno-tools` には compile_commands.json のパスを渡す。`cuno-build` には依存しない。
- `cuno-build` は `cuno-deps` / `cuno-script` に依存しない。解決済みの依存やスクリプトの結果は、cuno 本体がデータとして渡す。
- **ライブラリのクレートは、プロセスの環境変数やコマンド引数を直接読まない。** cuno 本体が集めて値として渡す（環境変数の一覧を cuno home の解決やコンパイラの検出に渡す、など）。
  - 理由: テストで任意の環境を作れるようにし、ライブラリの振る舞いがたまたま動いているプロセスの環境に左右されないようにするため。

## 共通のルールの持ち主

複数のコマンドが使うルールは、次のクレートが 1 か所で持つ。各コマンドはそれを呼び、同じルールを書き直さない。

| ルール | 持ち主 |
|---|---|
| コマンド引数の解釈、各処理の呼び出し、エラーの表示と終了コードの決定 | `cuno`（`cuno-core` の分類を使う） |
| エラーの分類、出力、cuno home | `cuno-core` |
| プロジェクトの探し方、パスの解決、`cuno.toml` と設定ファイルの読み込み・検証、優先順位と重ね合わせ（1 つの解決済みの設定にまとめる） | `cuno-manifest` |
| コンパイラの種類、検出、選択の優先順位、Windows での環境解決、バージョンの条件、フラグ翻訳 | `cuno-toolchain` |
| ソースの分類（エントリファイル・パッケージ本体・テスト）、出力先のパス、ビルドグラフ、テストのビルド、compile_commands.json、同梱ヘッダ | `cuno-build` |
| テストの実行と結果の表示 | `cuno-test` |
| fmt / lint、`.clang-format` / `.clang-tidy` の生成 | `cuno-tools` |

## リポジトリ構成

```
cuno/
├── Cargo.toml        # ワークスペース、かつ cuno パッケージ（バイナリ）
├── src/main.rs       # cuno バイナリ本体
├── crates/           # cuno 以外の上記クレート（ライブラリ）
├── headers/cuno/     # 同梱ヘッダ（test.hpp / test.h）。ビルド時にバイナリへ埋め込む
├── templates/        # cuno new が生成するサンプル
├── tests/            # cuno 自体の結合テスト（実際にコンパイラを呼ぶ）
└── docs/specs/       # 仕様書
```

- ルートはワークスペースであると同時に `cuno` パッケージでもある。
  - 理由: ルートの `tests/` は、ルートがパッケージでなければどのパッケージのテストにもならず、ビルドされない。ルートを `cuno` パッケージにすることで、`tests/` が cuno バイナリの結合テストとして動く。
  - あわせて、ルートで `-p cuno` を付けずに `cargo run` / `cargo install --path .` が使える。

## 主な外部ライブラリ

| ライブラリ | 用途 | 時期 |
|---|---|---|
| clap | コマンド引数の解析 | MVP |
| serde + toml | 設定ファイルの読み込み | MVP |
| mlua | Lua の組み込み | 後 |

## cuno 自体のテスト

- クレートごとのユニットテスト。
- 実際に MSVC / Clang を呼ぶ結合テスト（`tests/`）。
- CI: GitHub Actions の Windows ランナー（MSVC が入っている）。

## Rust

- edition 2024。
- **MSRV（最低サポートバージョン）は 1.88。**
  - 理由: edition 2024 で `let` チェーン（`if let Some(x) = a && b`）が使える最初のバージョン。設定の解決など、条件が重なる処理が多い cuno ではコードが大きく読みやすくなる。
  - cuno はビルド済みバイナリで配布するため、MSRV が影響するのはソースからビルドする利用者だけ。そのため、edition 2024 の最小（1.85）より新しくしても不利益は小さい。
  - MSRV を上げるのは意図的な変更とし、本仕様書を更新する。
- ツールチェーンは固定しない（`rust-toolchain.toml` を置かない）。CI で MSRV と stable の両方を確認する。

## 未決

- 並列実行の方式（スレッドプール / 非同期）
