# cuno 仕様書

cuno の決定済み仕様をまとめたドキュメント群。設計・実装はこのディレクトリの内容を正とする。各ドキュメントは単独で読めるように書き、採用理由もドキュメント内に記す。

## 言語と正本

- **正本はこの日本語版（`docs/ja/specs/`）**。実装はこちらに従う。
- 英語版（`docs/en/specs/`）は公開用の翻訳。内容が食い違った場合は日本語版を正とする。
- 日本語版を変更したら、**同じコミットで英語版も更新する**。
- 英語版の各ファイルの先頭には、翻訳元と、翻訳元の日本語ファイルの内容ハッシュを記録する:
  ```
  <!-- translated-from: docs/ja/specs/overview.md @ blob <日本語ファイルの git hash-object> -->
  ```
  コミットハッシュではなく内容ハッシュを使うのは、コミット前に値が決まり、日本語版と英語版を同じコミットに含められるため。この記録と、日本語ファイルの現在の `git hash-object` が異なれば、英語版は古い。

## 目次

| ドキュメント | 内容 |
|---|---|
| [overview.md](overview.md) | 目的、利用者、ゴール / 非ゴール / アンチゴール、MVP の範囲 |
| [commands.md](commands.md) | コマンド体系、オプション、出力、終了コード |
| [project-layout.md](project-layout.md) | プロジェクトの種類と構成 |
| [manifest.md](manifest.md) | 設定ファイル（`cuno.toml` / `.cuno/config.toml`） |
| [toolchain.md](toolchain.md) | 対象コンパイラ、言語規格、コンパイラ抽象化 |
| [build.md](build.md) | ビルドエンジン |
| [dependencies.md](dependencies.md) | 依存ライブラリ管理 |
| [testing.md](testing.md) | テスト |
| [dev-tools.md](dev-tools.md) | 開発ツール（fmt / lint / doc / IDE 連携） |
| [distribution.md](distribution.md) | cuno 自体の配布と運営、ライセンス、管理ディレクトリ |
| [architecture.md](architecture.md) | cuno 本体の内部構成（クレート構成、リポジトリ構成） |

仕様ではないが、ほかのツール（CMake、Visual Studio、clang-format など）から移る人向けの対応表は [移行ガイド](../guides/migration.md) にある。

## 記法

### 未決事項

各ドキュメントの「未決」節にある項目は、まだ仕様が決まっていない。扱いは次のとおり。

- MVP に関わる項目: 実装時の設計段階で決め、決めた内容をこのディレクトリの該当ドキュメントに書き戻す。
- MVP 後の段階に関わる項目: その段階の実装に入る前に決め、同様に書き戻す。
- 「初期案」と書かれた内容（設定値とコンパイラのフラグの対応など）は、実装時に検証して確定させ、書き戻す。

### 時期の表記

| 表記 | 意味 |
|---|---|
| MVP | 最初のバージョンに含める（範囲は [overview.md](overview.md)） |
| MVP 直後 | MVP の次に最優先で着手する（GCC 対応、`cuno init`、`cuno toolchain list` / `default`、`cuno self update`） |
| 依存管理 段階 1〜3 | 依存ライブラリ管理の段階（[dependencies.md](dependencies.md)）。段階 1 は MVP 直後の次に着手する |
| 後 | MVP 直後より後。順序は着手時に決める |
| 将来 | 現時点では着手の予定がない。必要になった時点で検討する |
