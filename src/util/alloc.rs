use core::{alloc::Layout, marker::PhantomData};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AllocError {
    /// A generic allocation failure
    AllocationFailed,

    /// Not enough pages could be allocated to accommodate this allocation
    OutOfMemory,

    /// Page was too small to fit this allocation
    PageTooSmall,
}

impl From<u32> for AllocError {
    fn from(value: u32) -> Self {
        match value {
            1 => AllocError::OutOfMemory,
            2 => AllocError::PageTooSmall,
            _ => AllocError::AllocationFailed,
        }
    }
}

impl From<AllocError> for u32 {
    fn from(value: AllocError) -> Self {
        value as u32
    }
}

use crate::MemoryStatistics;

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
        pub unsafe extern "C" fn __spacewasm_alloc(
            size: usize,
            align: usize,
            err: *mut u32,
        ) -> *mut u8 {
            let layout = core::alloc::Layout::from_size_align(size, align).unwrap();
            match unsafe { $crate::Allocator::alloc(&*GLOBAL_ALLOCATOR, layout) } {
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
            let layout = core::alloc::Layout::from_size_align(size, align).unwrap();
            unsafe { $crate::Allocator::dealloc(&*GLOBAL_ALLOCATOR, ptr, layout) }
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn __spacewasm_memory_statistics() -> $crate::MemoryStatistics {
            unsafe { $crate::Allocator::memory_statistics(&*GLOBAL_ALLOCATOR) }
        }
    };
}

/// Our allocator trait. This is very similar to [core::alloc::GlobalAlloc].
/// We are not using that trait since it doesn't return Result<...> it just panics
/// if an allocation fails. An adaptor is automatically implemented
///
/// # Safety
///
/// layout must have non-zero size. Attempting to allocate for a zero-sized layout will
/// result in undefined behavior.
///
/// The implementation must guarentee Ok() results are valid pointers against the requested layout.
pub unsafe trait Allocator {
    /// # Safety
    /// The caller must ensure that the layout has non-zero size.
    unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError>;

    /// # Safety
    /// The caller must ensure that `ptr` was allocated by this allocator with the given `layout`.
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

pub struct StaticAllocator<'a, const N: usize> {
    data: *mut u8,
    _phantom: PhantomData<&'a [u8; N]>,
}

impl<'a, const N: usize> StaticAllocator<'a, N> {
    pub fn new(data: &'a mut [u8; N]) -> Self {
        StaticAllocator {
            data: data.as_mut_ptr(),
            _phantom: PhantomData,
        }
    }

    pub fn new_from_ptr(data: *mut u8) -> Self {
        StaticAllocator {
            data,
            _phantom: PhantomData,
        }
    }
}

unsafe impl<'a, const N: usize> Allocator for StaticAllocator<'a, N> {
    unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
        assert_eq!(layout.size(), N);
        Ok(self.data)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _: Layout) {
        assert_eq!(ptr, self.data)
    }

    fn memory_statistics(&self) -> MemoryStatistics {
        MemoryStatistics {
            total_bytes: 0,
            pad_bytes: 0,
        }
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
        unsafe { __spacewasm_memory_statistics() }
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
}
