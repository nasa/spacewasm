<h1 align="center">SpaceWasm</h2>
<p align="center">
  <a href="https://github.com/nasa/spacewasm/actions/workflows/ci.yml"><img src="https://github.com/nasa/spacewasm/actions/workflows/ci.yml/badge.svg" /></a>
  <a href="https://codecov.io/gh/nasa/spacewasm"><img src="https://codecov.io/gh/nasa/spacewasm/branch/main/graph/badge.svg" /></a>
  <a href="#license"><img src="https://img.shields.io/badge/license-Apache%202.0-blue" alt="license" /></a>
</p>

<p align="center">
<?xml version="1.0" encoding="UTF-8" standalone="no"?>
<svg xml:space="preserve" version="1.1" width="12em" viewBox="0 0 1000 1000" id="svg2"
  xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape"
  xmlns:sodipodi="http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd"
  xmlns="http://www.w3.org/2000/svg" xmlns:svg="http://www.w3.org/2000/svg"
  xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:cc="http://creativecommons.org/ns#"
  xmlns:dc="http://purl.org/dc/elements/1.1/"><sodipodi:namedview  id="namedview1"  pagecolor="#ffffff"  bordercolor="#000000"  borderopacity="0.25" /><defs  id="defs2" /> 
<path  d="m 614.37908,0 c 0,1.76471 0,3.52941 0,5.39216 0,63.33333 -51.33986,114.65686 -114.65686,114.65686 -63.33333,0 -114.65686,-51.33987 -114.65686,-114.65686 v 0 c 0,-1.86275 0,-3.62745 0,-5.39216 H 0 V 999.99991 H 1000 V 0 Z"  fill="#654ff0"  id="path1"  style="display:inline;fill:#dc0021;fill-opacity:1;stroke-width:1.63399" /><path  style="font-weight:600;font-size:465.127px;font-family:Sans;-inkscape-font-specification:'Sans, Semi-Bold';fill:#ffffff;stroke-width:1.09692"  d="m 556.45163,725.63156 q 0,49.96481 -42.47009,81.30638 -42.24298,31.11445 -114.91908,31.11445 -42.01587,0 -73.35743,-7.2676 -31.11446,-7.49473 -58.36799,-18.85037 v -81.07927 h 9.53873 q 27.02643,21.57572 60.41201,33.15847 33.61269,11.58275 64.50003,11.58275 7.94895,0 20.89438,-1.36267 12.94543,-1.36268 21.12149,-4.54226 9.99297,-4.08803 16.35212,-10.22007 6.58627,-6.13205 6.58627,-18.16903 0,-11.12853 -9.53873,-19.07747 -9.31163,-8.17606 -27.48065,-12.49121 -19.07748,-4.54225 -40.42608,-8.40317 -21.12149,-4.08803 -39.74474,-10.22008 -42.69721,-13.85388 -61.54757,-37.47361 -18.62325,-23.84684 -18.62325,-59.04933 0,-47.23946 42.24298,-76.99124 42.4701,-29.97889 109.01415,-29.97889 33.38558,0 65.86271,6.58628 32.70424,6.35915 56.55109,16.125 v 77.89969 h -9.31163 q -20.44015,-16.35212 -50.19193,-27.25353 -29.52466,-11.12853 -60.412,-11.12853 -10.90142,0 -21.80283,1.58979 -10.6743,1.36268 -20.66727,5.45071 -8.8574,3.40669 -15.21655,10.44719 -6.35916,6.81338 -6.35916,15.67078 0,13.39966 10.22007,20.66727 10.22008,7.04049 38.60918,12.94542 18.62325,3.86092 35.65671,7.49473 17.26057,3.6338 37.01939,9.99296 38.83628,12.71832 57.23242,34.74826 18.62325,21.80283 18.62325,56.7782 z"  id="text1"  transform="scale(0.90597878,1.1037786)"  aria-label="S" /><path  style="font-weight:600;font-size:449.516px;font-family:Sans;-inkscape-font-specification:'Sans, Semi-Bold';font-variation-settings:'wght' 600;fill:#ffffff;stroke-width:1.0601"  d="m 1099.9271,466.6669 -88.0155,326.82095 H 918.62821 L 860.02431,580.58233 802.95686,793.48785 H 709.67351 L 621.65793,466.6669 h 88.01558 l 50.26326,224.97749 60.14032,-224.97749 h 84.06476 l 57.28695,224.97749 52.6777,-224.97749 z"  id="text2"  transform="scale(0.86092051,1.1615474)"  aria-label="W" /></svg>
</p>

