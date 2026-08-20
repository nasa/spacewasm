//! Integration test for the one SpaceWasm-specific validation rejection that
//! cannot be expressed as a `.wast` file.
//!
//! `CodeBuilder::new` rejects a `max_code_pages` that overflows the 24-bit
//! code-page index (returning an allocation error rather than panicking). This
//! is a *compiler option* set when the store's code builder is constructed, not
//! a property of any module's bytes, so no `(module binary ...)` assertion can
//! reach it — unlike the `StackTooLarge` and `TableRefNotUnique` rejections,
//! which are covered by binary modules in `tests/regression/*.wast`.

use core::alloc::Layout;

use spacewasm::{
    AllocError, Allocator, CodeBuilder, CompilerOptions, MemoryStatistics, global_allocator,
};

extern crate std;

// `CodeBuilder::new` allocates its code pages through the global allocator on
// the success path, so this test binary must provide the `__spacewasm_*`
// symbols. Back them with the system heap.
struct SystemAllocator;

unsafe impl Allocator for SystemAllocator {
    unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
        let ptr = unsafe { std::alloc::alloc(layout) };
        if ptr.is_null() {
            Err(AllocError::AllocationFailed)
        } else {
            Ok(ptr)
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { std::alloc::dealloc(ptr, layout) }
    }

    fn memory_statistics(&self) -> MemoryStatistics {
        MemoryStatistics {
            total_bytes: 0,
            pad_bytes: 0,
        }
    }
}

global_allocator!(SystemAllocator, SystemAllocator);

#[test]
fn code_builder_rejects_oversized_max_code_pages() {
    // `1 << 24` is the first value that does not fit the 24-bit code-page index;
    // `CodeBuilder::new` must return an allocation error rather than panic.
    let opts = CompilerOptions {
        allow_memory_grow: false,
        max_backpatch_iterations: 0,
        max_code_pages: 1 << 24,
    };
    assert!(matches!(
        CodeBuilder::new(opts),
        Err(AllocError::AllocationFailed)
    ));

    // One below the limit still builds successfully, pinning the boundary.
    let opts_ok = CompilerOptions {
        allow_memory_grow: false,
        max_backpatch_iterations: 0,
        max_code_pages: (1 << 24) - 1,
    };
    assert!(CodeBuilder::new(opts_ok).is_ok());
}
