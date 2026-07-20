
use core::{alloc::{GlobalAlloc, Layout}, ptr::NonNull};

use embedded_alloc::LlffHeap as Heap;
use spacewasm::{AllocError, Allocator, MemoryStatistics, WasmMemoryAllocator};

#[global_allocator]
static HEAP: Heap = Heap::empty();

pub fn init_alloc() {
    unsafe {
        embedded_alloc::init!(HEAP, 1024);
    }
}

pub struct BareMetalAllocator;
unsafe impl Allocator for BareMetalAllocator {
    unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
        unsafe { Ok(HEAP.alloc(layout)) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { HEAP.dealloc(ptr, layout) }
    }

    fn memory_statistics(&self) -> MemoryStatistics {
        panic!("The page allocator should be tracking it's own memory statistics.")
    }
}

impl WasmMemoryAllocator for BareMetalAllocator {
    fn allocate(&self, layout: Layout) -> Result<NonNull<u8>, AllocError> {
        unsafe { NonNull::new(HEAP.alloc(layout)).ok_or(AllocError::AllocationFailed) }
    }

    fn reallocate(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        layout: Layout,
    ) -> Result<NonNull<u8>, AllocError> {
        unsafe {
            NonNull::new(HEAP.realloc(ptr.as_ptr(), old_layout, layout.size()))
                .ok_or(AllocError::AllocationFailed)
        }
    }

    fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        unsafe { HEAP.dealloc(ptr.as_ptr(), layout) }
    }
}
