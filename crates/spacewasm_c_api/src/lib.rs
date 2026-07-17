//! # spacewasm_ffi
//!
//! C-ABI support layer for the [`spacewasm`] interpreter: opaque handles, the
//! self-referential `Store`/`InterpreterState` borrow, the host-function
//! trampoline, value marshalling, a streaming Wasm input bridge, error-code
//! mapping, and — in [`capi`] — the concrete `spacewasm_*` `extern "C"` entry
//! points.
//!
//! The entry points' const-generic capacities come from [`config`] (chosen at
//! build time via `SPACEWASM_*` environment variables). The global heap
//! allocator is the one thing C cannot supply, so it is left to an integrator
//! crate as a linker symbol (via [`spacewasm::global_allocator!`]). The guest
//! linear-memory allocator, in contrast, is constructed at runtime from C
//! callbacks via [`alloc`] and passed in per module load.
#![no_std]
// The public C-ABI surface intentionally uses C naming conventions
// (`spacewasm_value_t`, `SPACEWASM_OK`, …) so the generated header reads naturally.
#![allow(non_camel_case_types)]

pub mod abi;
pub mod alloc;
pub mod capi;
pub mod config;
pub mod engine;
pub mod host;
pub mod status;
pub mod stream;
pub mod value;

// Re-exports for C callers and downstream Rust consumers.
pub use alloc::{
    SpacewasmAllocator, spacewasm_alloc_fn_t, spacewasm_dealloc_fn_t, spacewasm_realloc_fn_t,
};
pub use engine::{
    Builder, SpacewasmCaller, SpacewasmStore, spacewasm_host_fn_t, spacewasm_hostcall_result_t,
};
pub use status::{spacewasm_run_status_t, spacewasm_status_t, spacewasm_trap_t};
pub use stream::{spacewasm_read_fn_t, spacewasm_read_result_t};
pub use value::{spacewasm_valtype_t, spacewasm_value_t};

/// FFI-safe copy of [`spacewasm::MemoryStatistics`] (already `#[repr(C)]`).
pub use spacewasm::MemoryStatistics as spacewasm_memory_statistics_t;

/// Global allocator statistics. Independent of the interpreter configuration,
/// so it takes no const-generic parameters.
#[unsafe(no_mangle)]
pub extern "C" fn spacewasm_memory_statistics() -> spacewasm_memory_statistics_t {
    spacewasm::Allocator::memory_statistics(&spacewasm::GlobalAllocator)
}
