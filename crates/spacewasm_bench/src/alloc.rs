use core::{
    alloc::{GlobalAlloc, Layout},
    ptr::NonNull,
};

use talc::{source::Claim, *};

use spacewasm::{AllocError, Allocator, MemoryStatistics, WasmMemoryAllocator};

#[global_allocator]
static TALC: TalcLock<spinning_top::RawSpinlock, Claim> = TalcLock::new(unsafe {
    static mut INITIAL_HEAP: [u8; min_first_heap_size::<DefaultBinning>() + 1_000_000] =
        [0; min_first_heap_size::<DefaultBinning>() + 1_000_000];

    Claim::array(&raw mut INITIAL_HEAP)
});

pub struct BareMetalAllocator;
unsafe impl Allocator for BareMetalAllocator {
    unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
        unsafe { Ok(TALC.alloc(layout)) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { TALC.dealloc(ptr, layout) }
    }

    fn memory_statistics(&self) -> MemoryStatistics {
        panic!("The page allocator should be tracking it's own memory statistics.")
    }
}

impl WasmMemoryAllocator for BareMetalAllocator {
    fn allocate(&self, layout: Layout) -> Result<NonNull<u8>, AllocError> {
        unsafe { NonNull::new(TALC.alloc(layout)).ok_or(AllocError::AllocationFailed) }
    }

    fn reallocate(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        layout: Layout,
    ) -> Result<NonNull<u8>, AllocError> {
        unsafe {
            NonNull::new(TALC.realloc(ptr.as_ptr(), old_layout, layout.size()))
                .ok_or(AllocError::AllocationFailed)
        }
    }

    fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        unsafe { TALC.dealloc(ptr.as_ptr(), layout) }
    }
}
