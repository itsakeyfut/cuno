# ほかのツールからの移行

CMake、Visual Studio、ctest、clang-format などから cuno に移るときに、「前はこうしていた」ことを cuno でどうするかを、ツールごとに表にまとめます。移行は複数のツールから一度に行うことが多いので、関係する節を続けて読めるよう 1 ページにしています。

## 読み方

各表の右の列は、次の 3 種類のどれかです。

- **今できること**: cuno のコマンド、オプション、`Cuno.toml` の項目をそのまま書いています。
- **未対応（Mn）**: cuno がまだ持っていないもの。その間どうするかと、対応する予定のマイルストーン（[ロードマップ](../roadmap.md)）を書いています。
- **対応なし**: cuno に同じ概念がないもの。代わりに cuno で何をするかを書いています。

cuno の設定は `Cuno.toml` 1 つに書きます。項目の詳細は [manifest.md](../specs/manifest.md)、コマンドの詳細は [commands.md](../specs/commands.md) を見てください。

## CMake から

| CMake で | cuno で |
|---|---|
| `CMakeLists.txt` を書く | `Cuno.toml` を書く。`cuno new <パス>` で、設定なしでビルドできるパッケージを作れる |
| 既存のディレクトリに `CMakeLists.txt` を足す | 未対応（M5、`cuno init`）。その間は `Cuno.toml` を手で置く |
| `cmake -S . -B build` → `cmake --build build` | `cuno build`。生成の段階はない |
| `add_executable(app src/main.cpp)` | 何も書かない。`src/main.cpp`（または `.c`）が既定の実行ファイルになる。名前やパスを変えるなら `[[bin]]` |
| 実行ファイルを複数作る | `src/bin/` にソースを置く（ファイルごとに 1 つ）、または `[[bin]]` を複数書く |
| `add_library(...)` | 未対応（M6、`[lib]`）。その間は 1 つのパッケージのソースとして `src/` に置く |
| ソースを `file(GLOB ...)` や一覧で列挙する | 何も書かない。`src/` 以下のソースはすべてコンパイルされる（`src/main.*` と `src/bin/` は実行ファイルの入口になり、それ以外がパッケージ本体として各実行ファイルにリンクされる） |
| `target_include_directories` | `[build]` の `include-dirs` |
| `target_compile_definitions` | `[build]` の `defines` |
| `target_compile_options` | 警告の強さなどは `[build]` の項目（`warnings`、`warnings-as-errors` など）。生のフラグはコンパイラごとに `[build.msvc]` などの `flags` |
| `target_link_libraries(app d3d12)`（システムライブラリ） | `[build]` の `links`。検索パスは `lib-dirs` |
| `target_link_libraries` の `PUBLIC` / `PRIVATE` / `INTERFACE` | 対応なし。今は 1 つのパッケージの設定がそのパッケージ全体に効く。ライブラリの公開ヘッダは M6 の `[lib]` の `public-headers` で扱う |
| `set(CMAKE_CXX_STANDARD 20)` | `[package]` の `edition`（`c++20` など） |
| `-DCMAKE_BUILD_TYPE=Release` | `cuno build --release`。設定は `[profile.release]` |
| `$<CONFIG:Debug>` などの generator expression | 対応なし。構成ごとの設定は `[profile.dev.build]` / `[profile.release.build]`、コンパイラごとは `[build.msvc]` など、OS ごとは `[platform.windows.build]` などの表に書く |
| `if(WIN32)` / `if(MSVC)` | `[platform.windows.build]`、`[build.msvc]` |
| `option()` とキャッシュ変数 | 対応なし。値は `Cuno.toml` に書く。マシンごとの設定は `.cuno/config.toml` |
| `CMakePresets.json` | 対応なし。`.cuno/config.toml` と、`--release`、`--compiler` などのオプション |
| `-G "Visual Studio 17 2022"` / `-T ClangCL` | `[toolchain]` の `compiler`、または `cuno build --compiler clang-cl` |
| `find_package(Foo)` | 未対応（M6 / M7、`cuno add`）。その間は `include-dirs`、`links`、`lib-dirs` で手で指定する |
| `FetchContent` / `ExternalProject` | 未対応（M6 で Git / URL の依存、M7 でレシピが既存の CMake を裏で呼ぶ） |
| `add_custom_command` / `add_custom_target` | 未対応（M7、`[[build.rule]]`） |
| `ctest` / `add_test` | 「ctest とテストフレームワークから」を参照 |
| `install()` / `export()` | 未対応（M10、`cuno install`） |
| `CMAKE_EXPORT_COMPILE_COMMANDS=ON` | 何もしない。`compile_commands.json` はビルドのたびにプロジェクト直下に書かれる |
| `cmake --build build --verbose` | `cuno build -v`（実行したコンパイラのコマンドを表示する） |
| `rm -rf build` | `cuno clean` |