SpaceWasm is an implementation of the [Wasm 1.0](https://webassembly.github.io/spec/versions/core/WebAssembly-1.0.pdf)
specification meant to interpret Wasm binary on-board spacecraft. It is developed at [NASA JPL](https://www.jpl.nasa.gov).

## Rationale

1. **Sequencing**: High-level spacecraft activities are typically encoded outside of the embedded flight-software in a command sequence.
These activities can include anything from driving the Mars rover and operating its arm, to checking temperature ranges are nominal.
Historically, the form and capability of sequences has varied from mission to mission, resulting in assorted/fragmented implementations.
SpaceWasm implements an industry standard, providing consolidation.

2. **Sandboxing**: The cost and time of flight-software development is high due to its constrained requirements and scope. Validating a new flight-software capability
often involves validating interactions with the entire system. This extends the V&V timeline and increases competition for testbed resources, which makes it hard to
get new autonomy software into flight. WebAssembly gives the opportunity for untrusted or low-trust executables to make their way on-board in a
way that flight-software can restrict access and compute time as well as monitor health and safety.

3. **Portability**: WebAssembly provides well-defined interfaces and sandboxing that make transferring to another platform trivial.

4. **Tooling**: Standardizing to WebAssembly opens doors into a wide community of rich tooling and research!

## Overview

This software comes with two major components:

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


## WASI 0.1 Support
The [`spacewasi`](crates/spacewasi#readme) crate provides a binary which can run arbitrary WASM modules that adhere to the WASI 0.1 (`wasip1`) spec in a sandboxed environment. Command line flags are available to mount host directories and environment variables:

```bash
# compile example from crates/spacewasi/tests/wasm/
$ clang --target=wasm32-wasip1 -mcpu=mvp hello_universe.c -o hello_universe.wasm

# convert module to MVP compatible file
$ crates/spacewasi/scripts/wasm2mvp.sh hello_universe.wasm

$ spacewasi hello_universe.wasm
hello universe!
```

For more information about this command and basic WASI compilation, see [`spacewasi/README.md`](crates/spacewasi/README.md).

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

Here are a couple of limitations that may be relevant to developers of Wasm modules.

| Limit               | Value               | Notes                                                                                                                                                                                                                                                         |
| ------------------- | ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Wasm page size      | 64 KiB / 1 B        | [Custom-Page-Sizes proposal](https://github.com/WebAssembly/custom-page-sizes) is supported                                                                                                                                                                   |
| Linear memory pages | 4 GiB               | Per the Wasm 1.0 spec. A module declaring more (or a `max` above this) is rejected. Note that the embedding will definitely limit this but it is dependent on how the interpreter is deployed.                                                                |
| IR Code             | 8 GiB               | Compiled IR, not raw bytecode. This limit is across all modules in the store. The IR / Bytecode ratio is printed in `spacewasm-std` as the "compilation ratio". It is difficult to estimate this upfront because it varies on the types of instructions used. |
| Function parameters | 255 32-bit words    | Per function.                                                                                                                                                                                                                                                 |
| Local variables     | 65,535 32-bit words | Per function.                                                                                                                                                                                                                                                 |

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
These tests validate the integrity of the Wasm interpreter against
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

Below is a table of the implemented and planned WebAssembly proposals with links to their tracking issue / implementation pull-request.

| Feature                                                                                                      | Status                                                 |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------ |
| [Wasm MVP](https://www.w3.org/TR/2019/REC-wasm-core-1-20191205/)                                             | All Versions                                           |
| [Mutable globals](https://github.com/WebAssembly/mutable-global)                                             | All Versions                                           |
| [Custom page sizes](https://github.com/WebAssembly/custom-page-sizes)                                        | [≥0.2.0](https://github.com/nasa/spacewasm/pull/84)    |
| [Bulk memory operations](https://github.com/WebAssembly/bulk-memory-operations)                              | [Planned](https://github.com/nasa/spacewasm/issues/54) |
| [Sign-extension operators](https://github.com/WebAssembly/sign-extension-ops)                                | [Planned](https://github.com/nasa/spacewasm/issues/55) |
| [Non-trapping float-to-int conversions](https://github.com/WebAssembly/nontrapping-float-to-int-conversions) | [Planned](https://github.com/nasa/spacewasm/issues/56) |
| [Multi-value](https://github.com/WebAssembly/multi-value)                                                    | Under Consideration                                    |
| [Multiple memories](https://github.com/WebAssembly/multi-memory)                                             | Under Consideration                                    |

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
