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
pub(crate) use opcode::*;

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
pub(crate) use ir_reader::*;

mod engine;
pub use engine::*;

#[cfg(any(test, kani))]
pub mod test_support {
    use crate::alloc::{AllocError, Allocator};
    use core::sync::atomic::{AtomicIsize, Ordering};
    extern crate std;
    use std::alloc::Layout;

    /// System allocator for tests
    /// Wraps std::alloc and tracks allocation statistics
    #[derive(Clone, Copy)]
    pub struct RustSystemAllocator;

    /// Live bytes handed out by [`RustSystemAllocator`], as a running total.
    static TOTAL_ALLOCATED: AtomicIsize = AtomicIsize::new(0);

    impl RustSystemAllocator {
        /// Live bytes currently handed out by this allocator.
        pub fn total_allocated(&self) -> isize {
            TOTAL_ALLOCATED.load(Ordering::Relaxed)
        }
    }

    unsafe impl Allocator for RustSystemAllocator {
        unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
            if layout.size() == 0 {
                Ok(core::ptr::null_mut())
            } else {
                let ptr = unsafe { std::alloc::alloc(layout) };
                if ptr.is_null() {
                    Err(AllocError::AllocationFailed)
                } else {
                    TOTAL_ALLOCATED.fetch_add(layout.size() as isize, Ordering::Relaxed);
                    Ok(ptr)
                }
            }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            if ptr.is_null() {
                return;
            }

            unsafe {
                std::alloc::dealloc(ptr, layout);
            }
            TOTAL_ALLOCATED.fetch_sub(layout.size() as isize, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
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
}