## Visual Studio のプロジェクト（`.sln` / `.vcxproj`）から

| Visual Studio で | cuno で |
|---|---|
| ソリューションをビルドする（Ctrl+Shift+B） | `cuno build`。開発者コマンドプロンプトは要らない。cuno が Visual Studio を見つけて環境を整える |
| Debug / Release の構成を切り替える | 既定が dev（Debug 相当）。`cuno build --release` |
| プラットフォームの切り替え（x64 など） | 今は x64。ほかのターゲットは未対応（M8、`--target`） |
| プラットフォーム ツールセット（v143、ClangCL） | `[toolchain]` の `compiler`（`msvc` / `clang-cl` / `clang`）。最低バージョンは `[toolchain.version]` |
| プロパティページの「追加のインクルード ディレクトリ」 | `[build]` の `include-dirs` |
| 「プリプロセッサの定義」 | `[build]` の `defines`。構成ごとは `[profile.dev.build]` / `[profile.release.build]` |
| 「警告レベル」「警告をエラーとして扱う」 | `[build]` の `warnings`、`warnings-as-errors` |
| 「ランタイム ライブラリ」（/MD、/MT） | `[build]` の `runtime`（`dynamic` / `static`） |
| 「追加の依存ファイル」（`d3d12.lib`） | `[build]` の `links`（`.lib` は付けずに `d3d12`） |
| 「追加のオプション」（コンパイラ） | `[build.msvc]` の `flags` |
| 「サブシステム」（コンソール / Windows） | `[[bin]]` の `subsystem`（`console` / `windows`） |
| プロパティシート（`.props`）で設定を共有する | 対応なし。設定は `Cuno.toml` に書き、マシンごとの設定は `.cuno/config.toml` |
| F5 でデバッグ実行する | 未対応（M9、`cuno ide vs`）。その間は `cuno build` の実行ファイルを Visual Studio で開いてデバッグできる |
| 実行する（Ctrl+F5） | `cuno run`。作業ディレクトリはプロジェクトのルート |
| 「コマンド引数」 | `cuno run -- <引数>` |
| スタートアップ プロジェクトを選ぶ | `cuno run --bin <名前>` |
| ソリューションに複数のプロジェクトを並べる | 未対応（M9、ワークスペース） |
| テスト エクスプローラー | `cuno test`。「ctest とテストフレームワークから」を参照 |
| 環境が足りないときに調べる | `cuno doctor` |

## Ninja / Make から

| Ninja / Make で | cuno で |
|---|---|
| `ninja` / `make` | `cuno build`。cuno が自分でビルドするので、Ninja も Make も要らない |
| `make -j8` | `cuno build -j 8`（既定は CPU コア数） |
| 変更したファイルだけビルドし直す | 何もしない。変わったソースと、それが include するヘッダを cuno が追う |
| `make clean` | `cuno clean` |
| `make run` などの自前のターゲット | `cuno run`。自前の処理は未対応（M7、`[[build.rule]]`） |
| `CFLAGS` / `CXXFLAGS` | `[build]` の項目。生のフラグは `[build.msvc]` などの `flags` |
| 依存ファイル（`-MMD`、`.d`）を自分で扱う | 何もしない |

## ctest とテストフレームワークから

cuno のテストは、同梱のヘッダで書き、`cuno test` で動かします。書き方は [testing.md](../specs/testing.md) にあります。C++20 では `cuno/test.hpp`、C と C++17 では `cuno/test.h` を使います。

