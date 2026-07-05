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
