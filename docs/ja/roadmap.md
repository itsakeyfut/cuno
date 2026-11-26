# cuno ロードマップ

> 各機能の仕様は [specs/](specs/README.md) を正とし、このドキュメントは「どの順番で、何をもって完了とするか」を示します。
> 時期の表記（MVP / MVP 直後 / 依存管理 段階 / 後 / 将来）は [specs/README.md](specs/README.md) の定義に従います。

## 進め方

- マイルストーンは**順番に**進めます。あるマイルストーンの内容がすべて完了してから、次のマイルストーンに進みます。
- **期限は設けません**。各マイルストーンの完了条件を満たしたときが、そのマイルストーンの完了です。

## 全体像

| マイルストーン | 内容 | バージョン | 仕様上の時期 |
|---|---|---|---|
| M0 | 開発基盤 | — | — |
| M1 | 設定とツールチェーン | — | MVP |
| M2 | ビルドと実行 | — | MVP |
| M3 | テスト | — | MVP |
| M4 | 整形・静的解析と MVP の仕上げ | v0.1.0 | MVP |
| M5 | GCC とツールチェーン管理 | v0.2.0 | MVP 直後 |
| M6 | ライブラリと依存管理（Git / URL） | v0.3.0 | 依存管理 段階 1 |
| M7 | レシピインデックスとカスタムビルド | v0.4.0 | 依存管理 段階 2 / 後 |
| M8 | マルチプラットフォーム | v0.5.0 | 後 |
| M9 | IDE 連携とビルドの効率化 | v0.6.0 | 後 |
| M10 | テストと配布の拡充 | v0.7.0 | 後 |
| — | 将来の検討事項 | — | 将来 |

- M1〜M4 で MVP です。MVP は大きいため、動くものを順に積み上げられるよう 4 つに分けています。
- M6 以降の順番は、依存関係が強いもの（依存管理はライブラリ対応が前提、など）を先にしています。
- SDL3 などの既存ライブラリを使えるようにすること（M7）を、Linux / macOS 対応（M8）より優先します。ゲーム開発での実用性を先に確立するためです。
- GCC 対応（M5）は急ぎではありませんが、依存管理（M6）より前に置きます。3 系統目のコンパイラを早めに入れることで、コンパイラ抽象化が MSVC / Clang に偏った設計になっていないかを早い段階で確かめるためです。

## M0: 開発基盤

cuno 自体を開発するための土台を作ります。

- Rust ワークスペースとクレートの骨組み（[architecture.md](specs/architecture.md)）
- CI（GitHub Actions の Windows ランナー。ビルド・テスト・clippy・rustfmt）
- 結合テストの仕組み（実際に MSVC / Clang を呼ぶテストを `tests/` で動かせる）

**完了条件**: 空のクレート群が CI でビルド・テストされる。

## M1: 設定とツールチェーン

cuno がプロジェクトと環境を正しく理解できるようにします。

- `cuno.toml` / `.cuno/config.toml` の読み込みと検証（未知のキーの警告、MVP 外の項目の警告 / エラー）
- プロジェクトの探し方（親ディレクトリをさかのぼる）とパスの解決
- コンパイラの検出（Visual Studio / Build Tools、LLVM）と選択の優先順位
- Windows での環境解決（Developer Command Prompt なしで MSVC / Clang を使える）
- 設定値からコンパイラのフラグへの翻訳（MSVC / clang / clang-cl）
- `cuno doctor`

**完了条件**: `cuno doctor` が、MSVC / Clang の有無とバージョン、clang-format / clang-tidy の有無を正しく診断できる。

## M2: ビルドと実行

C/C++ のプロジェクトをビルドして実行できるようにします。

- ソースの集め方（エントリファイルとパッケージ本体）
- 自前ビルドエンジン（ビルドグラフ、差分ビルド、並列コンパイル）
- ヘッダ依存の取得（`/sourceDependencies`、`-MD`、`/showIncludes`）
- 出力先（`target/`）とプロファイル（`dev` / `release`）
- compile_commands.json の生成
- `cuno build` / `cuno run` / `cuno check` / `cuno clean`

**完了条件**: 手書きの `cuno.toml` を持つプロジェクトを MSVC / Clang の両方で `cuno run` でき、ヘッダだけを変更したときも正しく差分ビルドされる。

## M3: テスト

テストを書いて実行できるようにします。

- 同梱ヘッダ（`cuno/test.hpp` / `cuno/test.h`）と、そのバイナリへの埋め込み・展開
- テストビルド（`CUNO_TEST`、エントリファイルの除外）
- テストランナー（1 テスト 1 プロセス、並列実行、出力の捕捉）
- `cuno test`
- `cuno new`（テスト付きのサンプルを生成）

