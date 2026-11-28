# ゲーム開発向けの設定

cuno の既定は、どの C/C++ のプロジェクトでもそのまま動くように選んでいます。ゲーム開発では、例外や RTTI を切る、C ランタイムを実行ファイルに取り込むなど、既定とは違う設定を選ぶことがよくあります。このページは、そうした設定を `Cuno.toml` にどう書くかと、選んだときに何ができなくなるかをまとめます。

ここに書く設定はどれも、`Cuno.toml` の仕様（[manifest.md](../specs/manifest.md)）にある項目です。このページは新しいことを決めず、選び方だけを説明します。

## まとめ

ゲームでよく選ぶ設定を、まとめて書くと次のようになります。

```toml
[build]
exceptions = false    # C++ の例外を使わない
rtti = false          # dynamic_cast / typeid を使わない
runtime = "static"    # C ランタイムを実行ファイルに取り込む

[profile.release]
lto = true            # リリースでリンク時最適化を使う

[profile.release.build]
warnings-as-errors = true   # リリースのビルドでは警告をエラーにする
```

`cuno new` が作るパッケージは、この設定のまま MSVC、clang-cl、clang でビルド・実行・テストできます。cuno のテスト用ヘッダ（`cuno/test.h`）も、例外と RTTI を切った設定で動きます。

## 例外を切る（`exceptions = false`）

```toml
[build]
exceptions = false
```

- **何が変わるか**: 各コンパイラに例外を切るフラグが渡ります（MSVC は `/D_HAS_EXCEPTIONS=0`、GCC と Clang は `-fno-exceptions` など。対応は [build.md](../specs/build.md#設定とフラグの対応)）。
- **できなくなること**:
  - `try` / `catch` / `throw` を書いたコードは、コンパイルエラーになります。cl は本来警告だけ出しますが、cuno はその警告をエラーに格上げして、ビルドの段階で止めます（`try` のない `throw` だけは、cl では検出できません）。
  - 例外で失敗を知らせる標準ライブラリの機能（メモリ確保の失敗、`std::vector::at` の範囲外など）は、例外を投げる代わりにプログラムを止めます。
  - 例外を使うライブラリのソースは、そのままではビルドできません。
- **代わりのエラー処理**: 戻り値で失敗を返します。値がないことだけを返すなら `std::optional`（C++17）、失敗の理由も返すなら `std::expected`（C++23）が使えます。

## RTTI を切る（`rtti = false`）

```toml
[build]
rtti = false
```

- **何が変わるか**: 各コンパイラに RTTI を切るフラグが渡ります（MSVC は `/GR-`、GCC と Clang は `-fno-rtti`。対応は [build.md](../specs/build.md#設定とフラグの対応)）。
- **できなくなること**: `dynamic_cast` と `typeid` を書いたコードは、コンパイルエラーになります（cl と clang-cl では、cuno が警告をエラーに格上げします）。型ごとの処理は、仮想関数や、型を表す値を自分で持つことで書きます。

## C ランタイムを取り込む（`runtime = "static"`）

```toml
[build]
runtime = "static"
```

- **何が変わるか**: C ランタイム（MSVC の CRT、MinGW の libgcc / libstdc++ など）を実行ファイルに取り込みます。配布するときに、ランタイムの DLL や再頒布パッケージを同梱する必要がなくなります。
- **気をつけること**: 実行ファイルが大きくなります。DLL をまたいでメモリを確保・解放するなど、ランタイムを共有することを前提にしたコードは、DLL ごとに別のランタイムを持つことになるので注意が必要です。既定の `dynamic` との違いは [manifest.md](../specs/manifest.md#build) を参照してください。

## リリースでリンク時最適化を使う（`lto = true`）

```toml
[profile.release]
lto = true
```

- **何が変わるか**: リリースのビルド（`cuno build --release`）で、リンクの段階で翻訳単位をまたいで最適化します。
- **気をつけること**: リンクに時間がかかります。開発中のビルド（`dev`）には付けず、リリースだけに書くのがふつうです。

## 警告をエラーにする（`warnings-as-errors = true`）

```toml
[profile.release.build]
warnings-as-errors = true
```

- **何が変わるか**: 警告が 1 つでもあればビルドを失敗にします（MSVC は `/WX`、GCC と Clang は `-Werror`）。
- **どこに書くか**: すべてのビルドに付けると、書きかけのコードでもビルドが止まります。上のようにリリースだけに付けるか、CI のマシンだけに `.cuno/config.toml` で付けると、手元の作業を止めずに済みます（[manifest.md](../specs/manifest.md#cunoconfigtoml)）。

## 足りないとき（`[build.<compiler>]`）

抽象化された設定にないフラグは、コンパイラごとに生のフラグとして書けます。

```toml
[build.msvc]
flags = ["/GS-"]
```

ここに書いたフラグは cuno が意味を理解しないまま渡すので、例外や RTTI のように上の項目がある設定は、項目の方で書いてください（[manifest.md](../specs/manifest.md#buildcompiler)）。
