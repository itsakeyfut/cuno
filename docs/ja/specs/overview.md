# 概要

## 目的

cuno は C/C++ 向けの統合ツールである。パッケージ管理・ビルド管理・ツールチェーン管理・開発ツールを一つのコマンドに統合し、**cargo と同じ使用感**で C/C++ 開発を始め、続けられるようにする。

置き換える対象: CMake、Ninja、ctest、`.clang-format` / `.clang-tidy` の個別管理、など。

## 解決するペイン

1. 設定（CMakeLists.txt / .cmake）が読みにくく、ファイルが分散している
2. IDE なしでプロジェクトを気軽に始められない
3. テストの仕組み（ctest / GTest / Catch 等）が乱立している
4. ビルド・実行の引数（`-Werror` 等）が直感的でない
5. ライブラリ導入時にハッシュ値を自分で調べる必要がある。vcpkg も分かりにくい

## 利用者

- 最初の利用者: cuno の開発者自身。**ゼロから始める新規プロジェクト**で使う。
- 将来: ゲーム開発などの実務での採用。
- CMake は**完全に置き換える**。既存 CMake プロジェクトとの共存は目標にしない。

## ゴール（MVP）

Windows 11 上で次の一連の流れが動くこと:

```
cuno new mygame   # プロジェクト + サンプルコード（テスト付き）を生成
cd mygame
cuno fmt          # 整形
cuno lint         # 静的解析
cuno test         # テスト実行
cuno run          # ビルドして実行
```

- 作業が重い場合の最低ラインは `cuno new` / `cuno test` / `cuno run`。
- MVP に含むコマンドの全体は `new` / `build` / `run` / `check` / `clean` / `test` / `fmt` / `lint` / `doctor`（[commands.md](commands.md)）。
- C と C++ の両方を扱う（`cuno new --lang c`、既定は C++）。
- Debug / Release を切り替えられる（`--release`）。

## 非ゴール（MVP 外。MVP 直後以降で扱う）

- 依存ライブラリ管理（`cuno add` 等）
- 外部テストフレームワーク（GTest / Catch2 / doctest）対応
- GCC 対応（MVP 直後）
- コンパイラの自動インストール
- C++20 モジュール
- Linux / macOS 対応
- Ninja バックエンド
- 公開パッケージサービス

## アンチゴール（動いても失敗とみなすもの）

- 利用者が CMake や他のビルド DSL を書く必要がある
- 設定が `Cuno.toml` + `.cuno/config.toml` を超えて多数のファイルに分散する（ユーザー単位の `<管理ディレクトリ>/config.toml` と、逃げ道のビルドスクリプトは例外。[manifest.md](manifest.md)）
- 始めるのに IDE が必要
- 依存のハッシュ値を利用者が手で調べる必要がある
- コンパイラ固有のフラグ（`/WX`, `-Werror` 等）を利用者が覚える必要がある

## 実装言語

- cuno 本体は **Rust** で実装する。
- 極限まで最適化が必要な箇所では、計測で効果が確認できる場合に限り **C やアセンブラ**も使ってよい。

## 将来の検討事項

- safec 対応（主要コンパイラで成立させた後。[toolchain.md](toolchain.md)）
