//! `spacewasm`: ground tooling for the `spacewasm` interpreter.
//!
//! * `link` — a Cargo linker: `rust-lld`, then `wasm-opt` held to this release's features,
//!   splitting the debug info into a symbol file
//! * `split` — the same split on a module already linked
//! * `validate` — load a module exactly as the flight interpreter would
//! * `backtrace` — interpreter PCs or module offsets, resolved to source
//! * `features` — the proposals this release implements, as `wasm-opt` and `rustc` flags
//! * `run` — execute a WASI 0.1 module, as `spacewasi` did
//! * `trace` — run each export and print its last instructions, as `spacewasm-trace` did
//! * `lower` — rewrite a module into one this release loads, as `wasm2mvp.sh` did
//!
//! `load` is what `run` and `trace` share: the ground interpreter, with real hosts.

mod args;
mod backtrace;
mod features;
mod load;
mod lower;
mod run;
mod split;
mod trace;
mod validate;

use clap::Parser;
use spacewasm::PageAllocator;
use spacewasm_tools::HeapRecorder;
use std::ffi::OsString;
use std::process::ExitCode;

// The decoder's process-global allocator, backed by the system allocator: this runs on the
// ground, where nothing is constrained. Sized for `run`: a page bounds the largest
// single allocation, and a WASI program as large as CPython needs `spacewasi`'s 128 MiB.
// A page is only taken from the system when first needed, so the commands that decode
// small modules pay for one. `validate` measures a load against the flight heap by
// recording what it allocates here.
spacewasm::global_allocator!(
    HeapRecorder<PageAllocator<System, 0x200>>,
    HeapRecorder::new(PageAllocator::new(System, 0x8_000_000))
);

struct System;

// SAFETY: forwards to the Rust global allocator, which upholds the same contract.
unsafe impl spacewasm::Allocator for System {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> Result<*mut u8, spacewasm::AllocError> {
        // SAFETY: the caller guarantees a non-zero size, as `Allocator` requires.
        let ptr = unsafe { std::alloc::alloc(layout) };
        match ptr.is_null() {
            true => Err(spacewasm::AllocError::AllocationFailed),
            false => Ok(ptr),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        // SAFETY: `ptr` came from `alloc` with `layout`.
        unsafe { std::alloc::dealloc(ptr, layout) }
    }
}

/// `Ok(false)`: the command ran and found a problem it has already reported.
type Outcome = Result<bool, Box<dyn std::error::Error>>;

fn main() -> ExitCode {
    // As Cargo's `linker` there is no subcommand to name.
    let argv: Vec<OsString> = std::env::args_os().collect();
    if spacewasm_tools::is_link(&argv[1..]) {
        return exit(link(&argv[1..]));
    }

    let result = match args::Cli::parse_from(argv).command {
        args::Command::Link { args } => link(&args),
        args::Command::Split(args) => split::run(&args),
        args::Command::Validate(args) => validate::run(&args),
        args::Command::Backtrace(args) => backtrace::run(&args),
        args::Command::Features(args) => features::run(&args),
        args::Command::Trace(args) => trace::run(&args),
        args::Command::Lower(args) => lower::run(&args),
        args::Command::Run(args) => match run::run(&args) {
            run::Ran::Exited(code) => return code,
            run::Ran::Outcome(outcome) => outcome,
        },
    };
    exit(result)
}

/// `spacewasm link`: the pipeline and its `SPACEWASM_*` settings are
/// [`spacewasm_tools::link`]'s.
fn link(args: &[OsString]) -> Outcome {
    Ok(spacewasm_tools::link(args)?)
}

fn exit(result: Outcome) -> ExitCode {
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(err) => {
            eprintln!("spacewasm: {err}");
            ExitCode::FAILURE
        }
    }
}
