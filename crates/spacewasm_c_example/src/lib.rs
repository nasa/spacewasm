//! # spacewasm_c
//!
//! Reference integrator crate producing a C-linkable build of the [`spacewasm`]
//! interpreter: it installs the global heap allocator and re-exports the
//! `spacewasm_*` C entry points from [`spacewasm_ffi::capi`]. The guest
//! linear-memory allocator is no longer a link-time hook — C constructs it at
//! runtime via `spacewasm_allocator_new`. The interpreter capacities (code
//! pages, control frames, stack depth) are chosen when `spacewasm_ffi` compiles,
//! via its `config` module and the `SPACEWASM_*` environment variables. Uses the
//! std-backed system allocator so it runs on a host for testing; a flight build
//! would substitute a deterministic allocator and compile with
//! `panic = "abort"`.

use core::sync::atomic::{AtomicI64, Ordering};
use std::alloc::Layout;

use spacewasm::{AllocError, Allocator, MemoryStatistics};

/// Host-build global allocator: forwards to the system allocator and tracks a
/// running byte total for `spacewasm_memory_statistics`. A flight build would
/// substitute a deterministic allocator here.
struct HostAllocator;

static ALLOCATED: AtomicI64 = AtomicI64::new(0);

unsafe impl Allocator for HostAllocator {
    unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
        if layout.size() == 0 {
            return Ok(core::ptr::null_mut());
        }
        let ptr = unsafe { std::alloc::alloc(layout) };
        if !ptr.is_null() {
            ALLOCATED.fetch_add(layout.size() as i64, Ordering::Relaxed);
        }
        Ok(ptr)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if ptr.is_null() {
            return;
        }
        unsafe { std::alloc::dealloc(ptr, layout) };
        ALLOCATED.fetch_sub(layout.size() as i64, Ordering::Relaxed);
    }

    fn memory_statistics(&self) -> MemoryStatistics {
        MemoryStatistics {
            total_bytes: ALLOCATED.load(Ordering::Relaxed) as i32,
            pad_bytes: 0,
        }
    }
}

// Install the global allocator. This resolves `__spacewasm_alloc`,
// `__spacewasm_dealloc`, and `__spacewasm_memory_statistics`.
spacewasm::global_allocator!(HostAllocator, HostAllocator);

// Re-export the `spacewasm_*` C entry points emitted by `spacewasm_ffi` so this
// crate's `cdylib`/`staticlib` exports them and Rust consumers can name them.
pub use spacewasm_ffi::capi::*;
