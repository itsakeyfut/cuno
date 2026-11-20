# cuno 自体の配布と運営

## ライセンス

- **MIT と Apache-2.0 のデュアルライセンス**（利用者がどちらかを選べる）。
- 理由: `cuno/test.h` などの同梱ヘッダは利用者のコードと一緒にコンパイルされるため、ゲーム会社などが製品開発に使っても問題にならない緩いライセンスである必要がある。Rust エコシステムの標準でもあり、Apache-2.0 の特許条項は企業にとっての安心材料になる。

## 配布形態

- **単一バイナリ**。`cuno/test.h` などの同梱ヘッダもバイナリに埋め込み、初回実行時に cuno の管理ディレクトリへ展開する。

## インストール方法

- GitHub Releases でバイナリを配布する。
- Windows: winget / Scoop / PowerShell のワンライナー。
- Rust 利用者向け: `cargo install cuno`。

## 管理ディレクトリ

- 全 OS で `~/.cuno`（Windows は `%USERPROFILE%\.cuno`）。
- 環境変数 `CUNO_HOME` で変更できる（cargo の `CARGO_HOME` と同じ）。
- 理由: 場所が 1 か所で説明しやすく、消すのも簡単。cargo / rustup と同じ形。OS 流の場所（Windows の `%LOCALAPPDATA%`、Linux の XDG）は作法に沿うが、OS ごとに場所が変わり分散するため採用しない。

| 中身 | 内容 |
|---|---|
| `config.toml` | ユーザー単位の設定（[manifest.md](manifest.md)） |
| `bin/` | cuno 本体（PowerShell のワンライナー等、cuno 自身のインストーラで入れた場合。winget / Scoop / `cargo install` で入れた場合はそれぞれの場所に置かれ、`cuno self update` もそれぞれの更新手段に案内する） |
| `include/<cuno のバージョン>/` | 同梱ヘッダの展開先。cuno のバージョンごとに分け、cuno を更新しても既存プロジェクトのビルドが壊れないようにする |
| `registry/` | レシピインデックス |
| `sources/` | 依存のソース |
| `builds/` | ビルド済み依存のキャッシュ |
| `toolchains/` | 自動インストールしたコンパイラ |

## 自己更新

- `cuno self update`（rustup と同じ方式）。

## cuno 自体の対応環境

Windows x64 → Linux x64 → macOS（arm64）の順に対応する。

## バージョン方針

- cuno 自体はセマンティックバージョニング。
- 1.0 以降は `cuno.toml` の後方互換を保証する（古い `cuno.toml` が新しい cuno でそのまま動く）。

## 貢献の受け入れ

- GitHub の Issue / PR で受け付ける。
- CONTRIBUTING.md に、コンパイラ追加手順（[toolchain.md](toolchain.md)）を含める。

## 未決

- 1.0 の基準
- 行動規範（Code of Conduct）、DCO / CLA の要否
