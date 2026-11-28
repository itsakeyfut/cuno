# cuno 自体の配布と運営

## ライセンス

- **MIT と Apache-2.0 のデュアルライセンス**（利用者がどちらかを選べる）。
- 理由: `cuno/test.h` などの同梱ヘッダは利用者のコードと一緒にコンパイルされるため、ゲーム会社などが製品開発に使っても問題にならない緩いライセンスである必要がある。Rust エコシステムの標準でもあり、Apache-2.0 の特許条項は企業にとっての安心材料になる。

## 配布形態

- **単一バイナリ**。`cuno/test.h` などの同梱ヘッダもバイナリに埋め込み、初回実行時に cuno の管理ディレクトリへ展開する。

## インストール方法

- GitHub Releases でバイナリを配布する。
  - リリースごとに `v<バージョン>` のタグを付け、`CHANGELOG.md` のそのバージョンの節をリリースノートにする。
  - 添付するのは `cuno-<バージョン>-x86_64-pc-windows-msvc.zip`（`cuno.exe`、`README.md`、`LICENSE-MIT`、`LICENSE-APACHE` を直下に置く）と、その SHA-256 を書いた `cuno-<バージョン>-x86_64-pc-windows-msvc.zip.sha256`。
  - バイナリにコード署名はしない。初めて実行するときに Windows の SmartScreen が警告することがある。
    - 理由: 証明書の取得と管理が要り、それがリリースの前提になる。winget などに対応するときにあらためて検討する。
  - 同じリリースで、全クレートを crates.io に公開する（`cargo install cuno` で入る）。
