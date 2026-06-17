use spacewasm::{AllocError, Allocator, MemoryStatistics, WasmMemoryAllocator};
use std::alloc::Layout;
use std::ptr::NonNull;

mod file;
pub use file::*;
mod debug;
pub use debug::*;
mod trace;
pub use trace::*;

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

impl WasmMemoryAllocator for RustSystemAllocator {
    fn allocate(&self, layout: Layout) -> Result<NonNull<u8>, AllocError> {
        unsafe { Ok(NonNull::new(std::alloc::alloc(layout)).ok_or(AllocError::AllocationFailed)?) }
    }

    fn reallocate(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        layout: Layout,
    ) -> Result<NonNull<u8>, AllocError> {
        unsafe {
            Ok(
                NonNull::new(std::alloc::realloc(ptr.as_ptr(), old_layout, layout.size()))
                    .ok_or(AllocError::AllocationFailed)?,
            )
        }
    }

    fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        unsafe { std::alloc::dealloc(ptr.as_ptr(), layout) }
    }
}
