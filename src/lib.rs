#![no_std]

pub mod util;
pub use util::*;

mod visitor;
pub use visitor::*;

mod store;
pub use store::*;

mod reader;
pub use reader::*;

mod host;
pub use host::*;

mod imports;
pub use imports::*;

mod stream;
pub use stream::*;

pub mod error;
pub use error::*;

pub mod module;
pub use module::*;

pub(crate) mod opcode;
pub use opcode::*;

mod types;
pub use types::*;

mod code;
pub use code::*;

mod constant;
pub use constant::*;

mod compiler;
pub use compiler::*;

mod text;
pub use text::*;

mod interpreter;
pub use interpreter::*;

mod memory;
pub use memory::*;

mod stack;
pub use stack::*;

mod ir_reader;
pub use ir_reader::*;

#[derive(Debug, Default, Clone)]
#[repr(C)]
pub struct MemoryStatistics {
    pub total_bytes: i32,
    pub pad_bytes: i32,
}

/// Computes the delta between two different statistic samples
impl core::ops::Sub for MemoryStatistics {
    type Output = MemoryStatistics;

    fn sub(self, rhs: Self) -> Self::Output {
        MemoryStatistics {
            total_bytes: self.total_bytes - rhs.total_bytes,
            pad_bytes: self.pad_bytes - rhs.pad_bytes,
        }
    }
}

impl core::ops::AddAssign for MemoryStatistics {
    fn add_assign(&mut self, rhs: Self) {
        self.total_bytes += rhs.total_bytes;
        self.pad_bytes += rhs.pad_bytes;
    }
}

#[cfg(any(test, kani))]
pub mod test_support {
    use crate::MemoryStatistics;
    use crate::alloc::{AllocError, Allocator};
    extern crate std;
    use std::alloc::Layout;

    /// System allocator for tests
    /// Wraps std::alloc and tracks allocation statistics
    #[derive(Clone, Copy)]
    pub struct RustSystemAllocator;

    // Track allocation statistics
    static mut TOTAL_ALLOCATED: i32 = 0;

    unsafe impl Allocator for RustSystemAllocator {
        unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
            if layout.size() == 0 {
                Ok(core::ptr::null_mut())
            } else {
                let ptr = unsafe { std::alloc::alloc(layout) };
                if !ptr.is_null() {
                    unsafe {
                        TOTAL_ALLOCATED += layout.size() as i32;
                    }
                }
                Ok(ptr)
            }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            if ptr.is_null() {
                return;
            }

            unsafe {
                std::alloc::dealloc(ptr, layout);
                TOTAL_ALLOCATED -= layout.size() as i32;
            }
        }

        fn memory_statistics(&self) -> MemoryStatistics {
            MemoryStatistics {
                total_bytes: unsafe { TOTAL_ALLOCATED },
                pad_bytes: 0,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::MemoryStatistics;
    use crate::alloc::Allocator;
    use crate::test_support::RustSystemAllocator;
    use core::alloc::Layout;

    static mut ALLOC_IMPL: RustSystemAllocator = RustSystemAllocator;
    #[allow(unused_unsafe)]
    static mut GLOBAL_ALLOCATOR: *mut RustSystemAllocator = unsafe { &raw mut ALLOC_IMPL };
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn __spacewasm_alloc(
        size: usize,
        align: usize,
        err: *mut u32,
    ) -> *mut u8 {
        let layout = Layout::from_size_align(size, align).unwrap();

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
