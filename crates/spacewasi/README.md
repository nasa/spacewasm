# `spacewasi`
The `spacewasi` crate provides a binary that can execute arbitrary WebAssembly modules that adhere to the WASI 0.1 (`wasip1`) spec in a sandboxed environment.

## Usage
`spacewasi` is a standalone binary that exposes its configuration via commandline arguments and flags:
```bash
$ spacewasi --help
Execute WASI-compatible WASM modules with spacewasm

Usage: spacewasi [OPTIONS] <FILE> [ARGS]...

Arguments:
<FILE>     Module filepath
[ARGS]...  Raw arguments passed on to the module

Options:
    --cwd-is-root                 Mount the current working directory as the root directory (/) in WASM
    --argv0 <ARGV0>               Override argv[0] value
-d, --dir <HOST_DIR[::WASM_DIR]>  Mount directories
-e, --env <KEY[=VALUE]>           Set environment variables
    --inherit-env                 Inherit all environment variables
-h, --help                        Print help
-V, --version                     Print version
```

## Compiling WASI Code
To compile SpaceWasm/spacewasi compatible WASM modules, ensure that you have installed the WASI SDK, `llvm`, and `wasm-opt`.

```bash
# compile example from crates/spacewasi/tests/wasm/
$ clang --target=wasm32-wasip1 -mcpu=mvp hello_universe.c -o hello_universe.wasm

# convert module to MVP compatible file
$ crates/spacewasi/scripts/wasm2mvp.sh hello_universe.wasm

$ spacewasi hello_universe.wasm
hello universe!
```

Several example C programs can be found in the [`tests/wasm/`](tests/wasm/) directory to test with.


As SpaceWASM currently supports the MVP version of WebAssembly, it is sometimes necessary to "lower" compiled binaries to this format in order for them to run properly. We provide a script which wraps `wasm-opt` in [`scripts/wasm2mvp.sh`](scripts/wasm2mvp.sh). Several large projects (including CPython) already have WASI compatible builds, and it is simple to lower them to MVP compatible formats. For example, Python 3.11 successfully runs with SpaceWASM:
```python
[Python-3.11.0-wasm32-wasi-16] $ spacewasi --cwd-is-root python3.11.wasm
Python 3.11.0 (tags/v3.11.0-dirty:deaf509, Oct 29 2022, 07:56:14) [Clang 14.0.4 (https://github.com/llvm/llvm-project 29f1039a7285a5c3a9c353d05414 on wasi
Type "help", "copyright", "credits" or "license" for more information.
>>> import sys
>>> print(sys.platform)
wasi
```

## Implementation
WASI 0.1 functions are implemented using the [`wasi-common`](https://crates.io/crates/wasi-common) crate from `wasmtime`. Every function is added into a shared SpaceWASM `HostModule` which can be accessed from the interpreter. Memory is wrapped in `wasi-common`'s `GuestMemory`, and each WASI function writes directly to WASM memory.

## Testing
Integration testing of example WASI WASM modules is done by `cargo test` using compiled C programs in `tests/wasm/`. As the WASI functions themselves are implemented by wasmtime, testing primarily only covers the CLI wrapper for `spacewasi`.

In order to run the tests, you much have `llvm` and the WASI SDK instealled, along with `wasm-opt`.