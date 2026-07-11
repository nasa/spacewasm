# SpaceWasm

[![CI](https://github.com/nasa/spacewasm/actions/workflows/ci.yml/badge.svg)](https://github.com/nasa/spacewasm/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/nasa/spacewasm/branch/main/graph/badge.svg)](https://codecov.io/gh/nasa/spacewasm)

SpaceWasm is an implementation of the [Wasm 1.0](https://webassembly.github.io/spec/versions/core/WebAssembly-1.0.pdf)
specification meant to interpret Wasm binary on-board spacecraft. This software comes with two major components:

1. Decoder/Validator:

   Reads the Wasm binary in [chunks](#streaming) and decodes it to an executable form. The decoder will use a fixed
   amount of memory and can be measured per-Wasm binary using the `spacewasm-check` executable on the ground.

   WebAssembly is validated during the decoding process and does not require another pass of the bytecode.

2. Interpreter:

   A Wasm interpreter that can operate on linear memory and interface with
   hooks from the [embedding](#embedding).

SpaceWasm does not execute direct WebAssembly bytecode. Wasm bytecode is meant to be small and structured in a way to
validate easily. These properties however make it slow to execute in-place. During the decoding process of Wasm
instructions, SpaceWasm converts bytecode into another intermediate representation (IR) which includes properties better
suited for interpretation. Read more about the IR in the [specification](src/SPEC.md).

## Requirements

The requirements of SpaceWasm are levied from similar work produced by [DLR](https://github.com/DLR-FT/wasm-interpreter).

See [requirements](./REQUIREMENTS.md).

## Embedding

Embedding the interpreter refers to instantiating it and providing implementations for the functions that are imported
into the module. Typically, the set of functions imported by the module are fixed and should be specified at compile
time both for the Wasm module and the embedder.

## Dynamic Allocation

SpaceWasm has a unique dynamic memory allocation model. All of its design choices stem from requirements levied by common
flight-software standards. Dynamic allocation follows the following rules:

1. All allocations occur over a discrete number of fixed size blocks called _pages_. These pages are distinct from Wasm's linear memory pages.
2. Deallocation cannot precede allocation.
3. Sub-regions inside pages cannot grow or shrink, sizes should be fixed ahead of time.
4. Memory usage must be deterministic.
5. Any allocation failures must _not_ result in panic.

The standard Rust [allocation](https://doc.rust-lang.org/alloc/) does not meet these constraints even with custom
allocators. To that end, SpaceWasm provides its own data structures that guarantee these properties. You will find these
data-structures contain the only usage of `unsafe` Rust semantics.

> [!NOTE]
> These limitations are only enforced on the implementation of the interpreter and _not_ on the Wasm bytecode it is made to interpret.

Wasm linear memory pages are allocated outside of dynamic memory pages.

## Streaming

_Peak_ memory usage is often an important constraint on small systems found on spacecraft. Many Wasm interpreters
require the Wasm binary to be given in one linear blob to the interpreter. This is typically fine for systems where the
same regions of memory may be reused for different purposes. Flight software on spacecraft generally assign fixed
portions of memory for certain purposes. Therefore, requiring the entire Wasm binary to fit into a single chunk of
memory is not feasible.

SpaceWasm is highly optimized to reduce peak memory usage and not require deallocation after allocation required for
streaming. To this end, there are certain [constraints](#interpreter-limitations) imposed on the WebAssembly
specification.

SpaceWasm supports decoding and compiling Wasm binary in a single pass via a streaming mechanism. Chunks of the Wasm
binary may be provided to the interpreter as they are read/requested from the filesystem. The stream must provide chunks
synchronously.

## Interpreter Limitations

This Wasm interpreter imposes additional constraints beyond the WebAssembly 1.0 specification to support
resource-constrained spacecraft environments.

See our [IR SPEC](./src/SPEC.md) for the full list of limitations.

These constraints enable deterministic memory usage and efficient execution in resource-constrained environments while
maintaining compatibility with most standard WebAssembly modules.

### Limits for Wasm Module Producers

Because SpaceWasm compiles bytecode into a fixed-width IR that is typically larger than the original bytecode, the
practical ceiling on raw module size is bounded by the IR code-page limit above (~8 GiB of IR). This is far larger than
any module expected on flight hardware; the binding constraint in practice is the peak memory configured for the
[streaming](#streaming) decoder, which is measured per-module on the ground with `spacewasm-check`.

> [!NOTE]
> `spacewasm-check` has not been developed yet. A similar tool can be found in `spacewasm-std`.

Here are a couple of limitations that may be relavent to developers of Wasm modules.

| Limit               | Value                 | Notes                                                                                                                                                                                                                                                                 |
| ------------------- | --------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Wasm page size      | 64 KiB (65,536 bytes) | The standard WebAssembly [linear memory page](https://webassembly.github.io/spec/core/exec/runtime.html#page-size) size. The [custom-page-sizes proposal](https://github.com/WebAssembly/custom-page-sizes) is planned but not yet supported, so this value is fixed. |
| Linear memory pages | 65,536 pages (4 GiB)  | Per the Wasm 1.0 spec. A module declaring more (or a `max` above this) is rejected. Note that the embedding will definitely limit this but it is dependent on how the interpreter is deployed.                                                                        |
| IR Code             | 8GiB                  | Compiled IR, not raw bytecode. This limit is across all modules in the store. The IR / Bytecode ratio is printed in `spacewasm-std` as the "compilation ratio". It is difficult to estimate this upfront because it varies on the types of instructions used.         |
| Function parameters | 255 32-bit words      | Per function.                                                                                                                                                                                                                                                         |
| Local variables     | 65,535 32-bit words   | Per function.                                                                                                                                                                                                                                                         |

## Similar Projects

While SpaceWasm is a ground up implementation, it draws on some other similar projects:

- https://github.com/wasmi-labs/wasmi
- https://github.com/wasm3/wasm3
- https://github.com/DLR-FT/wasm-interpreter

## Benchmarking

SpaceWasm is tested against the Coremark benchmark to trace performance regression.
See [coremark](crates/spacewasm_std/benches)
for more information.

## Testing

### Unit & Integration Tests

```bash
cargo test
```

The unit tests check for regressions on the `unsafe` container abstractions provided by SpaceWasm due to unique `alloc`
usage. There are also simple unit tests that cover all Wasm instructions without needing full WAST execution.

The integration tests are spectests from the Wasm 1.0 MVP suite
which was curated in https://github.com/WasmEdge/wasmedge-spectest.
These tests validate the integriy of the Wasm interpreter against
the specification.

### Fuzzing

SpaceWasm includes a comprehensive fuzzing infrastructure using libfuzzer
and [wasm-smith](https://github.com/bytecodealliance/wasm-tools/tree/main/crates/wasm-smith).

```bash
# Run fuzzer
make fuzz

# Analyze crashes with execution traces
make trace CRASH=fuzz/artifacts/no_traps/crash-xxx
```

## Feature Support Matrix

SpaceWasm currently implements exactly WebAssembly 1.0 (the MVP plus the mutable-globals proposal that was folded into
it). SpaceWasm will always be a subset of the full approved Wasm specification. Below is a table of the implemented and planned .

| Feature                                                                                                      | Status              |
| ------------------------------------------------------------------------------------------------------------ | ------------------- |
| [Wasm MVP](https://www.w3.org/TR/2019/REC-wasm-core-1-20191205/)                                             | Supported           |
| [Mutable globals](https://github.com/WebAssembly/mutable-global)                                             | Supported           |
| [Custom page sizes](https://github.com/WebAssembly/custom-page-sizes)                                        | Planned             |
| [Bulk memory operations](https://github.com/WebAssembly/bulk-memory-operations)                              | Planned             |
| [Sign-extension operators](https://github.com/WebAssembly/sign-extension-ops)                                | Planned             |
| [Non-trapping float-to-int conversions](https://github.com/WebAssembly/nontrapping-float-to-int-conversions) | Planned             |
| [Multi-value](https://github.com/WebAssembly/multi-value)                                                    | Under Consideration |
| [Multiple memories](https://github.com/WebAssembly/multi-memory)                                             | Under Consideration |

Currently, all other proposals are not planned or considered.

## Credits & Acknowledgments

Portions of this project are adapted from the open-source projects:

- [DLR-FT/wasm-interpreter](https://github.com/DLR-FT/wasm-interpreter), which is licensed under the Apache License 2.0.
- [Wasmtime](https://github.com/bytecodealliance/wasmtime), which is licensed under the Apache License 2.0 with LLVM-exception.
- [WABT](https://github.com/webassembly/wabt), which is licensed under the Apache License 2.0.
- [wasmedge-spectest](https://github.com/WasmEdge/wasmedge-spectest), which is licensed under MIT.
- [WebAssembly Testsuite](https://github.com/WebAssembly/testsuite), which is licensed under the Apache License 2.0.
- [Coremark](https://github.com/eembc/coremark), which is licensed under the COREMARK ACCEPTABLE USE AGREEMENT.
- [Wasm Coremark](https://github.com/wasm3/wasm-coremark), which provides no upstream license file; the wrapped CoreMark payload is governed by the COREMARK ACCEPTABLE USE AGREEMENT.
