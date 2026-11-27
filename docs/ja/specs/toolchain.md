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
- GCC は MVP 直後に追加する。Windows の MinGW-w64（ビルド対象は `x86_64-w64-mingw32`）から対応する。MinGW は MSVC のツールセットを使わず、自分のランタイムで動くため、Visual Studio がなくても使える。
  - 理由: cuno 自体は Windows のまま、コンパイラだけを増やせる。Linux の GCC は、cuno 自体を Linux で動かす M8 で扱う。

## 言語規格

| | 最低サポート | `cuno new` の既定 |
|---|---|---|
| C++ | C++17 | C++20 |
| C | C11 | C17 |

- `edition` で指定した規格を、使用するコンパイラが受け付けない場合（例: cl 19.52 未満で `c++23`、cl 19.37 未満で `c23`、GCC 11 未満で `c++23`）はエラーにし、必要なコンパイラのバージョンを案内する。cl は知らない `/std` を警告だけ出して無視するため、受け付けるかどうかはコンパイラのバージョンで判定する。
- `edition` と各コンパイラの規格フラグの対応は [build.md](build.md#設定とフラグの対応) にある。

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
  - **確認の方法**: `<yvals_core.h>` だけを含むファイルを、ビルドのコンパイルコマンドと同じ条件（同じ Clang、解決した Windows の環境、ターゲット `x86_64-pc-windows-msvc`、そしてビルドのコンパイルフラグのうち入力と出力の指定を除くすべて。言語規格、RTTI や例外の設定、インクルードパス、`defines`、利用者の `flags` を含む）で、**C++ として**構文チェックだけ行う（`clang-cl` は `/TP /Zs`、`clang` は `-x c++ -fsyntax-only`）。ファイルはプロジェクトの外（一時ディレクトリ）に置き、コンパイラはビルドと同じくプロジェクトのルートで実行する（プロジェクトの外ではカレントディレクトリ）。`flags` の相対パスがビルドと同じ意味になるように。
    - C++ として行う理由: C として扱うと標準ライブラリが別のエラー（STL1003）で止まるため。
    - ビルドのフラグを渡す理由: MSVC STL の判定はフラグとマクロに左右される。`_ALLOW_COMPILER_AND_STL_VERSION_MISMATCH` が定義されていれば STL1000 を出さず（利用者が意図的に許した組み合わせは、実際のビルドと同じく通す）、`rtti = false` で `_HAS_STATIC_RTTI=0` を定義したプロジェクトは `/GR-` なしでは別のエラーで止まる。一部のフラグだけを渡すと、実際のビルドとは違う判定になる。
  - **判定**: 終了コードが 0 でなく、標準出力・標準エラーのどちらかに `STL1000:` があれば、その組み合わせは使えない。必要な Clang の版は、標準ライブラリのメッセージ `expected Clang <バージョン> or newer` の `<バージョン>` から取り、そのままの形で示す（`22` のことも `17.0.0` のこともある）。取り出せなければ、標準ライブラリのメッセージをそのまま `help:` に出す。
  - **判定できない場合**: STL1000 以外の理由で失敗したとき（ヘッダが見つからない、他の STL エラー等）、または 30 秒以内に終わらないときは、組み合わせの可否を判定できない。通ったとは扱わず、環境の問題としてコンパイラの出力を示す（終了コードは下と同じ）。
  - 理由: ヘッダの条件式を読んだり、MSVC の版ごとの対応表を cuno が持ったりすると、新しい Visual Studio で書き方や版が変わったときに、使えない組み合わせを「問題なし」と判定しうる。試しのコンパイルは数十ミリ秒で、標準ライブラリの答えそのものを使える。
- **clang-tidy にも同じ確認を適用する。** clang-tidy は compile_commands.json のコマンドで解析し、MSVC の標準ヘッダを読むので、古い clang-tidy は STL1000 で全ファイルの解析に失敗する。
  - 方法: 同じファイルを、clang-tidy に `--checks=-*,readability-braces-around-statements`（チェックが 1 つもないと解析せずに止まるため、影響のない 1 つだけを有効にする）と、`--` に続けて、compile_commands.json がそのファイルに書くはずのコマンドを渡して解析させる。つまり `dev` プロファイルで `CUNO_TEST` を定義したコンパイルの条件に、MSVC / Windows SDK のインクルードパスと MSVC のバージョン（`-fms-compatibility-version`）を加え、clang 系のツールが読む形に書き換えたもの（[build.md](build.md#出力)）。プロジェクトの `.clang-tidy` は読ませない。STL1000 は clang-tidy では標準出力に出る。判定は上と同じ。
    - compile_commands.json と違い、`--` の後のコマンドにはコンパイラのプログラム名がない。そのため、cl 形式のコマンド（MSVC、clang-cl）は `--driver-mode=cl` を付けて clang-cl のものとして読ませ、MSVC を選んだときはターゲット `x86_64-pc-windows-msvc` も渡す（cl の条件はターゲットを名指ししないため）。
    - 理由: `cuno lint` は compile_commands.json のコマンドで clang-tidy を動かす。別の条件（たとえばビルドのプロファイル、`CUNO_TEST` なし、インクルードパスを環境変数の `INCLUDE` だけで渡すこと）で確かめると、`cuno doctor` が問題なしとした clang-tidy が、`cuno lint` では別のコマンドで解析して失敗しうる。
- **どこで確認するか**:

  | コマンド | 確認するもの | C だけのプロジェクト（`[package] lang = "c"`） |
  |---|---|---|
  | ビルド系コマンド | 選ばれた Clang | 行わない（C++ の標準ライブラリを使わないため） |
  | `cuno lint` | clang-tidy | 行わない |
  | `cuno doctor`（プロジェクトの中・外） | 選ばれた Clang と clang-tidy | 行わない |

  - GCC を選んだときは、どれも行わない。GCC は MSVC の標準ライブラリを使わないため。

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
- コンパイラの種類は 1 つの列挙（`CompilerKind`）で表し、種類ごとの答えは、すべての種類を名指しする `match` に書く。新しいコンパイラは、種類を 1 つ足すと、答えるべき箇所がコンパイルエラーとして挙がる。他の開発者による追加を想定し、手順を下に書く。
- `cuno.toml` の `[build]` 設定（例: `warnings-as-errors = true`）を各系統のフラグ（`-Werror` / `/WX`）に翻訳する。

### コンパイラを追加する手順

GCC（MinGW-w64）を追加したときに要ったものの一覧。プルリクエストは、1 と 2、3 から 6、7 から 9 の 3 つ（と、その後の修正）に分けた。種類を足すと 3 から 6 の `match` がまとめてコンパイルエラーになり、実際のコンパイラで動かすテストは、CI がそのコンパイラを入れていなければ CI で失敗するため、それぞれの中では分けられない。

1. **仕様を先に書く。** この文書に、探す場所と順、版の読み方、選ぶ条件、最低バージョンを書く。[build.md](build.md#設定とフラグの対応) の表すべてに列を足し、どのフラグも実際のコンパイラで確かめる。表が答えない設定（ランタイムの選び方、`check` の方式、LTO）は、理由とともに書く。
2. **検出**（`cuno-toolchain`）。そのコンパイラの検出のモジュール（GCC は `gcc.rs`）を作る。
   - 探す順: `[toolchain.paths]` の設定、`PATH`、配布物の標準の場所。同じファイルは 1 つにまとめる。
   - 版、プレリリースか、ビルド対象、ランタイムなど、選ぶ条件に要るものを、言語の設定によらない出力から読む（GCC は `-dumpfullversion`、`-dumpmachine`）。ビルドと同じ環境（GCC は自分のディレクトリを `PATH` に加え、`TOOL_OPTION_VARIABLES` を取り除く）と、プロジェクトのファイルを読まない作業ディレクトリで起動する。
   - 使うときだけ探す。そのコンパイラを選んだとき（`cuno-build` の `toolchain::ready`）と `cuno doctor` で呼び、ほかのコンパイラのビルドや `fmt`・`lint` では起動しない。
   - `[toolchain.paths]` の設定は、`cuno-manifest` の `Settings` の読み出し（GCC は `Settings::gcc_path`）で読む。`config.rs` の `Tool` にそのツールがあることを確かめ、設定の表のテストに足す。
3. **種類**（`cuno-toolchain` の `compiler.rs`）。`CompilerKind` に種類を、`CompilerKind::ALL` に同じ種類を足し、コンパイルエラーになった `match` をすべて答える。新しい系統（`Family`）、書き方（`Syntax`）、ヘッダ依存の取り方（`Dependencies`）が要るなら足す。主な答え:
   - 名前、最低バージョン、MSVC のツールセットと標準ライブラリを使うか、`check` をコンパイルで行うか、リンクに使うドライバ（GCC の C++ は `g++`）。
   - `cuno-manifest` の `CompilerName` との対応と、`[toolchain.version]` の置き場所（`Constraints::slot`）。
   - `CompilerKind::parse` が「まだ対応していない」とする名前から外す。
4. **選択**（`select.rs`）。`Found` に検出の結果を足し、`copy_of` で選ぶ条件を書く。`[toolchain.paths]` に書いたものはそれだけを使い、使えなければ設定を名指ししたエラーにする。見つからないとき、使えるものがないときの `help:` に、入れ方を書く（`error.rs`）。MSVC のツールセットを使わないコンパイラなら、Visual Studio が見つからなくてもビルドが止まらないこと（`toolchain::ready`）と、`cuno doctor` がそれを問題にしないことを確かめる。
5. **環境**（`prepare.rs`）。起動に要る環境を作る。そのコンパイラが自分で読む環境変数は `TOOL_OPTION_VARIABLES` に足して取り除く。コマンドの識別（変更の判定）に要るもの、つまり標準ヘッダのディレクトリ（`Prepared::include_dirs`）と、全プログラムにリンクするランタイムのファイルを、`Prepared` に持たせる。[build.md](build.md#変更の判定) の、同じ名前のファイルの出現を見逃さないため。
6. **フラグ**（`flags/`）。`compile.rs`、`link.rs` の各行と、`library_files`（リンカが探すライブラリのファイル名を、探す順に）、色の指定、ヘッダ依存のファイル、規格を受け付ける版（`check_edition`）。clang 系のツールに渡すコマンド（`clang_tools.rs`）には、ビルド対象と、ビルドが見る版のマクロ（GCC は `-fgnuc-version`。どの版をどのマクロで伝えるかは、`ToolContext::of` の、系統ごとの `match` に答える）を書く。MSVC の標準ライブラリを使わないコンパイラでは、clang-tidy の標準ライブラリの確認（`toolchain::tidy_compile`）を行わない。

7. **テストのランタイム**（`cuno-build` の `headers/cuno/`）。テストの登録がそのコンパイラで動くか（`constructor` 属性か、MSVC の `.CRT$XCU` か）、通常ビルドがランタイムなしでリンクできるか、`long double` などの表示、そのコンパイラが知らない `#pragma` を確かめる。
8. **テスト。**
   - 各モジュールの単体テストの表に、そのコンパイラの行を足す。
   - `tests/support/compilers.rs` の `Compiler` に足し、探す場所を `CUNO_TEST_*` で変えられるようにする。
   - 実際のコンパイラで動かすテスト（`tests/compile_flags.rs`、`tests/examples.rs`、`tests/test_headers.rs`、`tests/lint.rs`）の対象に足し、そのコンパイラだけのテスト（GCC は `tests/gcc.rs`）で、`cuno new` から `lint` までを通す。
   - 例の `examples/cpp/per-compiler-flags` に、そのコンパイラの `[build.<compiler>]` を足す。
   - ASCII でないパスで試す。プロジェクトのディレクトリ、ソース、実行ファイルの名前、一時ディレクトリを ASCII 以外の文字で名付け、そのコンパイラが起動するプログラム（コンパイラの本体、アセンブラ、リンカなど）がそれを開けるか、開けないものを cuno がビルドの前に拒むかを、そのコンパイラだけのテストで確かめる（GCC は `tests/gcc.rs` の `paths_that_are_not_ascii_build_or_are_refused`）。ANSI コードページがその文字を持つ Windows（日本語版の CP932）と、持たない Windows（英語版の CP1252。CI の Windows はこちら）の両方で通す。
     - 理由: GCC では、プロジェクトのディレクトリ名による失敗はレビューで、一時ディレクトリによる失敗は CI で初めて見つかった。後者は CP932 の手元では通り、CP1252 の CI で失敗した（[MinGW（GCC）](#mingwgcc) の「ASCII でないパス」）。
9. **CI。** `.github/workflows/ci.yml` の test ジョブで、そのコンパイラを、利用者が入れる場所に入れる。見つからなければテストは失敗する。
10. **文書。** [commands.md](commands.md) と [manifest.md](manifest.md) の、コンパイラの名前の一覧と「まだ対応していない」の記述を直す。

## Windows での環境解決

- 利用者が Developer Command Prompt を開かなくても MSVC を使えること。
- Visual Studio（または Build Tools）の場所と、必要な環境（環境変数等）は cuno が自動で解決する。
- **Clang（`clang` / `clang-cl`）にも同じ解決を適用する**。Windows の Clang も MSVC のヘッダ・ライブラリ・Windows SDK を使うため。
- Visual Studio / Build Tools が見つからない場合は、Clang を選んでいてもエラーにし、`cuno doctor` と同じ案内を出す。

### 解決の方法

- **環境は、ツールセットと Windows SDK のディレクトリから cuno が組み立てる。** `vcvars64.bat` は実行しない。
  - 理由: `vcvars64.bat` の実行は 1 回あたり約 1.75 秒かかり、その出力はコンソールのコードページ（日本語版 Windows では CP932）で、正しく復号しないと日本語のパスで黙って壊れる。組み立ては 10 ミリ秒程度で、文字コードの問題がない。ディレクトリの構成は Visual Studio 2017 から変わっておらず、cuno のテストが `vcvars64.bat` の結果と照合して、構成の変化を CI で検出する。
- Windows SDK の場所: レジストリの `HKLM\SOFTWARE\Microsoft\Windows Kits\Installed Roots` の `KitsRoot10`、次に `%ProgramFiles(x86)%\Windows Kits\10` の順に探し、使える版がある最初の場所を使う（レジストリの値が古く、そこに SDK が残っていない場合があるため）。絶対パスでない場所は探さない。
- Windows SDK の版: `Include\<バージョン>` のうち、`um\windows.h`、`ucrt\stdio.h`、`Lib\<バージョン>\um\x64`、`Lib\<バージョン>\ucrt\x64` がすべてあるものの最新（版は数値として比べ、数値でない名前のディレクトリは版として扱わない）。一部だけ入った版を選ぶと、後でヘッダやライブラリが見つからないエラーになるため。
- 組み立てる値（`<MSVC>` は MSVC のツールセットのディレクトリ、`<SDK>` は SDK の場所、`<バージョン>` は SDK の版）:

  | 変数 | 値 |
  |---|---|
  | `INCLUDE` | `<MSVC>\include`、Visual Studio の `VC\Auxiliary\VS\include`（あれば）、`<SDK>\Include\<バージョン>\` の `ucrt` / `um` / `shared` / `winrt` / `cppwinrt` |
  | `LIB` | `<MSVC>\lib\x64`、`<SDK>\Lib\<バージョン>\ucrt\x64`、`<SDK>\Lib\<バージョン>\um\x64` |
  | `LIBPATH` | `<MSVC>\lib\x64` |
  | `PATH`（先頭に足す） | `<MSVC>\bin\Hostx64\x64`、`<SDK>\bin\<バージョン>\x64` |

- **コンパイラやリンカが自分で読む環境変数は渡さない。** `CL` / `_CL_`（cl.exe と clang-cl のオプション）、`LINK` / `_LINK_`（link.exe と lld-link のオプション）、`CPATH` / `C_INCLUDE_PATH` / `CPLUS_INCLUDE_PATH`（Clang と GCC のインクルードパス）、`LIBRARY_PATH` / `GCC_EXEC_PREFIX` / `COMPILER_PATH`（GCC がライブラリと自分のプログラムを探す場所）、`DEPENDENCIES_OUTPUT` / `SUNPRO_DEPENDENCIES`（GCC が書くもう 1 つのヘッダ依存のファイル）は、ビルド、標準ライブラリの確認、`cuno doctor`、GCC の検出の環境から取り除く。ビルド系コマンドは、設定されていたものを 1 つずつ警告する（`warning: CL is set in the environment and is not used`、`help: write the flags in cuno.toml`）。
  - 理由: これらはコマンドラインに現れずにフラグを足すため、`cuno.toml` と違うフラグで黙ってビルドし、成功と報告することになる（例: `_CL_=-DNDEBUG` で dev プロファイルにも `NDEBUG` が定義される）。`-v` の表示にも出ない。エラーにしないのは、全体に `CL=/MP` などを設定している利用者が、cuno を使うたびに消さずに済むように。

- **Developer Command Prompt の変数は取り除く。** `VSINSTALLDIR`、`VCINSTALLDIR`、`VCToolsInstallDir`、`EXTERNAL_INCLUDE`、`WindowsSdkDir`、`WindowsSDKVersion`、`UniversalCRTSdkDir`、`UCRTVersion`。clang / clang-cl はこれらからツールセットとリンカーを探すため、別の Visual Studio の Developer Command Prompt から cuno を実行すると、cuno が選んだものと違うツールセットで成功してしまう。
- `vcvars64.bat` が設定するもののうち、次は設定しない（使えばエラーで分かる）。
  - .NET Framework SDK（`NETFXSDK`）のヘッダとライブラリ（`metahost.h`、`mscoree.lib` など）。
  - C++/CLI（`/clr`）と C++/CX（`/ZW`）のための `LIBPATH` の項目。
- Windows SDK が見つからない場合はエラー（終了コード 3）にし、Build Tools の Windows SDK の入れ方を案内する。

### Windows の Clang は MSVC 互換で動かす

- `clang` / `clang-cl` とも、ビルド対象は `x86_64-pc-windows-msvc`（MSVC と同じ ABI）。MSVC でビルドしたライブラリと混ぜてリンクできる。
- MinGW 系の Clang は対象外。

### リンカ

| コンパイラ | リンカ | 理由 |
|---|---|---|
| `msvc` | `link.exe` | MSVC 標準 |
| `clang` / `clang-cl` | `lld-link`（LLVM 同梱） | `link.exe` より高速 |

## 検出

- PATH、Visual Studio のインストール先（vswhere）、LLVM の標準インストール先、環境変数から探す。
- `cuno toolchain list` で検出したコンパイラを一覧表示する。
- **検出と確認のために cuno が実行するツールは、30 秒以内に終わらなければ打ち切る**（vswhere、cl.exe のバージョンの読み取り、各ツールの `--version`、標準ライブラリの試しのコンパイル）。打ち切ったツールは止め、実行に失敗したものとして扱う（成功とは扱わない）。
  - 理由: ウイルス対策のスキャンや壊れたインストールでツールが固まると、cuno ごと止まるため。普段は 1 秒もかからないので、30 秒は初回起動が遅い環境でも正常なツールを誤って打ち切らない長さとして、試しのコンパイルと同じ値に揃える。

### Visual Studio / Build Tools

- **vswhere だけで探す。** `%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe` を `-all -products * -prerelease -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -utf8 -format json` で実行し、C++ の x64 ツールを含むインストールをすべて得る。
  - 理由: vswhere は Visual Studio 2017 以降と Build Tools に必ず付き、インストール先を正しく知っている唯一の方法。`-utf8 -format json` で、コンソールのコードページ（日本語版 Windows では CP932）に左右されずに読める。
- 各インストールの MSVC ツールセットは、`VC\Tools\MSVC\<バージョン>` のうち `bin\Hostx64\x64\cl.exe` があるもの。既定のツールセットは `VC\Auxiliary\Build\Microsoft.VCToolsVersion.default.txt` に書かれた版。
- **cl.exe のバージョンは、cl.exe を引数なしで実行し、その表示の最初の `<数>.<数>.<数>` から読む。** 表示の言語（日本語版の Visual Studio を含む）に関係なく、数字の部分は同じだから。ツールセットのフォルダ名（例: `14.52.37025`）は cl.exe のバージョン（例: `19.52.37029`）と一致しないので使わない。
  - このとき環境変数 `CL` / `_CL_` は渡さない。cl.exe はこれらからオプションを読み、利用者が `CL=/nologo` を設定していると表示が出なくなるため。
  - バージョンを読めない cl.exe のツールセットは使えないものとして除き、ほかのツールセットの検出は続ける。使えるツールセットが 1 つもなければエラーにする。
- プレビュー版と、不完全なインストール（vswhere が complete / launchable と報告しないもの。セットアップのエラーや再起動待ち）も検出はする（`cuno doctor` で示せるように）。選ぶかどうかは「選択」が決め、どちらも選ばない。vswhere がこれらの情報を返さない場合は、プレビュー・不完全として扱う。
- C++ の x64 ツールを含むインストールが 1 つもない、または vswhere がない場合はエラー（終了コード 3）にし、Build Tools の「C++ によるデスクトップ開発」ワークロードの入れ方を案内する。

### LLVM（clang / clang-cl / clang-format / clang-tidy）

- 4 つのツールそれぞれについて、次の順に探し、見つかったものをすべて記録する。
  1. `[toolchain.paths]` に書いたパス（[manifest.md](manifest.md)）
  2. `PATH` の各ディレクトリ（絶対パスのものだけ。空や相対の項目は cuno の作業ディレクトリ（多くはプロジェクト）を指してしまい、そこにツールと同じ名前のファイルがあると使われるため、探さない）
  3. Windows では `%ProgramFiles%\LLVM\bin`（LLVM の公式インストーラーの既定の場所）
  4. Windows では、検出した各 Visual Studio の `VC\Tools\Llvm\x64\bin`（Visual Studio に同梱された Clang）
- 同じファイルを複数の場所から見つけた場合は 1 つとして扱う。
- **バージョンは、ツールを `--version` 付きで実行した表示から読む。** `version` を含む行の、`version` の後の最初の `<数>.<数>.<数>`（`clang version 22.1.8`、clang-tidy は `LLVM version 22.1.8`）。後の行の数字（インストール先のパスなど）は読まない。
  - 数字の直後に続く文字がある場合（`19.1.0-rc1`、`23.0.0git`）はプレリリースとして記録する。
  - `Target:` の行があれば、その既定のターゲット（`x86_64-pc-windows-msvc` など）を記録する。
  - 起動できない、またはエラーで終了したツールは「実行に失敗した」と記録し、バージョンを読まない。バージョンの行がないものは「バージョンなし」と記録する。いずれも一覧には残す（`cuno doctor` で示せるように）。
- 複数見つかった場合にどれを使うかは「選択」が決める。選ぶのは、実行でき、プレリリースでなく、Windows では既定のターゲットが MSVC（`*-windows-msvc`）のもののうち、バージョンの条件を満たす最新のもの（MinGW 向けの Clang は MSVC と混ぜてリンクできないため。並びはこの探索順を保つ）。
  - clang-format と clang-tidy は既定のターゲットを問わない（clang-tidy は表示しない）。
- **`[toolchain.paths]` にパスを書いたツールは、そのファイルだけを使う。** ほかに見つかったものが新しくても使わない。
  - 書いたパスにファイルがない場合、または書いたツールが上の条件を満たさない（実行できない、バージョンを表示しない、プレリリース、MSVC 向けでない Clang、バージョンの条件を満たさない）場合はエラー（終了コード 3、`cuno doctor` では問題）にし、その設定と理由を示す。
  - 書いたファイルが、キーとは別の LLVM のツールである場合も同じくエラーにする（例: `clang` に clang-cl のパス）。見分け方は次のとおり。
    - clang-format は `--version` の出力が `clang-format version` を含み、clang と clang-cl は `clang version` を含む。どちらも含まず、`LLVM (http://llvm.org` か `LLVM version`（配布元の名前が前に付いた `Ubuntu LLVM version` など）を含むものは clang-tidy。`--version` は 1 回だけ実行し、その出力でツールの種類とバージョンの両方を読む。
    - clang と clang-cl は、中身が同じ実行ファイルで、ファイル名によって振る舞いが決まるため、`--version` では見分けられない。書いたパスを `/?` を付けて実行し、成功して `OVERVIEW:` で始まる出力を返せば clang-cl、そうでなければ clang とみなす。
    - 理由: clang の代わりに clang-cl を（またはその逆を）使うと、もう一方の形式のフラグを渡すことになり、設定を指さないエラーで失敗するか、別の意味のフラグのままコンパイルして成功と報告する。名前ではなく実際の振る舞いで見分けるのは、別の名前のラッパー（`ccache` など）を誤って弾かないため。検出で見つけたツールは、名前で探しているため確かめない。
  - 理由: 書いたものの代わりに黙って別のツールを使うと、ユーザーが指定したのとは違うツールでビルドや lint をして成功と報告することになる。
- ツールが見つからないこと自体は、検出ではエラーにしない。使うときに必要かどうかを決めるのは各コマンド（`cuno doctor`、`cuno fmt` / `cuno lint`、ビルド系コマンド）。

### MinGW（GCC）

- GCC（`gcc` と `g++`）を、次の順に探し、見つかったものをすべて記録する。
  1. `[toolchain.paths] gcc` に書いたパス（[manifest.md](manifest.md)）
  2. `PATH` の各ディレクトリ（LLVM と同じく、絶対パスのものだけ）
  3. Windows では MSYS2 の標準の場所（`C:\msys64\ucrt64\bin`、`C:\msys64\mingw64\bin` の順）
  - 理由: MSYS2 を入れただけで、`PATH` を設定しなくても使えるようにする。LLVM の公式インストーラーの場所を探すのと同じ考え方。
- 同じファイルを複数の場所から見つけた場合は 1 つとして扱う。
- 見つけた GCC ごとに、次を記録する。いずれも言語の設定によらない出力から読む。
  - バージョン: `-dumpfullversion` の表示（`14.2.0`）。
  - プレリリースか: `--version` の 1 行目が `experimental` または `prerelease` を含むか。
  - ビルド対象: `-dumpmachine` の表示（`x86_64-w64-mingw32`）。
  - C ランタイムが UCRT か: MinGW-w64 のヘッダー `_mingw.h` を読んだ後のマクロに `_UCRT` があるか（`-dM -E -include _mingw.h` で空の入力を前処理した表示）。`_UCRT` はコンパイラが定義するのではなく、`_mingw.h` が定義する。
  - GCC は、その GCC のあるディレクトリを `PATH` の先頭に加えた環境で起動する。変えるのは cuno が起動する GCC の環境だけで、利用者やシステムの `PATH` は変えない。
    - 理由: GCC の本体（`cc1`）は、同じディレクトリにある DLL を使う。そのディレクトリが `PATH` にないと、前処理もコンパイルも、何も表示せずに失敗する（MSYS2 は、標準では `PATH` を変えない）。
  - これらを調べるとき、GCC は、その GCC のあるディレクトリを作業ディレクトリにして起動する。
    - 理由: `-include` は、作業ディレクトリを最初に探す。プロジェクトのディレクトリで起動すると、プロジェクトにある `_mingw.h` が GCC のものの代わりに読まれ、C ランタイムの判定が変わる。
  - 検出でも、ビルドと同じく、コンパイラが自分で読む環境変数（[Windows での環境解決](#windows-での環境解決)）を取り除いた環境で起動する。
    - 理由: `CPATH` などが別の `_mingw.h` を指していると、UCRT の GCC を MSVCRT と判定し、入っている GCC を入れるよう案内してしまう。`GCC_EXEC_PREFIX` や `COMPILER_PATH` があると、ビルドとは別の `cc1` で調べることになる。
  - 前処理（`-dM -E`）が失敗した GCC は、実行できないものとして扱う。MSVCRT とは扱わない。
    - 理由: 前処理できない GCC はビルドもできない。MSVCRT として扱うと、UCRT の GCC を入れている利用者に、UCRT の GCC を入れるよう案内してしまう。
  - 理由: `--version` の 1 行目の最初の数字は、配布元が付けた名前の中の数字（`gcc (Buildroot 2023.02.1) 12.3.0` の `2023.02.1`）のことがある。`-v` の `gcc version` の行は、言語の設定によって訳される。
- 複数見つかった場合にどれを使うかは、LLVM と同じく「選択」が決める。選ぶのは、実行でき、プレリリースでなく、ビルド対象が `x86_64-w64-mingw32` で、C ランタイムが UCRT のもののうち、バージョンの条件を満たす最新のもの。
- C ランタイムが MSVCRT の GCC（MSYS2 の mingw64、古い配布物など）は使わない。それしか見つからない場合はエラー（終了コード 3）にし、UCRT の GCC（MSYS2 の ucrt64）を入れるよう案内する。
  - 理由: UCRT は Windows 10 以降の標準の C ランタイムで、MSVC も使う。MSVCRT の GCC と UCRT の GCC は同じビルド対象を名乗るため、区別せずに新しいものを選ぶと、どちらのランタイムの実行ファイルができるかが、その時に入っている GCC 次第になる。
- GCC を探すのは、GCC を使うとき（GCC を選ぶとき、`cuno doctor`）だけ。MSVC や Clang でのビルド、`cuno fmt`、`cuno lint` では GCC を起動しない。
- `g++` は、使う `gcc` と同じディレクトリのものを使う。`g++` の場所を別に書く設定はない。
- 選んだ GCC には、ビルドの前に、インクルードのディレクトリ（`-E -v -x c++` で空の入力を前処理したときの `#include <...> search starts here:` と `End of search list.` の間）と、ライブラリのディレクトリ（`-print-search-dirs` の `libraries: =` の行）を尋ねる。言語の設定は `LC_ALL=C` にし、その GCC のディレクトリで起動する。どちらかが読めなければエラー（終了コード 3）にする。
  - 理由: 変更の判定は、これらを MSVC の `INCLUDE` と同じように使う（[build.md](build.md#変更の判定)）。知らないままでは、プロジェクトに置いた `sys/stat.h` が GCC の `sys/stat.h` の代わりに読まれるようになっても気づかず、古いコードのまま成功と報告する。
- ビルド（コンパイルとリンク）でも、GCC は、そのディレクトリを `PATH` の先頭に加えた環境で起動する（理由は上と同じ）。作った実行ファイルを `cuno run` などで実行する環境には加えない。
  - 理由: 加えると、作った実行ファイルが GCC の DLL を必要としても cuno の中では動いてしまい、外で起動しないことに気づけない（[build.md](build.md#理由)）。
- **ASCII でないパス。** MSYS2 の GCC は、`gcc` とコンパイラの本体（`cc1`）が UTF-8 で動き、アセンブラ（`as`）とリンカ（`ld`）は ANSI コードページ（日本語版の Windows では CP932）で動く。`gcc` が渡す UTF-8 のパスを `as` と `ld` は別の文字として読むため、ASCII でないパスのファイルを開けない（日本語の名前のディレクトリ、日本語のユーザー名の一時ディレクトリ）。cuno は、次のように扱う。
  - コンパイルに `-pipe` を付ける。GCC は、付けないと、アセンブリを一時ディレクトリのファイルに書いて `as` に渡すため、一時ディレクトリが ASCII でないと（日本語のユーザー名で起きる）、どのコンパイルも失敗する。
  - GCC に渡すパスのうち、`as` と `ld` が開くもの（オブジェクト、実行ファイル、`lib-dirs`）は、プロジェクトの中にあれば、プロジェクトのルートからの相対パスで書く（GCC はルートで実行する）。プロジェクトのディレクトリ名が ASCII でなくても、ビルドできるようにするため。
  - 出力先の中のファイル名は、ASCII でない文字を書き換える（[build.md](build.md#出力先ディレクトリ)）。ソースやテストの名前が ASCII でなくても、ビルドできるようにするため。
  - それでも `as` や `ld` に ASCII でないパスを渡すことになる場合は、コンパイルの前にエラー（終了コード 3）にし、そのパスを示す。
    - 出力先のパスが、プロジェクトの中ならルートからの相対で書いても、外ならそのままで、ASCII でない: `error: the target directory <パス> is not ASCII, which MinGW's assembler and linker cannot open`、`help: build into a directory whose path is ASCII (--target-dir, CUNO_TARGET_DIR or [build] target-dir)`。
    - `lib-dirs` のディレクトリのパスが、プロジェクトのルートからの相対で書いても ASCII でない: `` error: `lib-dirs` names <パス>, which is not ASCII, which MinGW's linker cannot open ``、`help: move the libraries to a directory whose path is ASCII`。リンクするとき（`cuno check` 以外）だけ。
    - `src/bin/` のファイル名から付く実行ファイルの名前が ASCII でない: `error: <パス> names an executable that is not ASCII, which MinGW's linker cannot write`、`help: rename the file to an ASCII name, or list it in [[bin]] with an ASCII name`。`<パス>` はそのファイル。プログラムをリンクするとき（`cuno build`、`cuno run`）だけ。実行ファイルの名前は利用者が使うものなので、書き換えはしない。
    - リンクするとき、一時ディレクトリ（下記）が ASCII でなく、ASCII の短い名前もなく、LTO を使うか、そのパスを ANSI コードページで表せない: `error: the temporary directory <パス> is not ASCII and has no short name, which GCC's linker cannot use`、`help: set TMP and TEMP to a directory whose path is ASCII`。
  - **一時ディレクトリ。** GCC が使う一時ディレクトリは、Windows が決めるとおり、`TMP`、なければ `TEMP`、なければ `USERPROFILE`。GCC のリンクは、そこにファイルを作り（`collect2` は ANSI コードページで動く）、LTO のリンクと長いリンクは、そのファイルを `ld` に渡す。そのため、一時ディレクトリのパスが ASCII でないとき、cuno は、その短い名前（8.3 形式。ASCII のもの）を `TMP` と `TEMP` にして GCC を起動する。短い名前は、ボリュームが作る設定のとき（システムのドライブでは通常そう）にある。
    - 短い名前がないとき、ANSI コードページで表せるパス（日本語版の Windows の日本語の名前）なら、普通のリンクは通るのでそのまま起動する。ただし、リンクのコマンドラインがとても長い（3 万文字ほど）と、GCC は引数をその一時ディレクトリのファイルに書いて `ld` に渡すため、`ld` の、文字化けしたパスのエラーで失敗する。前もって確かめる確実な方法がないため、cuno は止めない。

  - 理由: 黙って失敗させると、GCC のアセンブラの、文字化けしたパスのエラーだけが出る。ソースの名前が ASCII でない場合は、`as` が文字化けした名前でオブジェクトを書き、cuno が記録したオブジェクトがいつまでもないため、何も変えなくても毎回コンパイルとリンクをし直す。
- GCC は MSVC のツールセットを使わないため、GCC を選んでも Visual Studio は要らない。MSVC の環境（`INCLUDE`、`LIB`、`LIBPATH`）も組み立てない。
- `[toolchain.paths] gcc` に書いた場合は、LLVM と同じく、そのファイルだけを使い、条件を満たさなければエラー（終了コード 3）にする。

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
- まだ対応していないコンパイラ（`safec`）が選ばれた場合はエラー（終了コード 3）。警告して別のコンパイラでビルドすることはしない（黙って別のコンパイラでビルドされるのを防ぐため）。

自動選択の既定: Windows は MSVC、Linux は GCC、macOS は Apple Clang。

- Windows で Visual Studio / Build Tools が見つからない場合はエラーにする（Clang も Visual Studio を必要とするため、Clang への切り替えでは解決しない）。
- **同じコンパイラが複数バージョン入っている場合**は、バージョン制約を満たすもののうち最新を選ぶ。プレビュー版は選ばない。
- **Windows で clang / clang-cl を選んだ場合も、MSVC のツールセットを 1 つ使う**（ヘッダ・ライブラリと環境の解決のため）。使うのは、完全でプレビュー版でない Visual Studio のツールセットのうち cl.exe が最新のもの。MSVC のバージョン制約は MSVC を選んだときだけ見る（[manifest.md](manifest.md)）。

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

- 対応するビルド対象環境の一覧と、各環境のツールの検出方法
