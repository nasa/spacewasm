# SpaceWASM

[![CI](https://github.com/nasa/spacewasm/actions/workflows/ci.yml/badge.svg)](https://github.com/nasa/spacewasm/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/nasa/spacewasm/branch/main/graph/badge.svg)](https://codecov.io/gh/nasa/spacewasm)

SpaceWASM is an implementation of the [WASM 1.0](https://webassembly.github.io/spec/versions/core/WebAssembly-1.0.pdf)
specification meant to interpret WASM binary on-board spacecraft. This software comes with two major components:

1. Decoder/Validator:

   Reads the WASM binary in [chunks](#streaming) and decodes it to an executable form. The decoder will use a fixed
   amount of memory and can be measured per-WASM binary using the `spacewasm-check` executable on the ground.

   WebAssembly is validated during the decoding process and does not require another pass of the bytecode.

2. Interpreter:

   A WASM interpreter that can operate on linear memory and interface with
   hooks from the [embedding](#embedding).

SpaceWASM does not execute direct WebAssembly bytecode. Wasm bytecode is meant to be small and structured in a way to
validate easily. These properties however make it slow to execute in-place. During the decoding process of Wasm
instructions, SpaceWASM converts bytecode into another intermediate representation (IR) which includes properties better
suited for interpretation. Read more about the IR in the [specification](src/SPEC.md).

## Embedding

Embedding the interpreter refers to instantiating it and providing implementations for the functions that are imported
into the module. Typically, the set of functions imported by the module are fixed and should be specified at compile
time both for the WASM module and the embedder.

## Dynamic Allocation

SpaceWASM has a unique dynamic memory allocation model. All of its design choices stem requirements levied by common
flight-software standards. Dynamic allocation follows the following rules:

1. All allocations occur over a discrete number of fixed size blocks called _pages_.
2. Deallocation cannot precede allocation.
3. Sub-regions inside pages cannot grow or shrink, sizes should be fixed ahead of time.
4. Memory usage must be deterministic.
5. Any allocation failures must _not_ result in panic.

The standard Rust [allocation](https://doc.rust-lang.org/alloc/) does not meet these constraints even with custom
allocators. To that end, SpaceWASM provides its own data structures that guarantee these properties. You will find these
data-structures contain the only usage of `unsafe` Rust semantics.

## Streaming

_Peak_ memory usage is often an important constraint on small systems found on spacecraft. Many WASM interpreters
require the WASM binary to be given in one linear blob to the interpreter. This is typically fine for systems where the
same regions of memory may be reused for different purposes. Flight software on spacecraft generally assign fixed
portions of memory for certain purposes. Therefore, requiring the entire WASM binary to fit into a single chunk of
memory is not feasible.

SpaceWASM is highly optimized to reduce peak memory usage and not require deallocation after allocation required for
streaming. To this end there are certain [constraints](#interpreter-limitations) imposed on the WebAssembly
specification.

SpaceWASM supports decoding and compiling WASM binary in a single pass via a streaming mechanism. Chunks of the WASM
binary may be provided to the interpreter as they are read/requested from the filesystem. The stream must provide chunks
synchronously.

## Interpreter Limitations

This WASM interpreter imposes additional constraints beyond the WebAssembly 1.0 specification to support
resource-constrained spacecraft environments:

### Module & Store Limits

- **Modules in store**: Maximum 256 modules
- **Host modules**: Maximum 256 host modules
- **Function parameters**: Maximum 255 32-bit words
- **Local variables**: Maximum 65,535 32-bit words total per function

### IR Code Pages

- **Code pages**: Configurable via generic parameter `MAX_PAGES`, typically set at module instantiation
- **Page size**: 256 16-bit words (512 bytes)
- **Maximum page address**: 24-bit (16,777,216 pages)
- **Word offset in page**: 8-bit (0-255)

### Control Flow

- **Nesting depth**: Configurable via generic parameter `MAX_CONTROL_FRAMES` (blocks/loops/if-else)
- **Value stack**: Configurable via generic parameter `MAX_STACK_DEPTH`, values per function
- **Label jumps**: 22-bit signed offset (±2,097,151 instructions)
- **Stack truncation depth**: Maximum 255 32-bit words per label jump
- **br_table cases**: Maximum 256 branch targets

### Instruction Encoding

- **8-bit or 16-bit indexes**: 0-65,535
- **8-bit or 32-bit immediate**: 0-254 inline, 255+ extended
- **8-bit or 64-bit immediate**: 0-254 inline, 255+ extended

These constraints enable deterministic memory usage and efficient execution in resource-constrained environments while
maintaining compatibility with most standard WebAssembly modules.

## Benchmarking

SpaceWASM is tested against the Coremark benchmark to trace performance regression.
See [coremark](crates/spacewasm_std/benches)
for more information.

## Testing

### Unit & Integration Tests

```bash
cargo test
```

The unit tests check for regressions on the `unsafe` container abstractions provided by SpaceWASM due to unique `alloc`
usage. There are also simple unit tests that cover all WASM instructions without needing full WAST execution.

The integration tests are spectests from the WASM 1.0 MVP suite
which was curated in https://github.com/WasmEdge/wasmedge-spectest.
These tests validate the integriy of the WASM interpreter against
the specification.

### Fuzzing

SpaceWASM includes a comprehensive fuzzing infrastructure using libfuzzer
and [wasm-smith](https://github.com/bytecodealliance/wasm-tools/tree/main/crates/wasm-smith).

```bash
# Run fuzzer
make fuzz

# Analyze crashes with execution traces
make trace CRASH=fuzz/artifacts/no_traps/crash-xxx
```

## Proposals

Currently SpaceWASM implements exactly WebAssembly 1.0 which is:

- WASM MVP
- Mutable Globals

Additional WASM extensions/proposals could be developed later.

## Copyright

Copyright (c) 2026 California Institute of Technology (“Caltech”). U.S. Government
sponsorship acknowledged.
All rights reserved.
Redistribution and use in source and binary forms, with or without modification, are permitted provided
that the following conditions are met:

* Redistributions of source code must retain the above copyright notice, this list of conditions and
the following disclaimer.
* Redistributions in binary form must reproduce the above copyright notice, this list of conditions
and the following disclaimer in the documentation and/or other materials provided with the
distribution.
* Neither the name of Caltech nor its operating division, the Jet Propulsion Laboratory, nor the
names of its contributors may be used to endorse or promote products derived from this software
without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS
IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO,
THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER OR
CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS;
OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR
OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF
ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
