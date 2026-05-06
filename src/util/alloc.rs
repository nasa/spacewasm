use core::alloc::{Layout, LayoutError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AllocError {
    /// Not enough pages could be allocated to accommodate this allocation
    OutOfMemory,

    /// Zero sized allocations are undefined and disallowed
    IllegalZeroSize,

    /// Page was too small to fit this allocation
    PageTooSmall,

    /// A LayoutError occurred
    InvalidLayout,

    /// A generic allocation failure
    AllocationFailed,

    /// Stack-based heap allocations only support up 128-bit alignment
    InvalidAlignment,

    /// Stack-based heap allocation surpassed the supported nested allocation count
    StackAllocationTooDeep,

    /// Stack-based heap requires allocation and deallocation to occur in reverse order.
    /// This rule is checked during deallocation. If it is not held, this error will be thrown.
    /// This error is also raised when attempting to free a stack address when there are no more
    /// allocations held by the StackAllocator
    StackDeallocationInvariantViolation,

    /// The allocator returned an unknown error code
    Unknown,
}

impl From<u32> for AllocError {
    fn from(value: u32) -> Self {
        match value {
            0 => AllocError::OutOfMemory,
            1 => AllocError::IllegalZeroSize,
            2 => AllocError::PageTooSmall,
            3 => AllocError::InvalidLayout,
            4 => AllocError::AllocationFailed,
            5 => AllocError::InvalidAlignment,
            6 => AllocError::StackAllocationTooDeep,
            7 => AllocError::StackDeallocationInvariantViolation,
            _ => AllocError::Unknown,
        }
    }
}

impl From<AllocError> for u32 {
    fn from(value: AllocError) -> Self {
        value as u32
    }
}

impl From<LayoutError> for AllocError {
    fn from(_value: LayoutError) -> Self {
        AllocError::InvalidLayout
    }
}

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

unsafe extern "C" {
    /// Allocate a pointer on the heap (or wherever) given a size and alignment.
    /// If allocation could not succeed, write the error code corresponding
    /// to [AllocError] into [err] and return NULL.
    fn __spacewasm_alloc(size: usize, align: usize, err: *mut u32) -> *mut u8;

    /// Deallocate a pointer given it's size and alignment
    fn __spacewasm_dealloc(ptr: *mut u8, size: usize, align: usize);

    /// Get basic information about the allocation statistics
    fn __spacewasm_memory_statistics() -> MemoryStatistics;
}

#[macro_export]
macro_rules! global_allocator {
    ($ty: ty, $val:expr) => {
        static mut ALLOC_IMPL: $ty = $val;

        #[allow(unused_unsafe)]
        static mut GLOBAL_ALLOCATOR: *mut $ty = unsafe { &raw mut ALLOC_IMPL };

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn __spacewasm_alloc(size: usize, align: usize, err: *mut u32) -> *mut u8 {
            let Ok(layout) = core::alloc::Layout::from_size_align(size, align) else {
                unsafe { *err = ::spacewasm::AllocError::InvalidLayout.into(); }
                return core::ptr::null_mut();
            };

            match unsafe { (*GLOBAL_ALLOCATOR).alloc(layout) } {
                Ok(ptr) => ptr,
                Err(alloc_err) => {
                    unsafe { *err = alloc_err.into(); }
                    core::ptr::null_mut()
                }
            }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn __spacewasm_dealloc(ptr: *mut u8, size: usize, align: usize) {
            let layout = core::alloc::Layout::from_size_align(size, align).unwrap();
            unsafe { (*GLOBAL_ALLOCATOR).dealloc(ptr, layout) }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn __spacewasm_memory_statistics() -> ::spacewasm::MemoryStatistics {
            unsafe { (*GLOBAL_ALLOCATOR).memory_statistics() }
        }
    };
}

/// Our allocator trait. This is very similar to [core::alloc::GlobalAlloc].
/// We are not using that trait since it doesn't return Result<...> it just panics
/// if an allocation fails. An adaptor is automatically implemented
pub unsafe trait Allocator {
    unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError>;
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout);
    fn memory_statistics(&self) -> MemoryStatistics;
}

unsafe impl<T: Allocator> Allocator for &T {
    unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
        unsafe { (**self).alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { (**self).dealloc(ptr, layout) }
    }

    fn memory_statistics(&self) -> MemoryStatistics {
        (**self).memory_statistics()
    }
}

#[derive(Clone, Copy)]
pub struct GlobalAllocator;
unsafe impl Allocator for GlobalAllocator {
    unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
        let mut err: u32 = 0;
        let ptr = unsafe { __spacewasm_alloc(layout.size(), layout.align(), &mut err) };

        if ptr.is_null() {
            Err(err.into())
        } else {
            Ok(ptr)
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { __spacewasm_dealloc(ptr, layout.size(), layout.align()) }
    }

    fn memory_statistics(&self) -> MemoryStatistics {
        unsafe { __spacewasm_memory_statistics().into() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_statistics_sub() {
        let s1 = MemoryStatistics {
            total_bytes: 100,
            pad_bytes: 20,
        };
        let s2 = MemoryStatistics {
            total_bytes: 60,
            pad_bytes: 10,
        };
        let diff = s1 - s2;
        assert_eq!(diff.total_bytes, 40);
        assert_eq!(diff.pad_bytes, 10);
    }

    #[test]
    fn test_memory_statistics_add_assign() {
        let mut s1 = MemoryStatistics {
            total_bytes: 100,
            pad_bytes: 20,
        };
        let s2 = MemoryStatistics {
            total_bytes: 50,
            pad_bytes: 5,
        };
        s1 += s2;
        assert_eq!(s1.total_bytes, 150);
        assert_eq!(s1.pad_bytes, 25);
    }

    #[test]
    fn test_alloc_error_from_layout_error() {
        let layout_err = Layout::from_size_align(usize::MAX, 1).unwrap_err();
        let alloc_err: AllocError = layout_err.into();
        assert!(matches!(alloc_err, AllocError::InvalidLayout));
    }
}
