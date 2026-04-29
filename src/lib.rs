#![no_std]

pub mod decode;
pub use decode::*;

pub mod ir;
pub use ir::*;

pub mod util;
pub use util::*;

pub mod common;
pub use common::*;

pub mod exec;
pub use exec::*;

#[cfg(test)]
mod tests {
    use crate::{global_allocator, AllocError, Allocator, MemoryStatistics};
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

    global_allocator!(RustSystemAllocator, RustSystemAllocator {});
}
