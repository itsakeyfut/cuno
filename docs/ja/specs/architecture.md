# cuno 本体の内部構成

## クレート構成

Rust のワークスペースで、役割ごとにクレートを分ける。

| クレート | 役割 | 時期 |
|---|---|---|
| `cuno` | CLI。コマンドを解釈し、各クレートを呼び出す。単一バイナリの本体 | MVP |
| `cuno-manifest` | `cuno.toml` / `config.toml` の読み込みと検証（[manifest.md](manifest.md)） | MVP |
| `cuno-toolchain` | コンパイラ抽象化（インターフェース）、検出、各コンパイラの実装、フラグ翻訳（[toolchain.md](toolchain.md)） | MVP |
| `cuno-build` | ビルドグラフの構築・実行、差分判定、compile_commands.json の出力（[build.md](build.md)） | MVP |
| `cuno-test` | テストの検出・実行・結果表示（[testing.md](testing.md)） | MVP |
| `cuno-tools` | fmt / lint（clang-format / clang-tidy の呼び出し）（[dev-tools.md](dev-tools.md)） | MVP |
| `cuno-script` | Lua ビルドスクリプトの組み込み | 後 |
| `cuno-deps` | 依存の解決・取得・ロックファイル（[dependencies.md](dependencies.md)） | 依存管理 段階 1 |

- `cuno-build` の中で、「ビルドグラフの構築」と「グラフの実行」をモジュールとして分ける（将来の Ninja バックエンドの余地）。

## リポジトリ構成

```
cuno/
├── Cargo.toml        # ワークスペース
├── crates/           # 上記クレート
├── headers/cuno/     # 同梱ヘッダ（test.hpp / test.h）。ビルド時にバイナリへ埋め込む
├── templates/        # cuno new が生成するサンプル
├── tests/            # cuno 自体の結合テスト（実際にコンパイラを呼ぶ）
└── docs/specs/       # 仕様書
```

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

## 未決

- Rust の最低サポートバージョン（MSRV）
- 並列実行の方式（スレッドプール / 非同期）
- エラー型の設計
