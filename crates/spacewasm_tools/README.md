# spacewasm_tools

Ground tooling for the `spacewasm` interpreter: build modules a given release will
load, check and run them, and turn the PCs it reports back into source lines.

```shell
cargo install spacewasm_tools
```

Or download a prebuilt binary, for Linux (x86_64, arm64), macOS (Apple silicon, Intel)
and Windows (x86_64), from the [releases](https://github.com/nasa/spacewasm/releases)
page, which [`cargo binstall`](https://github.com/cargo-bins/cargo-binstall) does for
you:

```shell
cargo binstall spacewasm_tools
```

|                       |                                                                                                                                                    |
| --------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| `spacewasm link`      | A Cargo linker: `rust-lld`, then `wasm-opt` held to this release's features, then the debug info split into a symbol file and the result validated |
| `spacewasm validate`  | Load modules exactly as the flight interpreter would                                                                                               |
| `spacewasm split`     | The link's split, on a module already linked                                                                                                       |
| `spacewasm backtrace` | Interpreter PCs or module offsets, resolved to source                                                                                              |
| `spacewasm features`  | The proposals this release implements, as `wasm-opt` and `rustc` flags                                                                             |
| `spacewasm run`       | Run a WASI 0.1 (`wasip1`) module                                                                                                                   |
| `spacewasm trace`     | Run every exported function, printing the last instructions each executed                                                                          |
| `spacewasm lower`     | Rewrite a module built for more proposals than this release has into one it loads                                                                  |

Each release of this crate is built against the same release of `spacewasm`, so its
feature set is that interpreter's own: upgrading the tool is what lets a build use a
proposal the interpreter has gained.

## `features`

```console
$ spacewasm features
spacewasm 0.8.0: WebAssembly 1.0, plus
  mutable-globals
  custom-page-sizes
  sign-extension-ops
  nontrapping-float-to-int-conversions

wasm-opt:  --mvp-features --enable-mutable-globals --enable-custom-page-sizes --enable-sign-ext --enable-nontrapping-float-to-int
rustc:     -Ctarget-cpu=mvp -Ctarget-feature=+mutable-globals,+sign-ext,+nontrapping-fptoint,-bulk-memory,-reference-types,-multivalue,-extended-const,-simd128,-tail-call
```

`wasm-opt` starts from MVP and adds back, rather than subtracting from its own
default, so a proposal a newer `wasm-opt` enables by default stays out. `rustc` turns
every proposal it can gate on or off explicitly, for the same reason. `--wasm-opt` and
`--rustc` print one line each, for scripts.

## `link`

Set it as Cargo's linker for the Wasm target. It runs `rust-lld`; for an optimised
link (`-O1` and up, which release profiles get) it then runs `wasm-opt`, writes
`<bin>.sym` beside `<bin>.wasm`, and validates the result.

```toml
# .cargo/config.toml
[target.wasm32v1-none]
linker = "spacewasm"
rustflags = ["-Ctarget-cpu=mvp", "-Ctarget-feature=..."]  # `spacewasm features --rustc`

[env]
SPACEWASM_OPT = "-Os --converge"
```

The profile needs `debug = true` and `strip = false`. Neither changes the uplinked
module: debug info is in custom sections, which the split removes.

| Variable                      |                                                                                                                           |
| ----------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `SPACEWASM_OPT`               | `wasm-opt` optimisation flags. Default `-Os`; `none` skips `wasm-opt`. Feature flags are refused: those are the release's |
| `SPACEWASM_WASM_OPT`          | The `wasm-opt` to run. Default `wasm-opt`                                                                                 |
| `SPACEWASM_WASM_OPT_REQUIRED` | `1` fails the link if `wasm-opt` is missing, instead of warning                                                           |
| `SPACEWASM_LINKER`            | The linker to run first. Default `rust-lld`                                                                               |
| `SPACEWASM_VALIDATE`          | `0` skips validating the result                                                                                           |

The link-time validation fails the build on an instruction this release does not
implement. It knows no deployment's limits or host, so anything else it finds is a
warning; `spacewasm validate` with the deployment's settings is the real check.

## `validate`

```shell
spacewasm validate seq.wasm other.wasm
```

Decodes and instantiates each module through `spacewasm` itself, with the compiler
options given (`--max-code-pages`, `--max-backpatch-iterations`, `--allow-memory-grow`,
`--decoder-limits`). Host imports are stood in for from the module's own import
section, so a missing host function is not caught here. An instruction from an
unsupported proposal is named, with how to keep it out:

```
bulk.wasm: `memory.fill` at byte 0x2c is from the bulk-memory-operations proposal, which this release of spacewasm does not implement. ...
```

A module that loads is then checked against the flight global heap. Everything the
load allocates there is laid out on pages the way the flight `PageAllocator` would
lay it out, given `--heap-page-size` (`GLOBAL_ALLOCATOR_PAGE_SIZE`, default 8192)
and `--heap-pages` (`GLOBAL_ALLOCATOR_MAX_PAGES`, default 16). The module fails if
it needs more pages than that, or if one allocation is bigger than a page. The
engine's own allocations, such as its stack and the store, are not counted.
Neither is guest linear memory, which lives outside the heap.

```console
$ spacewasm validate --verbose hello_universe.wasm
hello_universe.wasm: ok, 25502 bytes, 18860 IR words in 74 code pages, 40656 heap bytes in 5 of 16 8192-byte pages
  section                         size        heap   padding  allocations
  header                             8           6         0            1
  type                              84         668         2           21
  import                           253          28         0            1
  function                          58        1762         2            1
  ...
  code                           22236       38020         0          103
  data                            2665           0         0            0
  total                          25502       40656         4          133
```

`--verbose` charges each allocation to the section being decoded when it was made.
`heap` includes alignment `padding`, but not the space left unused at the end of a
page when the next allocation did not fit.

## `split`

```shell
spacewasm split linked.wasm -o seq.wasm --symbols seq.sym --optimize=-Os
```

`--optimize` runs `wasm-opt` with this release's features and the arguments given;
without it the module is only stripped.

## `backtrace`

```shell
spacewasm backtrace seq.sym --module seq.wasm --pc 0x40,0x12,0x3
```

`--pc` takes a backtrace innermost first, as `spacewasm_backtrace` writes it. Every entry
after the first is a caller's `call` instruction, exactly. The first is the faulting
instruction after a trap, the `call` that paused while a host call has paused, and the
next instruction to run out of fuel.

Resolving a PC replays the load to recover the IR layout, so it needs the compiler
options the flight store had (as for `validate`), and `--base` if other modules were
loaded into the store first. `--offset` takes byte offsets into the uplinked module
instead, and needs none of that.

## `run`

```console
$ spacewasm run --help
Run a WASI 0.1 (`wasip1`) module

Usage: spacewasm run [OPTIONS] <FILE> [ARGS]...

Arguments:
  <FILE>     The module
  [ARGS]...  Arguments for the module

Options:
      --cwd-is-root                  Mount the current working directory as the root directory (/) in the guest
      --argv0 <ARGV0>                Override argv[0]
  -d, --dir <HOST_DIR[::GUEST_DIR]>  Mount a host directory, at the same path or at `GUEST_DIR`. Repeatable
  -e, --env <KEY[=VALUE]>            Set an environment variable; `KEY` alone copies it from this process. Repeatable
      --inherit-env                  Inherit this process's whole environment
      --raw-tty                      Put the terminal in raw mode for the run
      --entry <ENTRY>                The function to run [default: _start]
```

What the `spacewasi` binary was, with the same flags. The WASI 0.1 system calls are
`wasmtime`'s [`wasi-common`](https://crates.io/crates/wasi-common), bound into one
`HostModule`; guest memory is handed to it as `wasi-common`'s `GuestMemory`. The guest's
`proc_exit` status is this command's exit status.

A WASI toolchain targets more proposals than the interpreter implements, so its output
usually needs `lower` first:

```console
$ clang --target=wasm32-wasip1 -mcpu=mvp hello_universe.c -o hello_universe.wasm
$ spacewasm lower hello_universe.wasm
$ spacewasm run hello_universe.wasm
hello universe!
```

Large programs with WASI builds run this way too, CPython among them:

```console
[Python-3.11.0-wasm32-wasi-16] $ spacewasm run --cwd-is-root python3.11.wasm
Python 3.11.0 (tags/v3.11.0-dirty:deaf509, Oct 29 2022, 07:56:14) [Clang 14.0.4 (https://github.com/llvm/llvm-project 29f1039a7285a5c3a9c353d05414 on wasi
>>> import sys
>>> print(sys.platform)
wasi
```

The C programs in [`tests/wasi/`](tests/wasi/) are examples, and the fixtures the tests
run; the committed `.wasm` beside each needs no toolchain to test with. Regenerating
them needs the WASI SDK and `wasm-opt`.

## `lower`

```shell
spacewasm lower input.wasm -o output.wasm
```

What `crates/spacewasi/scripts/wasm2mvp.sh` was, but held to this release: `wasm-opt`'s
lowering passes rewrite the instructions of each proposal the interpreter lacks into
WebAssembly 1.0 ones, and a proposal it implements is left alone. (`wasm2mvp.sh` lowered
sign-extension and saturating truncation too, after the interpreter had both.) A proposal
with no lowering pass, SIMD say, is reported rather than left to fail at load time.

## `trace`

```shell
spacewasm trace module.wasm --limit 50
seed_to_wasm crash-xxx --stdout | spacewasm trace
```

What `spacewasm-trace` was. Runs every exported function, with zero for each argument,
and prints the last `--limit` instructions each executed with `pc`/`sp`/`fp`; on a panic
the trace is printed first. Reads the module from stdin when no file is given. Host
imports are stood in for, and trap if called. `make trace` builds it with
`--features strict-assertions`, so the interpreter's own bounds checks stop at the fault.

## The symbol file

A Wasm module: the module as linked, DWARF intact, then two custom sections.

- `spacewasm.module`: the exact module that flies. `--module` is checked against it.
- `spacewasm.offsets`: a version 3 source map, one generated line, whose segments
  map a byte of the uplinked module to the linked byte it was optimised from.
  Absent when the split moved no code byte.

Addresses in the DWARF are linked addresses, so `llvm-dwarfdump` and
`llvm-symbolizer` read a symbol file directly.

## As a library

`PROPOSALS`, `validate`, `split`, `Symbols`, `ir_map`, `resolve_pcs` and `link` are
what the binary is made of. Build without default features (`default-features =
false`) to leave out `clap` and `run`'s WASI host, which only the binary needs. `validate`, `ir_map` and `link` decode through
`spacewasm`, whose allocator is process-global: the library does not install one, so
a binary using it must, with `spacewasm::global_allocator!`. The library re-exports
`spacewasm`, so the allocator is installed into the same release.

A tool wrapping this crate can be the Cargo linker itself, so its users need no
`spacewasm` installed:

```rust
let args: Vec<OsString> = std::env::args_os().skip(1).collect();
if spacewasm_tools::is_link(&args) {
    return match spacewasm_tools::link(&args) { /* ... */ };
}
```
