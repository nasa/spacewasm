use spacewasm::{AllocError, Allocator, MemoryStatistics};
use std::alloc::Layout;

mod file;
pub use file::*;
mod debug;
pub use debug::*;

pub struct RustSystemAllocator;
unsafe impl Allocator for RustSystemAllocator {
    unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
        unsafe { Ok(std::alloc::alloc(layout)) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { std::alloc::dealloc(ptr, layout) }
    }

    fn memory_statistics(&self) -> MemoryStatistics {
        panic!("The page allocator should be tracking it's own memory statistics.")
    }
}