- Windows: PowerShell のワンライナー（[下記](#powershell-のワンライナー)）。winget / Scoop は後で対応する。
- Rust 利用者向け: `cargo install cuno`。

## 管理ディレクトリ

- 全 OS で `~/.cuno`（Windows は `%USERPROFILE%\.cuno`）。
- 環境変数 `CUNO_HOME` で変更できる（cargo の `CARGO_HOME` と同じ）。絶対パスで書く。相対パスや、Windows でドライブに依存するパス（`/x`、`C:x`）はエラー（終了コード 3）。
  - 理由: 相対パスを許すと、cuno を実行するディレクトリによって管理ディレクトリ（とユーザー単位の設定）が変わってしまう。
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

- 同梱ヘッダは、ビルドするコマンドがビルドの前に書き出す。中身が同じファイルは書き直さない。書き出せない場合はエラー（終了コード 3）。

### PowerShell のワンライナー

```
powershell -c "irm https://github.com/itsakeyfut/cuno/releases/latest/download/install.ps1 | iex"
```

- `install.ps1` はリポジトリの直下に置き、リリースごとに zip と一緒に添付する。URL は常に最新のリリースのものを指す。
  - 理由: スクリプトとバイナリが必ず同じリリースのものになる。`main` のスクリプトを配ると、リリース前の変更が公開済みのリリースと食い違いうる。
- 手順:
  1. 管理ディレクトリを決める。`CUNO_HOME` があればそれ（cuno と同じく、絶対パスでなければエラーで何もしない）、なければ `%USERPROFILE%\.cuno`。
  2. 最新の版を、`https://github.com/itsakeyfut/cuno/releases/latest` の転送先（`…/releases/tag/v<version>`）から知る。
  3. zip と `.sha256` を一時ディレクトリにダウンロードし、SHA-256 を照合する。
  4. zip から `cuno.exe` を取り出し、`<管理ディレクトリ>\bin\cuno.exe` に置く。すでにあれば置き換える。
  5. `<管理ディレクトリ>\bin` がユーザーの `PATH`（レジストリの `HKCU\Environment` の `Path`）になければ末尾に足し、変更をほかのプロセスに知らせる。値の種類（`REG_EXPAND_SZ`）と、`%USERPROFILE%` など展開前の書き方はそのまま残す。
  6. 入れた版と場所を表示し、新しく開いた端末で `cuno` が使えることを伝える。
- 失敗したら、一時ディレクトリを消し、置いてあった `cuno.exe` と `PATH` は変えずに、エラーを表示して止まる。照合が合わないときも、何も置かない。
- 保存した `install.ps1` を実行するときは、引数を渡せる（`irm … | iex` では渡せない）。
  - `-Version <x.y.z>`: 入れる版。手順 2 を飛ばす。
  - `-Releases <URL>`: リリースを取得する場所。既定は `https://github.com/itsakeyfut/cuno/releases`。zip は `<URL>/download/v<version>/` から取得する。社内のミラーや、インストーラのテストに使う。
- ダウンロードには cuno と同じく `curl.exe` を使い（[ダウンロード](#ダウンロード)）、zip は `tar.exe` で展開する。どちらも Windows に標準で入っているものを `%SystemRoot%\System32` から呼ぶ。
  - 理由: PowerShell 5.1 の `curl` は `Invoke-WebRequest` の別名で、`PATH` の先に Git for Windows のツールがあると、zip を読めない GNU tar が見つかる。

## ダウンロード

- cuno がネットワークから取得するとき（`cuno self update`、依存管理の取得）は、`curl` を起動する。Windows では、Windows 10 1803 以降に標準で入っている `curl.exe` が `PATH` で見つかる。
  - 理由: Windows の `curl.exe` は Windows の証明書ストアで TLS を検証し、`HTTPS_PROXY` などのプロキシの環境変数にも従う。会社のプロキシや TLS の検査の下でも、利用者が設定を足さずに動く。Rust の HTTP ライブラリを組み込むと、これらを cuno が自前で扱うことになり、バイナリも大きくなる。
- `curl` を起動できなければエラー（終了コード 3）で、`help:` に、Windows 10 1803 以降に入っていること、`PATH` で見つかるようにすることを書く。
- ダウンロードに失敗したら（HTTP のエラーを含む）エラー（終了コード 3）で、URL と `curl` のエラーを示す。

## 自己更新

- `cuno self update` で最新のリリースに更新する（[commands.md](commands.md#cuno-self-update)）。
- 入れ方は、実行中の `cuno.exe` の場所で見分ける（パスの同一性で比べる）。
  - `<管理ディレクトリ>\bin\cuno.exe`: cuno 自身のインストーラ（ワンライナー）で入れたもの。cuno が更新・削除する。
  - `<CARGO_HOME>\bin\cuno.exe`（`CARGO_HOME` がなければ `%USERPROFILE%\.cargo`）: `cargo install` で入れたもの。cargo で更新・削除するよう案内する。
  - それ以外（zip を手で展開したものなど）: cuno は自分を書き換えず、ワンライナーで入れ直すよう案内する。
  - 理由: インストーラが入れ方を記録すると、記録と実際の場所が食い違ったとき（別の場所の cuno から実行したときなど）に、違う `cuno.exe` を書き換えうる。場所で見分ければ、食い違う記録がない。
- アンインストールは `cuno self uninstall`（[commands.md](commands.md#cuno-self-uninstall)）。

## cuno 自体の対応環境

Windows x64 → Linux x64 → macOS（arm64）の順に対応する。

## バージョン方針

- cuno 自体はセマンティックバージョニング。
- リリースするバージョンは `x.y.z` の形だけ。プレリリース（`0.2.0-rc.1` など）やビルドメタデータ付きの版は出さない。`0.0.0` は「まだリリースしていない」を表し、リリースしない。
  - 理由: プレリリースの並び順（`0.2.0-rc.1` は `0.2.0` より前）は、バージョンを比べる道具によって扱いが割れる。扱わない形に限れば、リリースの判定を誤らない。
- 1.0 以降は `Cuno.toml` の後方互換を保証する（古い `Cuno.toml` が新しい cuno でそのまま動く）。
- 1.0 にするかどうかは、実務での採用が報告された時点で検討する。マイルストーンの完了だけでは 1.0 にしない（[roadmap.md](../roadmap.md)）。

## 貢献の受け入れ

- GitHub の Issue / PR で受け付ける。
- CONTRIBUTING.md に、コンパイラ追加手順（[toolchain.md](toolchain.md)）を含める。

## 未決

- 行動規範（Code of Conduct）、DCO / CLA の要否
