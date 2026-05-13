#![no_std]

pub mod core;
pub use core::*;

pub mod util;
pub use util::*;

pub mod exec;
pub use exec::*;

#[cfg(test)]
mod tests {
    use crate::{AllocError, Allocator, MemoryStatistics};
    extern crate std;
    use std::alloc::Layout;

    struct RustSystemAllocator;
    unsafe impl Allocator for RustSystemAllocator {
        unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
            unsafe { Ok(std::alloc::alloc(layout)) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { std::alloc::dealloc(ptr, layout) }
        }

        fn memory_statistics(&self) -> MemoryStatistics {
            panic!("The page allocator should be tracking its own memory statistics.")
        }
    }

    static mut ALLOC_IMPL: RustSystemAllocator = RustSystemAllocator;
    #[allow(unused_unsafe)]
    static mut GLOBAL_ALLOCATOR: *mut RustSystemAllocator = unsafe { &raw mut ALLOC_IMPL };
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn __spacewasm_alloc(
        size: usize,
        align: usize,
        err: *mut u32,
    ) -> *mut u8 {
        let Ok(layout) = Layout::from_size_align(size, align) else {
            unsafe {
                *err = AllocError::InvalidLayout.into();
            }
            return core::ptr::null_mut();
        };

        match unsafe { (*GLOBAL_ALLOCATOR).alloc(layout) } {
            Ok(ptr) => ptr,
            Err(alloc_err) => {
                unsafe {
                    *err = alloc_err.into();
                }
                core::ptr::null_mut()
            }
        }
    }
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn __spacewasm_dealloc(ptr: *mut u8, size: usize, align: usize) {
        let layout = Layout::from_size_align(size, align).unwrap();
        unsafe { (*GLOBAL_ALLOCATOR).dealloc(ptr, layout) }
    }
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn __spacewasm_memory_statistics() -> MemoryStatistics {
        unsafe { (*GLOBAL_ALLOCATOR).memory_statistics() }
    }
}
