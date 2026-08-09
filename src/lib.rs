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
pub(crate) use opcode::{DROP, SELECT, RETURN, IF, BR, BR_IF, BR_TABLE, CALL, CALL_HOST, CALL_EXTERN, CALL_INDIRECT, LOCAL_GET, LOCAL_SET, LOCAL_TEE, GLOBAL_GET, GLOBAL_GET_EXTERN, GLOBAL_GET_HOST, GLOBAL_SET, GLOBAL_SET_EXTERN, GLOBAL_SET_HOST, UNREACHABLE, I32_LOAD, I64_LOAD, F32_LOAD, F64_LOAD, I32_LOAD8_S, I32_LOAD8_U, I32_LOAD16_S, I32_LOAD16_U, I64_LOAD8_S, I64_LOAD8_U, I64_LOAD16_S, I64_LOAD16_U, I64_LOAD32_S, I64_LOAD32_U, I32_STORE, I64_STORE, F32_STORE, F64_STORE, I32_STORE8, I32_STORE16, I64_STORE8, I64_STORE16, I64_STORE32, MEMORY_SIZE, MEMORY_GROW, I32_CONST, I64_CONST, F32_CONST, F64_CONST, I32_EQZ, I32_EQ, I32_NE, I32_LT_S, I32_LT_U, I32_GT_S, I32_GT_U, I32_LE_S, I32_LE_U, I32_GE_S, I32_GE_U, I64_EQZ, I64_EQ, I64_NE, I64_LT_S, I64_LT_U, I64_GT_S, I64_GT_U, I64_LE_S, I64_LE_U, I64_GE_S, I64_GE_U, F32_EQ, F32_NE, F32_LT, F32_GT, F32_LE, F32_GE, F64_EQ, F64_NE, F64_LT, F64_GT, F64_LE, F64_GE, I32_CLZ, I32_CTZ, I32_POPCNT, I32_ADD, I32_SUB, I32_MUL, I32_DIV_S, I32_DIV_U, I32_REM_S, I32_REM_U, I32_AND, I32_OR, I32_XOR, I32_SHL, I32_SHR_S, I32_SHR_U, I32_ROTL, I32_ROTR, I64_CLZ, I64_CTZ, I64_POPCNT, I64_ADD, I64_SUB, I64_MUL, I64_DIV_S, I64_DIV_U, I64_REM_S, I64_REM_U, I64_AND, I64_OR, I64_XOR, I64_SHL, I64_SHR_S, I64_SHR_U, I64_ROTL, I64_ROTR, F32_ABS, F32_NEG, F32_CEIL, F32_FLOOR, F32_TRUNC, F32_NEAREST, F32_SQRT, F32_ADD, F32_SUB, F32_MUL, F32_DIV, F32_MIN, F32_MAX, F32_COPYSIGN, F64_ABS, F64_NEG, F64_CEIL, F64_FLOOR, F64_TRUNC, F64_NEAREST, F64_SQRT, F64_ADD, F64_SUB, F64_MUL, F64_DIV, F64_MIN, F64_MAX, F64_COPYSIGN, I32_WRAP_I64, I32_TRUNC_F32_S, I32_TRUNC_F32_U, I32_TRUNC_F64_S, I32_TRUNC_F64_U, I64_EXTEND_I32_S, I64_EXTEND_I32_U, I64_TRUNC_F32_S, I64_TRUNC_F32_U, I64_TRUNC_F64_S, I64_TRUNC_F64_U, F32_CONVERT_I32_S, F32_CONVERT_I32_U, F32_CONVERT_I64_S, F32_CONVERT_I64_U, F32_DEMOTE_F64, F64_CONVERT_I32_S, F64_CONVERT_I32_U, F64_CONVERT_I64_S, F64_CONVERT_I64_U, F64_PROMOTE_F32};

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
pub(crate) use ir_reader::IrReader;

mod engine;
pub use engine::*;

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
    /// Wraps `std::alloc` and tracks allocation statistics
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
