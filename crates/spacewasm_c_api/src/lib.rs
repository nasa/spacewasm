//! # spacewasm_c_api
//!
//! C-ABI support layer for the [`spacewasm`] interpreter
#![no_std]
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

#[cfg(all(feature = "provide-panic-handler", not(test)))]
mod panic;

#[cfg(feature = "provide-global-allocator")]
pub mod global_alloc;

// The suite drives the runtime-registered global allocator, so it depends on
// the `provide-global-allocator` feature (on by default).
#[cfg(all(test, feature = "provide-global-allocator"))]
mod tests;

// Re-exports for C callers and downstream Rust consumers.
pub use alloc::{
    SpacewasmAllocator, spacewasm_alloc_fn_t, spacewasm_dealloc_fn_t, spacewasm_realloc_fn_t,
};
pub use engine::{
    SpacewasmCaller, SpacewasmStore, spacewasm_host_fn_t, spacewasm_hostcall_result_t,
};
#[cfg(feature = "provide-global-allocator")]
pub use global_alloc::{
    spacewasm_global_alloc_fn_t, spacewasm_global_dealloc_fn_t, spacewasm_set_global_allocator,
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