**完了条件**: `cuno new` → `cuno test` → `cuno run` が、C / C++ の両方、MSVC / Clang の両方で動く。

## M4: 整形・静的解析と MVP の仕上げ

MVP の流れを完成させ、最初のリリースを出します。

- `cuno fmt` / `cuno lint`（clang-format / clang-tidy の呼び出し、preset、allow / warn / deny）
- `.clang-format` / `.clang-tidy` の生成
- 出力メッセージ・終了コードの仕上げ
- 配布（GitHub Releases のバイナリ）

**完了条件**: [overview.md](specs/overview.md) に定める MVP の流れ（`cuno new` → `cuno fmt` → `cuno lint` → `cuno test` → `cuno run`）が Windows 11 で動き、v0.1.0 として配布されている。

## M5: GCC とツールチェーン管理

- GCC 対応（コンパイラ抽象化の追加手順のドキュメント化を含む）
- `cuno init`
- `cuno toolchain list` / `cuno toolchain default`
- `cuno self update`（公開された最新版への更新。v0.1.0 で配布の形が決まった後に、それを前提に設計する）

**完了条件**: GCC で MVP の流れが動く。動作確認の環境は、着手時点で手間が少ない方（Windows の MinGW、または Linux）を選ぶ。v0.1.0 の後に入れた cuno が、`cuno self update` で公開された最新版に更新できる。

## M6: ライブラリと依存管理（Git / URL）

cuno 形式のパッケージ同士を依存として使えるようにします。

- ライブラリプロジェクト（`[lib]`、`cuno new --lib`、static / shared / header-only）
- `[dependencies]`（Git / URL 指定）、`cuno add` / `remove` / `update` / `tree` / `fetch`
- `cuno.lock`（ハッシュ値の自動取得と記録）
- バージョン解決（1 ライブラリ 1 バージョン）

**完了条件**: Git 上の cuno 形式ライブラリを `cuno add --git` で追加し、ハッシュ値を調べずにビルドできる。

## M7: レシピインデックスとカスタムビルド

既存のライブラリ（SDL3、FFmpeg 等）を `cuno add` だけで使えるようにします。

- レシピインデックス（Git リポジトリ）と `cuno add sdl3`
- レシピに従って既存ビルドシステム（CMake 等）を裏で呼ぶ仕組み
- `cuno vendor`
- features、システムライブラリ（`system = true`）、ライセンス表示と警告
- カスタムルール（`[[build.rule]]`）と Lua ビルドスクリプト（レシピでも必要になるため、ここで実装する）。どちらも [設計原則 9](concept.md#9-複雑な設定も書け黙って間違えない)（複雑な設定も書け、黙って間違えない）に従って設計する

**完了条件**: SDL3 を使ったプロジェクトを `cuno add sdl3` → `cuno run` で動かせる。

## M8: マルチプラットフォーム

- Linux / macOS 対応（cuno 自体の配布を含む）
- クロスコンパイル（`--target`、Android / WebAssembly）

**完了条件**: Linux / macOS で MVP の流れが動き、Windows から Android / WebAssembly 向けにビルドできる。

## M9: IDE 連携とビルドの効率化

日々の開発を快適にします。

- IDE 連携（`cuno ide vs` / `cuno ide vscode`、`cuno metadata`）
- ビルドの高速化（ビルドキャッシュ、プリコンパイルヘッダ、ユニティビルド）
- 独自プロファイル（`inherits`）、`[alias]`
- ワークスペース（複数パッケージ）

**完了条件**: Visual Studio のデバッガで cuno のプロジェクトをデバッグでき、ワークスペースで複数パッケージ（例: engine / game / editor）をまとめてビルドできる。

## M10: テストと配布の拡充

- テストの拡充（`cuno bench`、サニタイザ、カバレッジ）
- 外部テストフレームワーク（GTest / Catch2 / doctest）
- `cuno install`

**完了条件**: GTest を使った既存のテスト資産を `cuno test` で実行できる。

## 将来の検討事項

- 自作フォーマッタ（clang-format の置き換え）
- コンパイラの自動インストール（`cuno toolchain install`）
- C++20 モジュール
- 公開パッケージサービス（依存管理 段階 3、`cuno package` / `publish`）
- `cuno doc`
- Ninja バックエンド
- safec 対応

## v1.0 について

1.0 以降は、`cuno.toml` の後方互換を保証します（[distribution.md](specs/distribution.md)）。

1.0 にするかどうかは、**実務での採用が報告された時点で検討します**。マイルストーンの完了だけでは 1.0 にしません。
