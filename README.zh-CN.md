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
  <a href="README.md">English</a> | 简体中文 | <a href="README.ja.md">日本語</a>
</p>

SpaceWasm 是 [Wasm 1.0](https://webassembly.github.io/spec/versions/core/WebAssembly-1.0.pdf)
规范的一种实现，用于在航天器上解释 Wasm 二进制文件。它由 [NASA JPL](https://www.jpl.nasa.gov) 开发。

## 理由

1. **指令序列**：航天器的高级活动通常以指令序列的形式编码在嵌入式飞行软件之外。
这些活动可以包括驾驶火星车及操作其机械臂，也可以包括检查温度范围是否正常。
过去，不同任务所用序列的形式和功能各不相同，造成实现方案繁杂且零散。
SpaceWasm 实现了一项行业标准，从而统一这些实现。

2. **沙箱**：由于飞行软件的需求和范围受到严格约束，其开发成本与耗时都很高。验证一项新的飞行软件能力
通常需要验证它与整个系统的交互。这会延长验证与确认（V&V）周期，并加剧对测试平台资源的争用，使新的自主软件难以
进入实际飞行环境。WebAssembly 使不受信任或低信任的可执行文件能够进入航天器，
同时让飞行软件可以限制其访问权限和计算时间，并监控健康与安全状况。

3. **可移植性**：WebAssembly 提供定义明确的接口和沙箱机制，让迁移到其他平台变得简单。

4. **工具生态**：将 WebAssembly 作为标准，可以接入一个拥有丰富工具和研究成果的广泛社区！

## 概述

本软件包含两个主要组件：

1. 解码器/验证器：

   它分[块](#流式处理)读取 Wasm 二进制文件，并将其解码为可执行形式。解码器使用固定
   大小的内存；在地面端，可使用 `spacewasm-check` 可执行文件测量每个 Wasm 二进制文件的内存用量。

   WebAssembly 会在解码过程中完成验证，无需再次遍历字节码。

2. 解释器：

   一个可在线性内存上运行，并能与[嵌入](#嵌入)方提供的钩子交互的
   Wasm 解释器。

SpaceWasm 不直接执行 WebAssembly 字节码。Wasm 字节码的设计目标是体积小、结构便于验证，
但这些特性也会降低其原地执行的速度。在解码 Wasm 指令的过程中，SpaceWasm
会将字节码转换为另一种中间表示（IR），其中包含更适合解释执行的属性。有关 IR 的更多信息，请参阅
[规范](src/SPEC.md)。

## 要求

SpaceWasm 的要求源自 [DLR](https://github.com/DLR-FT/wasm-interpreter) 开展的类似工作。

请参阅[要求](./REQUIREMENTS.md)。

## 嵌入

嵌入解释器是指实例化解释器，并为模块所导入的函数提供实现。
通常，模块导入的函数集合是固定的，应在编译时同时为 Wasm 模块和嵌入方
指定这些函数。

## 动态分配

SpaceWasm 采用独特的动态内存分配模型。其所有设计选择都源自常见飞行软件标准提出的要求。
动态分配遵循以下规则：

1. 所有分配都基于离散的固定大小块（称为_页_）进行。这些页不同于 Wasm 的线性内存页。
2. 释放操作不得先于分配操作。
3. 页内的子区域不能增长或缩小，其大小应预先固定。
4. 内存使用必须具有确定性。
5. 任何分配失败都_不得_导致 panic。

即使使用自定义分配器，Rust 标准[分配](https://doc.rust-lang.org/alloc/)机制也无法满足这些约束。
因此，SpaceWasm 提供了自己的数据结构，以保证这些属性。你会发现，这些数据结构包含
项目中唯一使用 Rust `unsafe` 语义的代码。

> [!NOTE]
> 这些限制仅适用于解释器的实现，_不_适用于解释器所要解释的 Wasm 字节码。

Wasm 线性内存页在动态内存页之外分配。

## 流式处理

对于航天器上的小型系统而言，_峰值_内存用量通常是一项重要约束。许多 Wasm 解释器
要求以一个连续的数据块向解释器提供整个 Wasm 二进制文件。对于同一片内存区域可复用于不同用途的系统，
这通常没有问题。但航天器飞行软件通常会为特定用途划分固定的内存区域。因此，要求整个 Wasm
二进制文件放入单个连续数据块并不可行。

SpaceWasm 经过高度优化，可降低峰值内存用量，并避免流式处理所需的分配完成后再进行释放。
为此，它对 WebAssembly 规范施加了一些[约束](#解释器限制)。

SpaceWasm 通过流式机制，支持单次遍历完成 Wasm 二进制文件的解码和编译。可以在从文件系统
读取或请求 Wasm 二进制文件的各个数据块时，将它们提供给解释器。数据流必须同步提供这些数据块。


## WASI 0.1 支持
[`spacewasi`](crates/spacewasi#readme) crate 提供了一个二进制程序，可在沙箱环境中运行遵循 WASI 0.1（`wasip1`）规范的任意 WASM 模块。命令行参数可用于挂载主机目录和环境变量：

```bash
# compile example from crates/spacewasi/tests/wasm/
$ clang --target=wasm32-wasip1 -mcpu=mvp hello_universe.c -o hello_universe.wasm

# convert module to MVP compatible file
$ crates/spacewasi/scripts/wasm2mvp.sh hello_universe.wasm

$ spacewasi hello_universe.wasm
hello universe!
```

有关此命令和 WASI 基本编译方式的更多信息，请参阅 [`spacewasi/README.md`](crates/spacewasi/README.md)。

## 解释器限制

为了支持资源受限的航天器环境，此 Wasm 解释器施加了超出 WebAssembly 1.0 规范的额外约束。

有关完整的限制列表，请参阅我们的 [IR 规范](./src/SPEC.md)。

这些约束既能实现确定性的内存使用和资源受限环境中的高效执行，
又能保持与大多数标准 WebAssembly 模块的兼容性。

### 面向 Wasm 模块制作者的限制

SpaceWasm 会将字节码编译为固定宽度的 IR，而 IR 通常大于原始字节码，因此原始模块大小的
实际上限受上述 IR 代码页上限（约 8 GiB IR）约束。这远大于飞行硬件上预期使用的任何模块；
实践中的约束因素是为[流式处理](#流式处理)解码器配置的峰值内存，
该值会在地面端使用 `spacewasm-check` 按模块测量。

> [!NOTE]
> `spacewasm-check` 尚未开发。`spacewasm-std` 中提供了类似工具。

以下是几项可能与 Wasm 模块开发者相关的限制。

| 限制                 | 值                  | 说明                                                                                                                                                                                                                                               |
| -------------------- | ------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Wasm 页大小          | 64 KiB / 1 B        | 支持 [Custom-Page-Sizes 提案](https://github.com/WebAssembly/custom-page-sizes)                                                                                                                                                                    |
| 线性内存页           | 4 GiB               | 遵循 Wasm 1.0 规范。声明更多页数（或将 `max` 设为更大值）的模块会被拒绝。请注意，嵌入方必然会加以限制，但具体限制取决于解释器的部署方式。                                                                                                           |
| IR 代码              | 8 GiB               | 指编译后的 IR，而不是原始字节码。此限制适用于存储区中的所有模块。`spacewasm-std` 会将 IR/字节码比率打印为“compilation ratio”。由于该比率随所用指令类型而异，因此很难预先估算。                                                                      |
| 函数参数             | 255 个 32 位字      | 每个函数。                                                                                                                                                                                                                                         |
| 局部变量             | 65,535 个 32 位字   | 每个函数。                                                                                                                                                                                                                                         |

## 基准测试

SpaceWasm 使用 Coremark 基准测试来追踪性能回归。
有关更多信息，请参阅 [coremark](crates/spacewasm_std/benches)。

## 测试

### 单元测试与集成测试

```bash
cargo test
```

单元测试会检查 SpaceWasm 因独特的 `alloc` 用法而提供的 `unsafe` 容器抽象是否发生回归。
此外，还有一些简单的单元测试，无需执行完整的 WAST 即可覆盖所有 Wasm 指令。

集成测试使用 Wasm 1.0 MVP 测试套件中的规范测试，
该套件整理自 https://github.com/WasmEdge/wasmedge-spectest。
这些测试用于验证 Wasm 解释器是否符合规范。

### 模糊测试

SpaceWasm 包含一套使用 libfuzzer 和
[wasm-smith](https://github.com/bytecodealliance/wasm-tools/tree/main/crates/wasm-smith) 的完整模糊测试基础设施。

```bash
# Run fuzzer
make fuzz

# Analyze crashes with execution traces
make trace CRASH=fuzz/artifacts/no_traps/crash-xxx
```

## 功能支持矩阵

下表列出了已实现和计划实现的 WebAssembly 提案，并附有对应跟踪 Issue 或实现 PR 的链接。

| 功能                                                                                                         | 状态                                                   |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------ |
| [Wasm MVP](https://www.w3.org/TR/2019/REC-wasm-core-1-20191205/)                                             | 所有版本                                               |
| [可变全局变量](https://github.com/WebAssembly/mutable-global)                                                | 所有版本                                               |
| [自定义页大小](https://github.com/WebAssembly/custom-page-sizes)                                             | [≥0.2.0](https://github.com/nasa/spacewasm/pull/84)    |
| [批量内存操作](https://github.com/WebAssembly/bulk-memory-operations)                                        | [计划中](https://github.com/nasa/spacewasm/issues/54)  |
| [符号扩展运算符](https://github.com/WebAssembly/sign-extension-ops)                                          | [计划中](https://github.com/nasa/spacewasm/issues/55)  |
| [非陷阱浮点数到整数转换](https://github.com/WebAssembly/nontrapping-float-to-int-conversions)                 | [计划中](https://github.com/nasa/spacewasm/issues/56)  |
| [多值](https://github.com/WebAssembly/multi-value)                                                           | 正在考虑                                               |
| [多内存](https://github.com/WebAssembly/multi-memory)                                                        | 正在考虑                                               |

目前，所有其他提案都尚未实现、规划或考虑。

## 项目来源与致谢

本项目的部分内容改编自以下开源项目：

- [rust-lang/rust](https://github.com/rust-lang/rust)，采用 MIT OR Apache License 2.0 双许可证。
- [DLR-FT/wasm-interpreter](https://github.com/DLR-FT/wasm-interpreter)，采用 Apache License 2.0。
- [Wasmtime](https://github.com/bytecodealliance/wasmtime)，采用带 LLVM 例外条款的 Apache License 2.0。
- [WABT](https://github.com/webassembly/wabt)，采用 Apache License 2.0。
- [wasmedge-spectest](https://github.com/WasmEdge/wasmedge-spectest)，采用 MIT 许可证。
- [WebAssembly Testsuite](https://github.com/WebAssembly/testsuite)，采用 Apache License 2.0。
- [Coremark](https://github.com/eembc/coremark)，采用 COREMARK ACCEPTABLE USE AGREEMENT。
- [Wasm Coremark](https://github.com/wasm3/wasm-coremark)，上游未提供许可证文件；其封装的 CoreMark 内容受 COREMARK ACCEPTABLE USE AGREEMENT 约束。
