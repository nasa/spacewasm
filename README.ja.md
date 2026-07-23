<h1 align="center">SpaceWasm</h2>

<p align="center">
<img src="docs/logo.svg" width="150" height="150">
</p>

<p align="center">
  <a href="https://github.com/nasa/spacewasm/actions/workflows/ci.yml"><img src="https://github.com/nasa/spacewasm/actions/workflows/ci.yml/badge.svg" /></a>
  <a href="https://codecov.io/gh/nasa/spacewasm"><img src="https://codecov.io/gh/nasa/spacewasm/graph/badge.svg?token=3EddiLtM36"/></a>
  <a href="#license"><img src="https://img.shields.io/badge/license-Apache%202.0-blue" alt="license" /></a>
</p>

<p align="center">
  <a href="README.md">English</a> | <a href="README.zh-CN.md">简体中文</a> | 日本語
</p>

SpaceWasm は、宇宙機上で Wasm バイナリを解釈するための
[Wasm 1.0](https://webassembly.github.io/spec/versions/core/WebAssembly-1.0.pdf) 仕様の実装です。[NASA JPL](https://www.jpl.nasa.gov) で開発されています。

## 理由

1. **シーケンシング**：宇宙機の高レベルな活動は通常、組み込みフライトソフトウェアの外部でコマンドシーケンスとして記述されます。
こうした活動には、火星探査車の走行やアームの操作から、温度範囲が正常かどうかの確認まで、さまざまなものがあります。
これまでシーケンスの形式と機能はミッションごとに異なり、多様で断片化した実装が生じていました。
SpaceWasm は業界標準を実装し、これらを統合します。

2. **サンドボックス化**：フライトソフトウェアの開発は、要件と対象範囲が厳しく制約されるため、コストと時間がかかります。新しいフライトソフトウェア機能の検証では、
システム全体との相互作用を検証しなければならないことがよくあります。そのため V&V の期間が長くなり、テストベッド資源の競合も激しくなり、新しい自律ソフトウェアを
実際の飛行に導入することが難しくなります。WebAssembly を使用すれば、信頼できない、または信頼度の低い実行ファイルを宇宙機に搭載しつつ、
フライトソフトウェア側でアクセス権限と計算時間を制限し、健全性と安全性を監視できます。

3. **可搬性**：WebAssembly は明確に定義されたインターフェースとサンドボックス機構を備えているため、別のプラットフォームへの移植が容易です。

4. **ツール**：WebAssembly を標準とすることで、豊富なツールと研究成果を持つ広範なコミュニティを活用できます！

## 概要

このソフトウェアは、次の 2 つの主要コンポーネントで構成されています。

1. デコーダー／バリデーター：

   Wasm バイナリを[チャンク](#ストリーミング)単位で読み込み、実行可能な形式にデコードします。デコーダーが使用するメモリ量は固定されており、
   地上では `spacewasm-check` 実行ファイルを使用して Wasm バイナリごとに測定できます。

   WebAssembly はデコード中に検証されるため、バイトコードをもう一度走査する必要はありません。

2. インタープリタ：

   線形メモリ上で動作し、[組み込み](#組み込み)側から提供されるフックと連携できる
   Wasm インタープリタです。

SpaceWasm は WebAssembly バイトコードを直接実行しません。Wasm バイトコードは小さく、検証しやすい構造になるよう設計されていますが、
その性質のため、そのまま実行すると低速です。SpaceWasm は Wasm 命令のデコード中に、
バイトコードを解釈実行に適した特性を持つ別の中間表現（IR）へ変換します。IR の詳細については
[仕様](src/SPEC.md)を参照してください。

## 要件

SpaceWasm の要件は、[DLR](https://github.com/DLR-FT/wasm-interpreter) による類似の取り組みを基に定められています。

[要件](./REQUIREMENTS.md)を参照してください。

## 組み込み

インタープリタの組み込みとは、インタープリタをインスタンス化し、モジュールがインポートする関数の実装を提供することです。
通常、モジュールがインポートする関数の集合は固定されており、Wasm モジュール側と組み込み側の両方で
コンパイル時に指定する必要があります。

## 動的割り当て

SpaceWasm は独自の動的メモリ割り当てモデルを採用しています。設計上の選択はすべて、一般的なフライトソフトウェア標準の要件に基づいています。
動的割り当ては次の規則に従います。

1. すべての割り当ては、_ページ_と呼ばれる一定数の固定サイズブロック単位で行われます。これらのページは Wasm の線形メモリページとは異なります。
2. 割り当てより前に解放を行うことはできません。
3. ページ内の部分領域は拡大も縮小もできず、サイズを事前に固定する必要があります。
4. メモリ使用量は決定論的でなければなりません。
5. 割り当てが失敗しても panic を引き起こしては_なりません_。

Rust 標準の[アロケーション](https://doc.rust-lang.org/alloc/)は、カスタムアロケータを使用してもこれらの制約を満たしません。
そのため SpaceWasm は、これらの性質を保証する独自のデータ構造を提供しています。これらのデータ構造だけが、
プロジェクト内で Rust の `unsafe` セマンティクスを使用しています。

> [!NOTE]
> これらの制限はインタープリタの実装にのみ適用され、解釈対象の Wasm バイトコードには適用され_ません_。

Wasm の線形メモリページは、動的メモリページの外部に割り当てられます。

## ストリーミング

宇宙機に搭載される小型システムでは、_ピーク_メモリ使用量が重要な制約になることがよくあります。多くの Wasm インタープリタは、
Wasm バイナリ全体を 1 つの連続したデータとして渡す必要があります。同じメモリ領域を異なる目的で再利用できるシステムでは、
通常これは問題になりません。しかし、宇宙機のフライトソフトウェアでは一般に、特定の用途ごとに固定のメモリ領域が割り当てられます。
したがって、Wasm バイナリ全体を単一の連続領域に収めることは現実的ではありません。

SpaceWasm は、ピークメモリ使用量を減らし、ストリーミングに必要な割り当て後の解放を不要にするよう高度に最適化されています。
そのため、WebAssembly 仕様にはいくつかの[制約](#インタープリタの制限)が課されています。

SpaceWasm はストリーミング機構により、Wasm バイナリを 1 回の走査でデコードおよびコンパイルできます。ファイルシステムから
Wasm バイナリのチャンクを読み取る、または要求するたびに、それをインタープリタへ渡せます。ストリームはチャンクを同期的に提供する必要があります。


## WASI 0.1 サポート
[`spacewasi`](crates/spacewasi#readme) crate は、WASI 0.1（`wasip1`）仕様に準拠する任意の WASM モジュールをサンドボックス環境で実行できるバイナリを提供します。コマンドラインフラグを使用して、ホストのディレクトリと環境変数をマウントできます。

```bash
# compile example from crates/spacewasi/tests/wasm/
$ clang --target=wasm32-wasip1 -mcpu=mvp hello_universe.c -o hello_universe.wasm

# convert module to MVP compatible file
$ crates/spacewasi/scripts/wasm2mvp.sh hello_universe.wasm

$ spacewasi hello_universe.wasm
hello universe!
```

このコマンドと WASI の基本的なコンパイル方法については、[`spacewasi/README.md`](crates/spacewasi/README.md)を参照してください。

## インタープリタの制限

この Wasm インタープリタは、資源が制約された宇宙機環境をサポートするため、WebAssembly 1.0 仕様を超える追加の制約を課します。

制限の完全な一覧については、[IR 仕様](./src/SPEC.md)を参照してください。

これらの制約により、資源が制約された環境で決定論的なメモリ使用と効率的な実行を実現しながら、
ほとんどの標準 WebAssembly モジュールとの互換性を維持できます。

### Wasm モジュール作成者向けの制限

SpaceWasm はバイトコードを固定幅の IR にコンパイルします。IR は通常、元のバイトコードより大きいため、
生のモジュールサイズの実用上の上限は、上記の IR コードページ上限（約 8 GiB の IR）によって決まります。
これは飛行用ハードウェアで想定されるどのモジュールよりもはるかに大きく、実際の制約要因は、
[ストリーミング](#ストリーミング)デコーダーに設定されるピークメモリです。この値は地上で `spacewasm-check` を使用し、モジュールごとに測定されます。

> [!NOTE]
> `spacewasm-check` はまだ開発されていません。`spacewasm-std` に同様のツールがあります。

Wasm モジュールの開発者に関係する可能性のある制限をいくつか示します。

| 制限                 | 値                  | 備考                                                                                                                                                                                                                                                      |
| -------------------- | ------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Wasm ページサイズ    | 64 KiB / 1 B        | [Custom-Page-Sizes 提案](https://github.com/WebAssembly/custom-page-sizes)をサポート                                                                                                                                                                      |
| 線形メモリページ     | 4 GiB               | Wasm 1.0 仕様に準拠します。これより多くのページを宣言する（または `max` をこれより大きくする）モジュールは拒否されます。組み込み側で必ず制限されますが、その内容はインタープリタの配備方法に依存します。                                                     |
| IR コード            | 8 GiB               | 生のバイトコードではなく、コンパイル済み IR の値です。この制限はストア内のすべてのモジュールにまたがります。`spacewasm-std` は IR／バイトコード比を「compilation ratio」として表示します。この比率は命令の種類によって異なるため、事前の見積もりは困難です。 |
| 関数パラメータ       | 255 個の 32 ビットワード    | 関数ごと。                                                                                                                                                                                                                                        |
| ローカル変数         | 65,535 個の 32 ビットワード | 関数ごと。                                                                                                                                                                                                                                        |

## ベンチマーク

SpaceWasm は Coremark ベンチマークでテストし、性能の回帰を追跡しています。
詳細については [coremark](crates/spacewasm_std/benches)を参照してください。

## テスト

### ユニットテストと統合テスト

```bash
cargo test
```

ユニットテストは、SpaceWasm 独自の `alloc` 使用法に伴って提供される `unsafe` コンテナ抽象化の回帰を検査します。
また、WAST 全体を実行せずにすべての Wasm 命令を対象とする簡単なユニットテストもあります。

統合テストには Wasm 1.0 MVP テストスイートの仕様テストを使用しています。
このテスト群は https://github.com/WasmEdge/wasmedge-spectest から選定されたものです。
これらのテストは、Wasm インタープリタが仕様に準拠していることを検証します。

### ファジング

SpaceWasm には、libfuzzer と
[wasm-smith](https://github.com/bytecodealliance/wasm-tools/tree/main/crates/wasm-smith) を使用した包括的なファジング基盤が含まれています。

```bash
# Run fuzzer
make fuzz

# Analyze crashes with execution traces
make trace CRASH=fuzz/artifacts/no_traps/crash-xxx
```

## 機能サポートマトリクス

次の表は、実装済みおよび実装予定の WebAssembly 提案と、対応する追跡 Issue または実装 PR へのリンクを示します。

| 機能                                                                                                         | 状態                                                   |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------ |
| [Wasm MVP](https://www.w3.org/TR/2019/REC-wasm-core-1-20191205/)                                             | すべてのバージョン                                     |
| [可変グローバル](https://github.com/WebAssembly/mutable-global)                                              | すべてのバージョン                                     |
| [カスタムページサイズ](https://github.com/WebAssembly/custom-page-sizes)                                     | [≥0.2.0](https://github.com/nasa/spacewasm/pull/84)    |
| [バルクメモリ操作](https://github.com/WebAssembly/bulk-memory-operations)                                    | [予定](https://github.com/nasa/spacewasm/issues/54)    |
| [符号拡張演算子](https://github.com/WebAssembly/sign-extension-ops)                                          | [予定](https://github.com/nasa/spacewasm/issues/55)    |
| [非トラップ浮動小数点数から整数への変換](https://github.com/WebAssembly/nontrapping-float-to-int-conversions) | [予定](https://github.com/nasa/spacewasm/issues/56)    |
| [マルチバリュー](https://github.com/WebAssembly/multi-value)                                                 | 検討中                                                 |
| [マルチメモリ](https://github.com/WebAssembly/multi-memory)                                                  | 検討中                                                 |

現在、その他の提案はいずれも実装、計画、検討されていません。

## クレジットと謝辞

このプロジェクトの一部は、次のオープンソースプロジェクトを基にしています。

- [rust-lang/rust](https://github.com/rust-lang/rust)：MIT OR Apache License 2.0 のデュアルライセンス。
- [DLR-FT/wasm-interpreter](https://github.com/DLR-FT/wasm-interpreter)：Apache License 2.0。
- [Wasmtime](https://github.com/bytecodealliance/wasmtime)：LLVM 例外条項付き Apache License 2.0。
- [WABT](https://github.com/webassembly/wabt)：Apache License 2.0。
- [wasmedge-spectest](https://github.com/WasmEdge/wasmedge-spectest)：MIT ライセンス。
- [WebAssembly Testsuite](https://github.com/WebAssembly/testsuite)：Apache License 2.0。
- [Coremark](https://github.com/eembc/coremark)：COREMARK ACCEPTABLE USE AGREEMENT。
- [Wasm Coremark](https://github.com/wasm3/wasm-coremark)：上流にはライセンスファイルがありません。内包されている CoreMark ペイロードには COREMARK ACCEPTABLE USE AGREEMENT が適用されます。
