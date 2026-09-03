// Portions of this file are derived from the Rust project
// (https://github.com/rust-lang/rust), licensed under Apache-2.0. These
// portions have been modified for SpaceWasm.

use core::alloc::Layout;

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

unsafe extern "C" {
    /// Allocate a pointer on the heap (or wherever) given a size and alignment.
    /// If allocation could not succeed, write the error code corresponding
    /// to [AllocError] into `err` and return NULL.
    fn __spacewasm_alloc(size: usize, align: usize, err: *mut u32) -> *mut u8;

    /// Deallocate a pointer given it's size and alignment
    fn __spacewasm_dealloc(ptr: *mut u8, size: usize, align: usize);
}

/// Installs the process-global allocator that backs the `__spacewasm_alloc` /
/// `__spacewasm_dealloc` / `__spacewasm_memory_statistics` FFI symbols.
///
/// # Single-threaded requirement (UB otherwise)
///
/// The generated code stores the allocator instance and a pointer to it in
/// `static mut` globals and dereferences them from the exported `extern "C"`
/// functions **without any synchronization**. This is only sound in a
/// single-threaded environment, which is the model SpaceWasm targets: the
/// interpreter and its allocator run on a single thread.
///
/// Calling any of the generated `__spacewasm_*` functions concurrently from
/// more than one thread (or re-entrantly, e.g. allocating from within an
/// allocator callback that is itself mid-allocation) creates aliasing
/// `&mut`/`&` references to the `static mut` state and is **undefined
/// behavior**. Embedders that need multi-threaded access must provide their own
/// synchronization *around* these entry points.
#[macro_export]
macro_rules! global_allocator {
    ($ty: ty, $val:expr) => {
        // SAFETY: see the macro's doc comment -- these `static mut` globals are
        // sound only under the single-threaded, non-re-entrant usage contract.
        static mut ALLOC_IMPL: $ty = $val;

        #[allow(unused_unsafe)]
        static mut GLOBAL_ALLOCATOR: *mut $ty = unsafe { &raw mut ALLOC_IMPL };

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn __spacewasm_alloc(
            size: usize,
            align: usize,
            err: *mut u32,
        ) -> *mut u8 {
            let layout = match core::alloc::Layout::from_size_align(size, align) {
                Ok(l) => l,
                Err(_) => {
                    unsafe {
                        *err = $crate::AllocError::AllocationFailed.into();
                    }
                    return core::ptr::null_mut();
                }
            };
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
            let Ok(layout) = core::alloc::Layout::from_size_align(size, align) else {
                return;
            };
            unsafe { $crate::Allocator::dealloc(&*GLOBAL_ALLOCATOR, ptr, layout) }
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
}

unsafe impl<T: Allocator> Allocator for &T {
    unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
        unsafe { (**self).alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { (**self).dealloc(ptr, layout) }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alloc_error_from_u32() {
        assert_eq!(AllocError::from(1u32), AllocError::OutOfMemory);
        assert_eq!(AllocError::from(2u32), AllocError::PageTooSmall);
        assert_eq!(AllocError::from(0u32), AllocError::AllocationFailed);
        // Any unrecognized code falls back to the generic failure.
        assert_eq!(AllocError::from(42u32), AllocError::AllocationFailed);
    }

    #[test]
    fn test_alloc_error_into_u32() {
        assert_eq!(u32::from(AllocError::AllocationFailed), 0);
        assert_eq!(u32::from(AllocError::OutOfMemory), 1);
        assert_eq!(u32::from(AllocError::PageTooSmall), 2);
    }

    #[test]
    fn test_alloc_error_round_trip() {
        for err in [
            AllocError::AllocationFailed,
            AllocError::OutOfMemory,
            AllocError::PageTooSmall,
        ] {
            let code: u32 = err.clone().into();
            assert_eq!(AllocError::from(code), err);
        }
    }

    #[test]
    fn test_reference_allocator_delegates() {
        // The blanket `impl Allocator for &T` should forward every call to the
        // underlying allocator. This is the form `global_allocator!` relies on,
        // allocating through `&*GLOBAL_ALLOCATOR`.
        let alloc = GlobalAllocator;
        let by_ref = &alloc;

        let layout = Layout::from_size_align(32, 8).unwrap();
        let ptr = unsafe { Allocator::alloc(&by_ref, layout) }.unwrap();
        assert!(!ptr.is_null());

        unsafe { Allocator::dealloc(&by_ref, ptr, layout) };
    }

    #[test]
    fn test_global_allocator_alloc_dealloc() {
        let alloc = GlobalAllocator;
        let layout = Layout::from_size_align(32, 8).unwrap();
        let ptr = unsafe { alloc.alloc(layout) }.unwrap();
        assert!(!ptr.is_null());
        unsafe { alloc.dealloc(ptr, layout) };
    }

    #[test]
    fn test_global_allocator_alloc_error() {
        // The test-backing `__spacewasm_alloc` returns NULL for a zero-sized
        // allocation, which drives `GlobalAllocator::alloc` down its error path.
        let alloc = GlobalAllocator;
        let layout = Layout::from_size_align(0, 1).unwrap();
        let result = unsafe { alloc.alloc(layout) };
        assert_eq!(result, Err(AllocError::AllocationFailed));
    }
}
