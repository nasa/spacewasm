Copyright 2026, by the California Institute of Technology. ALL RIGHTS RESERVED. United States Government Sponsorship
acknowledged. Any commercial use must be negotiated with the Office of Technology Transfer at the California Institute
of Technology.

This software may be subject to U.S. export control laws. By accepting this software, the user agrees to comply with all
applicable U.S. export laws and regulations. User has the responsibility to obtain export licenses, or other export
authority as may be required before exporting such information to foreign countries or providing access to foreign
persons.

# SpaceWASM

SpaceWASM is an implementation of the [WASM 1.0](https://webassembly.github.io/spec/versions/core/WebAssembly-1.0.pdf)
specification meant to interpret WASM binary on-board spacecraft. This software comes with two major components:

1. Decoder:

   Reads the WASM binary in [chunks](#streaming) and decodes it to an executable form. The decoder will use a fixed
   amount of memory and can be measured per-WASM binary using the `spacewasm-check` executable on the ground.

2. Interpreter:

   A WASM interpreter that can operate on linear memory and interface with
   hooks from the [embedding](#embedding).

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

SpaceWASM supports decoding and compiling WASM binary in a single pass via a streaming mechanism. Chunks of the WASM
binary may be provided to the interpreter as they are read/requested from the filesystem. The stream must provide chunks
synchronously.

## Execution

## Testing

> In development

## Proposals

> In development
