<h1 align="center">SpaceWasm</h1>

<p align="center">
<img src="docs/logo.svg" width="150" height="150">
</p>

<p align="center">
  <a href="https://github.com/nasa/spacewasm/actions/workflows/ci.yml"><img src="https://github.com/nasa/spacewasm/actions/workflows/ci.yml/badge.svg" /></a>
  <a href="https://codecov.io/gh/nasa/spacewasm"><img src="https://codecov.io/gh/nasa/spacewasm/graph/badge.svg?token=3EddiLtM36"/></a>
  <a href="#license"><img src="https://img.shields.io/badge/license-Apache%202.0-blue" alt="license" /></a>
</p>

SpaceWasm is an implementation of the [WebAssembly](https://webassembly.org/)
specification meant to interpret Wasm binary on-board spacecraft. It is developed at
[NASA JPL Robotics](https://robotics.jpl.nasa.gov/who-we-are/groups/358T/).

SpaceWasm is the interpreter behind [F´](https://github.com/nasa/fprime)'s
[`Svc::WasmSequencer`](https://fprime.jpl.nasa.gov/latest/Svc/WasmSequencer/docs/sdd/), released in
[F´ v4.4.0](https://github.com/nasa/fprime/releases/tag/v4.4.0). F´ is JPL's open-source flight-software framework and
[flew on the Ingenuity Mars Helicopter](https://www.jpl.nasa.gov/news/meet-the-open-source-software-powering-nasas-ingenuity-mars-helicopter/).

## Rationale

1. **Command sequencing**: High-level spacecraft activities are typically encoded outside of the embedded flight-software in command sequences.
These activities can include anything from driving the Mars rover and operating its arm, to checking temperature ranges are nominal.
Curiosity has executed [more than 76,000 unique sequences](https://www-robotics.jpl.nasa.gov/media/documents/IEEE25-sequence-traceability-v3.pdf) on Mars.
Historically, the form and capability of sequences has varied from mission to mission, even within JPL.
Several deep-space missions use [VML](https://ntrs.nasa.gov/citations/20150014761).
Curiosity and Perseverance share a couple sequencing languages, and Europa Clipper has another.
JWST uses [JavaScript](https://doi.org/10.1117/12.671403) and Euclid uses [MicroPython](https://essr.esa.int/project/micropython-for-leon-pre-qualified-version).
SpaceWasm implements an industry standard, providing consolidation.

2. **Sandboxing**: The cost and time of flight-software development is high due to its constrained requirements and scope. Validating a new flight-software capability
often involves validating interactions with the entire system. This extends the V&V timeline and increases competition for testbed resources, which makes it hard to
get new autonomy software into flight. WebAssembly gives the opportunity for untrusted or low-trust executables to make their way on-board in a
way that flight-software can restrict access and compute time as well as monitor health and safety.

   On EO-1, the [Autonomous Sciencecraft Experiment](https://doi.org/10.1145/1082473.1082798) encountered two anomalies in flight.
   CPU load hit 100% against 80% on the testbed, which starved a memory-scrub task and triggered a fault response.
   Later, a new algorithm overwrote planner memory and the processor was restarted.
   Running those algorithms as separate Wasm modules prevents both failures.
   Flight software gives each module a [fuel](docs/REQUIREMENTS.md) budget (REQ-5) and regains control when it's spent, so the memory scrub still runs.
   A load or store outside a module's linear memory traps instead of overwriting planner memory.

3. **Portability**: WebAssembly provides well-defined interfaces and sandboxing that make transferring to another platform trivial.
This includes platforms that flight computers typically target like PowerPC (the RAD750 on Perseverance), SPARC, ARM (the Snapdragon on Ingenuity), and RISC-V.

4. **Tooling**: Standardizing to WebAssembly opens doors into a wide community
   of rich tooling and research! Most common languages (C, C++, Rust, Go, and
   others) can target WebAssembly and thus run in SpaceWasm. Despite the name,
   WebAssembly is not tied to the web.

## Overview

This software comes with two major components:

1. Decoder/Validator:

   Reads the Wasm binary in [chunks](#streaming) and decodes it to an executable form. The decoder will use a fixed
   amount of memory and is intended to be measured per-Wasm binary on the ground using
   [`spacewasm validate`](crates/spacewasm_tools#validate), which reports the heap a module's load takes, section by
   section with `--verbose`.

   WebAssembly is validated during the decoding process and does not require another pass of the bytecode.

2. Interpreter:

   A Wasm interpreter that can operate on linear memory and interface with
   hooks from the [embedding](#embedding).

SpaceWasm does not execute direct WebAssembly bytecode. Wasm bytecode is meant to be small and structured in a way to
validate easily. These properties however make it slow to execute in-place. During the decoding process of Wasm
instructions, SpaceWasm converts bytecode into another intermediate representation (IR) which includes properties better
suited for interpretation. Read more about the IR in the [specification](docs/ir.md).

## Requirements

The requirements of SpaceWasm are levied from similar work produced by [DLR](https://github.com/DLR-FT/wasm-interpreter).

See [requirements](./docs/REQUIREMENTS.md).

SpaceWasm uses the Rust 2024 edition; the minimum supported Rust version is **1.87** (matching `rust-version` in
`Cargo.toml`). Older toolchains fail to build with an edition2024 error.

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
allocators. To that end, SpaceWasm provides its own data structures that guarantee these properties. These
data-structures contain the bulk of the `unsafe` Rust semantics; the remainder lives in the performance-critical
operand-stack, linear-memory, and IR access paths and the C FFI.

> [!NOTE]
> These limitations are only enforced on the implementation of the interpreter and _not_ on the Wasm bytecode it is made to interpret.

Wasm linear memory pages are allocated outside of dynamic memory pages.

### A Failed Module Load Invalidates the Engine

> [!IMPORTANT]
> Any error returned while loading a module (`Module::new`, `Engine::push_module`, or any non-OK status from
> `spacewasm_load_module`, including `SPACEWASM_ERR_CAPACITY`) leaves the engine in an unusable state. The embedder
> **must** drop the entire engine (store, `CodeBuilder`, and all loaded modules) and reconstruct it before loading or
> invoking anything else. `Engine::reset` / `spacewasm_reset` only resets execution state and is _not_ a recovery path
> for a failed load.

This is an intentional divergence from the WebAssembly specification, which requires instantiation to be atomic:

- **No rollback of imported state.** Data and element segments are applied directly to their target memory or table
  as they are streamed in, rather than buffered until the whole module validates. Buffering would require a second
  allocation holding every module's segment data, which is too costly on small platforms. When the target is an
  _imported_ memory or table, a rejected module can therefore leave writes in another module's memory or table,
  including table entries that reference the module index the rejected module would have received.
- **No rollback of allocations.** Because deallocation cannot precede allocation (rule 2 above), freeing the partial
  state of a rejected module _poisons_ the page allocator. Every later allocation fails with `HeapPoisoned`
  (`SPACEWASM_ERR_HEAP_POISONED`) until all pages are released, so no further module can be loaded. Code pages
  already consumed by the rejected module are not returned to the `max_code_pages` budget.

Because the engine must be discarded, the leftover writes above are never observable by a correctly behaving
embedder. Reports that rely on continuing to use an engine after a failed load are out of scope for
[security reporting](./SECURITY.md).

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


## Ground Tooling
The [`spacewasm`](crates/spacewasm_tools#readme) command, from `cargo install spacewasm_tools` (or `cargo binstall spacewasm_tools` for a prebuilt binary), is the ground side of the
interpreter: a Cargo linker that holds `wasm-opt` to exactly the proposals a release implements and splits debug info into
a symbol file, a validator that loads a module as the flight interpreter would, and a resolver from the PCs a backtrace
reports to source lines.

It also runs arbitrary modules that adhere to the WASI 0.1 (`wasip1`) spec in a sandboxed environment, with flags to
mount host directories and set environment variables:

```bash
# compile example from crates/spacewasm_tools/tests/wasi/
$ clang --target=wasm32-wasip1 -mcpu=mvp hello_universe.c -o hello_universe.wasm

# rewrite what this release does not implement
$ spacewasm lower hello_universe.wasm

$ spacewasm run hello_universe.wasm
hello universe!
```

`spacewasm trace` runs a module's exports and prints the last instructions each executed, for debugging the interpreter
itself; `make trace` feeds it a fuzzer seed. See the [`spacewasm_tools` README](crates/spacewasm_tools/README.md) for every
command.

## Interpreter Limitations

This Wasm interpreter imposes additional constraints beyond the WebAssembly specification to support
resource-constrained spacecraft environments.

See our [IR SPEC](./docs/ir.md#limitations) for the full list of limitations.

These constraints enable deterministic memory usage and efficient execution in resource-constrained environments while
maintaining compatibility with most standard WebAssembly modules.

### Limits for Wasm Module Producers

Because SpaceWasm compiles bytecode into a fixed-width IR that is typically larger than the original bytecode, the
practical ceiling on raw module size is bounded by the IR code-page limit in the table below (~8 GiB of IR). This is
far larger than any module expected on flight hardware; the binding constraint in practice is the peak memory
configured for the [streaming](#streaming) decoder, which is measured per-module on the ground with
[`spacewasm validate`](crates/spacewasm_tools#validate): it loads the module against the flight configuration
(`--max-code-pages`, `--decoder-limits`, `--heap-page-size`, `--heap-pages`) and reports the IR it compiles to and the
global heap pages its load takes.

Here are a couple of limitations that may be relevant to developers of Wasm modules.

| Limit               | Value               | Notes                                                                                                                                                                                                                                                                     |
| ------------------- | ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Wasm page size      | 64 KiB / 1 B        | [Custom-Page-Sizes proposal](https://github.com/WebAssembly/custom-page-sizes) is supported                                                                                                                                                                               |
| Linear memory pages | 4 GiB               | Per the Wasm 1.0 spec. A module declaring more (or a `max` above this) is rejected. Note that the embedding will definitely limit this but it is dependent on how the interpreter is deployed.                                                                            |
| IR Code             | 8 GiB               | Compiled IR, not raw bytecode. This limit is across all modules in the store. The IR / Bytecode ratio is printed by [`example`](crates/example) as the "compilation ratio". It is difficult to estimate this upfront because it varies on the types of instructions used. |
| Function parameters | 255 32-bit words    | Per function.                                                                                                                                                                                                                                                             |
| Local variables     | 65,535 32-bit words | Per function.                                                                                                                                                                                                                                                             |
| Import name length  | 32 bytes            | Applies to each import's module name and field name; a longer name is rejected at decode time.                                                                                                                                                                            |
| Custom section name | 32 bytes            | The name of each custom section (e.g. the `name` section); a longer name is rejected at decode time.                                                                                                                                                                      |

## Benchmarking

SpaceWasm is tested against the Coremark benchmark to trace performance regression.
See [coremark](crates/example/benches)
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
These tests validate the integrity of the Wasm interpreter against
the specification.

### Fuzzing

SpaceWasm includes a comprehensive fuzzing infrastructure using libfuzzer
and [wasm-smith](https://github.com/bytecodealliance/wasm-tools/tree/main/crates/wasm-smith).

```bash
# Run fuzzer
make fuzz

# Differentially test the validator against wasmi (SpaceWasm vs a reference)
make fuzz-validate-differential

# Analyze crashes with execution traces
make trace CRASH=fuzz/artifacts/no_traps/crash-xxx
```

## Feature Support Matrix

Below is a table of the implemented and planned WebAssembly proposals with links to their tracking issue / implementation pull-request.

| Feature                                                                                                      | Status                                                              |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------- |
| [Wasm MVP](https://www.w3.org/TR/2019/REC-wasm-core-1-20191205/)                                             | All Versions                                                        |
| [Mutable globals](https://github.com/WebAssembly/mutable-global)                                             | All Versions                                                        |
| [Custom page sizes](https://github.com/WebAssembly/custom-page-sizes)                                        | [≥0.2.0](https://github.com/nasa/spacewasm/pull/84)                 |
| [Bulk memory operations](https://github.com/WebAssembly/bulk-memory-operations)                              | [Planned](https://github.com/nasa/spacewasm/issues/54)              |
| [Sign-extension operators](https://github.com/WebAssembly/sign-extension-ops)                                | [≥0.8.0](https://github.com/nasa/spacewasm/pull/216)                |
| [Non-trapping float-to-int conversions](https://github.com/WebAssembly/nontrapping-float-to-int-conversions) | [≥0.8.0](https://github.com/nasa/spacewasm/pull/212)                |
| [SIMD](https://github.com/WebAssembly/simd)                                                                  | [Under Consideration](https://github.com/nasa/spacewasm/issues/187) |
| [Multi-value](https://github.com/WebAssembly/multi-value)                                                    | Under Consideration                                                 |
| [Multiple memories](https://github.com/WebAssembly/multi-memory)                                             | Under Consideration                                                 |

Currently, all other proposals are not implemented, planned or considered.

## Credits & Acknowledgments

Portions of this project are adapted from the open-source projects:

- [rust-lang/rust](https://github.com/rust-lang/rust), which is dual-licensed under MIT OR Apache License 2.0.
- [DLR-FT/wasm-interpreter](https://github.com/DLR-FT/wasm-interpreter), which is licensed under the Apache License 2.0.
- [Wasmtime](https://github.com/bytecodealliance/wasmtime), which is licensed under the Apache License 2.0 with LLVM-exception.
- [WABT](https://github.com/webassembly/wabt), which is licensed under the Apache License 2.0.
- [wasmedge-spectest](https://github.com/WasmEdge/wasmedge-spectest), which is licensed under MIT.
- [WebAssembly Testsuite](https://github.com/WebAssembly/testsuite), which is licensed under the Apache License 2.0.
- [Coremark](https://github.com/eembc/coremark), which is licensed under the COREMARK ACCEPTABLE USE AGREEMENT.
- [Wasm Coremark](https://github.com/wasm3/wasm-coremark), which provides no upstream license file; the wrapped CoreMark payload is governed by the COREMARK ACCEPTABLE USE AGREEMENT.
