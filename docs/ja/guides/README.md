# ガイド

仕様（[specs/](../specs/README.md)）が cuno の振る舞いを決めるのに対し、ガイドは、目的ごとに仕様を読み直して、何をどう書くかを説明します。ガイドは新しいことを決めません。食い違えば仕様が正しいものです。

## はじめに

- [ほかのツールからの移行](migration.md): CMake、Visual Studio、ctest、clang-format などでしていたことを、cuno でどうするか。

## プロジェクトの種類ごと

- [ゲーム開発向けの設定](game-development.md): 例外と RTTI を切る、C ランタイムを取り込むなど、ゲームでよく選ぶ設定と、選んだときにできなくなること。
- OS とカーネル、組み込みの開発: cuno が freestanding のビルドに対応する M11 で書きます（[roadmap.md](../roadmap.md#m11-os組み込み開発freestanding)）。

## コンパイラごと

- [コンパイラごとのガイド](compilers.md): MSVC（cl）、Clang（clang / clang-cl）、GCC（MinGW-w64）の入れ方、場所とバージョンの指定、ほかのコンパイラと違うところ。