| ctest / テストフレームワークで | cuno で |
|---|---|
| `enable_testing()` と `add_test(...)` | 何も書かない。`tests/` の直下のファイルと、`#ifdef CUNO_TEST` で囲んだソース内のテストが見つかる |
| `ctest` | `cuno test` |
| `ctest -R pattern` | `cuno test pattern`（名前に含む文字列で絞り込む） |
| `ctest -j8` | `cuno test -j 8`。テストは並列に動き（並列数の既定は CPU コア数）、1 つずつ別のプロセスで動く |
| `ctest --output-on-failure` | 何もしない。失敗したテストの出力は既定で表示される。常に表示するなら `cuno test --nocapture` |
| GoogleTest の `TEST(Suite, Name)` | `test("name", [] { ... })`（C++20）、`cuno_test(name)`（C / C++17） |
| `EXPECT_EQ(a, b)` / `ASSERT_EQ(a, b)` | `expect(eq(a, b))` / `require(eq(a, b))`、または `cuno_check_eq(a, b)` / `cuno_require_eq(a, b)` |
| Catch2 の `CHECK` / `REQUIRE`、doctest の `CHECK` / `REQUIRE` | `expect` / `require`、または `cuno_check` / `cuno_require` |
| 浮動小数点数の比較（`EXPECT_NEAR`、`Approx`） | `approx(a, b, 誤差)`、`cuno_check_approx(a, b, 誤差)` |
| `EXPECT_THROW` | `throws<例外型>(関数)` |
| `GTEST_SKIP()` | `skip(理由)`、`cuno_skip(理由)` |
| 既存の GoogleTest / Catch2 / doctest のテストをそのまま動かす | 未対応（M10、`[test]` の `framework`）。その間は cuno のテストに書き直すか、別の実行ファイルとして `src/bin/` に置いて `cuno run --bin` で動かす |
| ベンチマーク（Google Benchmark など） | 未対応（M10、`cuno bench`） |

## clang-format から

| clang-format で | cuno で |
|---|---|
| `.clang-format` を書く | `Cuno.toml` の `[fmt]` に書く。`style`（`cuno`、`llvm`、`google`、`microsoft` など）、`indent-width`、`column-limit`、`use-tabs` |
| `.clang-format` のほかのオプション | `[fmt.clang-format]` に clang-format のオプションをそのまま書く |
| `clang-format -i $(git ls-files '*.cpp' '*.h')` | `cuno fmt`。パッケージのコード（`src/`、`tests/`、`examples/`、`benches/` など）を対象にする |
| CI で `clang-format --dry-run --Werror` | `cuno fmt --check`（整形されていないファイルがあれば終了コード 1） |
| 整形しないファイルを除く | `[fmt]` の `exclude` |
| エディタが `.clang-format` を読む | 何もしない。`cuno build`、`cuno fmt`、`cuno lint` が `Cuno.toml` から `.clang-format` をプロジェクト直下に書き出す |

## clang-tidy から

| clang-tidy で | cuno で |
|---|---|
| `.clang-tidy` の `Checks:` を書く | `Cuno.toml` の `[lint]`。`preset`（`recommended` / `strict` / `none`）を選び、`allow` / `warn` / `deny` で調整する |
| `WarningsAsErrors:` | `[lint]` の `deny` |
| `run-clang-tidy -p build` | `cuno lint`。`compile_commands.json` は cuno が用意する |
| `clang-tidy --fix` | `cuno lint --fix` |
| 解析しないファイルを除く | `[lint]` の `exclude` |
| エディタが `.clang-tidy` を読む | 何もしない。`cuno build`、`cuno fmt`、`cuno lint` が `Cuno.toml` から `.clang-tidy` をプロジェクト直下に書き出す |

## vcpkg / Conan から

依存管理は M6 と M7 で入ります。それまでは、ライブラリのヘッダとライブラリファイルの場所を `[build]` に書いて使います。

| vcpkg / Conan で | cuno で |
|---|---|
| `vcpkg.json` / `conanfile.txt` に依存を書く | 未対応（M6、`Cuno.toml` の `[dependencies]`） |
| `vcpkg install sdl3` / `conan install` | 未対応（M6 で Git / URL、M7 で `cuno add sdl3`） |
| `vcpkg-configuration.json` のベースライン、`conan.lock` | 未対応（M6、`Cuno.lock`） |
| features（vcpkg）/ options（Conan） | 未対応（M7） |
| バイナリキャッシュ | 未対応。マシン内のキャッシュは依存管理とともに入る予定（時期は未定）。チームや CI での共有は将来 |
| 依存のソースをリポジトリに置く | 未対応（M7、`cuno vendor`）。その間は、ソースを `src/` 以下に置けば一緒にビルドされる |
| ビルド済みのライブラリを使う | `[build]` の `include-dirs`、`lib-dirs`、`links` |

## Cargo から

cuno は Cargo を手本にしているので、多くのことは同じ形でできます。

| Cargo で | cuno で |
|---|---|
| `cargo new` | `cuno new` |
| `cargo init` | 未対応（M5、`cuno init`） |
| `cargo build` / `cargo build --release` | `cuno build` / `cuno build --release` |
| `cargo run -- args` / `cargo run --bin name` | `cuno run -- args` / `cuno run --bin name` |
| `cargo check` | `cuno check`（`--tests`、`--all-targets` も同じ） |
| `cargo test filter` | `cuno test filter`。`#[cfg(test)]` の代わりに `#ifdef CUNO_TEST` |
| `cargo fmt` / `cargo fmt --check` | `cuno fmt` / `cuno fmt --check` |
| `cargo clippy` / `cargo clippy --fix` | `cuno lint` / `cuno lint --fix` |
| `cargo clean` | `cuno clean` |
| `Cargo.toml` の `[package]`、`[[bin]]`、`[profile.release]` | `Cuno.toml` の同じ名前の表 |
| `.cargo/config.toml` | `.cuno/config.toml` |
| `[dependencies]` | 未対応（M6） |
| `build.rs` | 未対応（M7、`[[build.rule]]` と Lua のビルドスクリプト） |
| `rustup` | 未対応（M5、`cuno toolchain list`）。コンパイラのインストールは将来。今は `cuno doctor` が見つけたコンパイラを示す |
| `rustup update`（ツール自体の更新） | 未対応（M5、`cuno self update`） |
| `cargo install --path .` | 未対応（M10、`cuno install`） |
| ワークスペース | 未対応（M9） |

## Premake から

| Premake で | cuno で |
|---|---|
| `premake5.lua` を書く | `Cuno.toml` を書く。Lua のスクリプトは要らない |
| `premake5 vs2022` でプロジェクトを生成してビルドする | `cuno build`。生成の段階はない。Visual Studio のプロジェクトの生成は未対応（M9、`cuno ide vs`） |
| `kind "ConsoleApp"` / `"WindowedApp"` | `[[bin]]` の `subsystem`（`console` / `windows`） |
| `kind "StaticLib"` / `"SharedLib"` | 未対応（M6、`[lib]` の `kind`） |
| `files { "src/**.cpp" }` | 何も書かない。`src/` 以下のソースはすべてコンパイルされる（`src/main.*` と `src/bin/` は実行ファイルの入口になり、それ以外がパッケージ本体として各実行ファイルにリンクされる） |
| `includedirs` / `defines` / `links` / `libdirs` | `[build]` の `include-dirs` / `defines` / `links` / `lib-dirs` |
| `buildoptions` | `[build.msvc]` などの `flags` |
| `configurations { "Debug", "Release" }` と `filter "configurations:Debug"` | `[profile.dev.build]` / `[profile.release.build]` |
| `filter "system:windows"` | `[platform.windows.build]` |
| `cppdialect "C++20"` | `[package]` の `edition` |
| `staticruntime "On"` | `[build]` の `runtime = "static"` |
| `prebuildcommands` / 自前の Lua の処理 | 未対応（M7、`[[build.rule]]` と Lua のビルドスクリプト） |

## Meson から

| Meson で | cuno で |
|---|---|
| `meson.build` を書く | `Cuno.toml` を書く |
| `meson setup builddir` → `meson compile -C builddir` | `cuno build`。設定の段階はない |
| `executable('app', sources)` | 何も書かない。`src/main.cpp` が既定の実行ファイルになる |
| `meson test` | `cuno test` |
| `--buildtype=release` | `cuno build --release` |
| `default_options: ['cpp_std=c++20']` | `[package]` の `edition` |
| `warning_level=3`、`werror=true` | `[build]` の `warnings`、`warnings-as-errors` |
| `meson_options.txt` | 対応なし。値は `Cuno.toml` に書き、マシンごとの設定は `.cuno/config.toml` |
| `dependency('zlib')`、wrap | 未対応（M6 / M7） |
| `custom_target` | 未対応（M7、`[[build.rule]]`） |
| クロスファイル（`--cross-file`） | 未対応（M8、`--target`） |

## Bazel から

| Bazel で | cuno で |
|---|---|
| `BUILD` と `MODULE.bazel` を書く | `Cuno.toml` を書く |
| `bazel build //app:main` | `cuno build` |
| `bazel run //app:main -- args` | `cuno run -- args` |
| `bazel test //...` | `cuno test` |
| `cc_binary` | `src/main.cpp` か `src/bin/`、または `[[bin]]` |
| `cc_library` | 未対応（M6、`[lib]`） |
| `copts` / `defines` / `includes` | `[build.msvc]` などの `flags`、`[build]` の `defines`、`include-dirs` |
| `select({"@platforms//os:windows": ...})` | `[platform.windows.build]`。コンパイラごとは `[build.msvc]` など |
| `-c opt` | `cuno build --release` |
| `genrule` | 未対応（M7、`[[build.rule]]`） |
| リモートキャッシュ / リモート実行 | 対応なし。ビルドキャッシュは M9 |
| `bazel query`、`compile_commands.json` の抽出ツール | `compile_commands.json` はビルドのたびにプロジェクト直下に書かれる。パッケージの情報の JSON 出力は未対応（M9、`cuno metadata`） |
