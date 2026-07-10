use crate::StaticAllocator;
use crate::alloc::{AllocError, Allocator, GlobalAllocator};
use crate::util::Vec;
use core::alloc::Layout;
use core::ops::{Deref, DerefMut};
use core::{mem, ptr};

/// A heap-allocated value with a configurable allocator.
/// Similar to [::alloc::boxed::Box] but allows specifying a custom allocator.
pub struct Box<T: ?Sized, A: Allocator = GlobalAllocator> {
    ptr: *mut T,
    alloc: A,
}

impl<A: Allocator, T: core::fmt::Debug> core::fmt::Debug for Box<T, A> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        (**self).fmt(f)
    }
}

impl<T: Clone, A: Allocator + Clone> Clone for Box<T, A> {
    fn clone(&self) -> Self {
        Box::new_in(self.alloc.clone(), (**self).clone()).unwrap()
    }
}

impl<T: ?Sized, A: Allocator> Box<T, A> {
    #[inline]
    fn into_raw_with_allocator(self) -> (*mut T, A) {
        let mut b = mem::ManuallyDrop::new(self);
        // We carefully get the raw pointer out in a way that Miri's aliasing model understands what
        // is happening: using the primitive "deref" of `Box`. In case `A` is *not* `Global`, we
        // want *no* aliasing requirements here!
        // In case `A` *is* `Global`, this does not quite have the right behavior; `into_raw`
        // works around that.
        let ptr = &raw mut **b;
        let alloc = unsafe { ptr::read(&b.alloc) };
        (ptr, alloc)
    }

    #[inline]
    pub fn leak<'a>(b: Self) -> &'a mut T
    where
        A: 'a,
    {
        let (ptr, alloc) = b.into_raw_with_allocator();
        mem::forget(alloc);
        unsafe { &mut *ptr }
    }
}

impl<T: Sized> Box<T, GlobalAllocator> {
    /// Create a new box using the global allocator
    pub fn new(value: T) -> Result<Box<T>, AllocError> {
        Box::new_in(GlobalAllocator, value)
    }
}

impl<'a, T: Sized, const N: usize> Box<T, StaticAllocator<'a, N>> {
    /// Create a new box with static memory
    pub fn new_static(
        alloc: StaticAllocator<'a, N>,
        value: T,
    ) -> Result<Box<T, StaticAllocator<'a, N>>, AllocError> {
        const { assert!(N == size_of::<T>()) }
        Self::new_in(alloc, value)
    }
}

impl<T: Sized, A: Allocator> Box<T, A> {
    /// Create a new box with a custom allocator
    pub fn new_in(alloc: A, value: T) -> Result<Box<T, A>, AllocError> {
        if size_of::<T>() == 0 {
            Ok(Box {
                ptr: core::ptr::NonNull::<T>::dangling().as_ptr(),
                alloc,
            })
        } else {
            let layout = Layout::new::<T>();
            let ptr = unsafe { alloc.alloc(layout)? } as *mut T;

            // Write the value into the allocated memory
            unsafe {
                ptr::write(ptr, value);
            }

            Ok(Box { ptr, alloc })
        }
    }
}

impl<T: ?Sized, A: Allocator> Box<T, A> {
    pub(crate) unsafe fn from_raw(alloc: A, ptr: *mut T) -> Box<T, A> {
        Box { ptr, alloc }
    }

    /// Get a raw pointer to the boxed value
    pub fn as_ptr(&self) -> *const T {
        self.ptr
    }

    /// Get a mutable raw pointer to the boxed value
    pub fn as_mut_ptr(&mut self) -> *mut T {
        self.ptr
    }
}

impl<T: ?Sized, A: Allocator> Deref for Box<T, A> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.ptr }
    }
}

impl<T: ?Sized, A: Allocator> DerefMut for Box<T, A> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.ptr }
    }
}

impl<T: ?Sized, A: Allocator> Drop for Box<T, A> {
    fn drop(&mut self) {
        // Null pointers do not have an allocation, they are usually just for dyn* on ZSTs
        if !self.ptr.is_null() {
            unsafe {
                // SAFETY: Compute the layout before dropping the value.
                // Creating a reference to get metadata is safe even though we're about to drop.
                let layout = Layout::for_value(&*self.ptr);

                // Drop the contained value
                ptr::drop_in_place(self.ptr);

                // Deallocate the memory
                self.alloc.dealloc(self.ptr as *mut u8, layout);
            }
        }
    }
}

impl<T: PartialEq, A: Allocator> PartialEq for Box<T, A> {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl<T: Eq, A: Allocator> Eq for Box<T, A> {}

impl<T: PartialOrd, A: Allocator> PartialOrd for Box<T, A> {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        (**self).partial_cmp(&**other)
    }
}

impl<T: Ord, A: Allocator> Ord for Box<T, A> {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        (**self).cmp(&**other)
    }
}

impl<T, A: Allocator> From<Vec<T, A>> for Box<[T], A> {
    fn from(v: Vec<T, A>) -> Self {
        v.into_boxed_slice()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let b = Box::new(42).unwrap();
        assert_eq!(*b, 42);
    }

    #[test]
    fn test_deref() {
        let b = Box::new(100).unwrap();
        assert_eq!(*b, 100);
    }

    #[test]
    fn test_deref_mut() {
        let mut b = Box::new(10).unwrap();
        *b = 20;
        assert_eq!(*b, 20);
    }
    #[test]
    fn test_clone() {
        let b1 = Box::new(42).unwrap();
        let b2 = b1.clone();
        assert_eq!(*b1, *b2);
    }

    #[test]
    fn test_equality() {
        let b1 = Box::new(42).unwrap();
        let b2 = Box::new(42).unwrap();
        let b3 = Box::new(43).unwrap();

        assert_eq!(b1, b2);
        assert_ne!(b1, b3);
    }

    #[test]
    fn test_ordering() {
        let b1 = Box::new(10).unwrap();
        let b2 = Box::new(20).unwrap();

        assert!(b1 < b2);
        assert!(b2 > b1);
    }

    #[test]
    fn test_drop() {
        use core::sync::atomic::{AtomicBool, Ordering};

        static DROPPED: AtomicBool = AtomicBool::new(false);

        #[allow(unused)]
        struct DropChecker(u32);
        impl Drop for DropChecker {
            fn drop(&mut self) {
                DROPPED.store(true, Ordering::SeqCst);
            }
        }

        {
            let _b = Box::new(DropChecker(42)).unwrap();
        }

        assert!(DROPPED.load(Ordering::SeqCst));
    }

    #[test]
    fn test_box_slice() {
        use crate::Vec;

        let mut v = Vec::new(3).unwrap();
        v.push(1);
        v.push(2);
        v.push(3);

        let b = v.into_boxed_slice();
        assert_eq!(b.len(), 3);
        assert_eq!(&*b, &[1, 2, 3]);
        drop(b);
    }
}

#[cfg(kani)]
mod kani_proofs {
    use super::*;
    use crate::test_support::RustSystemAllocator;

    /// Verify Box allocation, initialization, and dereference operations.
    #[kani::proof]
    fn proof_box_allocation_and_deref() {
        let alloc = RustSystemAllocator;
        let value: u32 = kani::any();

        let boxed = Box::new_in(alloc, value).unwrap();
        assert_eq!(*boxed, value, "dereferenced value should match original");

        let ptr = boxed.as_ptr();
        assert!(!ptr.is_null(), "pointer should not be null for non-ZST");
    }

    /// Verify Box ZST (zero-sized type) handling.
    #[kani::proof]
    fn proof_box_zst_handling() {
        let alloc = RustSystemAllocator;

        let boxed = Box::new_in(alloc, ());
        assert!(boxed.is_ok(), "allocation should succeed for ZST");

        let boxed = boxed.unwrap();

        let ptr = boxed.as_ptr();
        assert!(ptr.is_null(), "pointer should be null for ZST");

        assert_eq!(*boxed, (), "ZST value should be unit");
    }

    /// Verify Box deref_mut operation.
    #[kani::proof]
    fn proof_box_deref_mut() {
        let alloc = RustSystemAllocator;
        let value: u32 = kani::any();

        let mut boxed = Box::new_in(alloc, value).unwrap();
        let new_value: u32 = kani::any();
        *boxed = new_value;

        assert_eq!(
            *boxed, new_value,
            "mutated value should be stored correctly"
        );
    }

    /// Verify Box drop safety.
    #[kani::proof]
    fn proof_box_drop_safety() {
        let alloc = RustSystemAllocator;
        let value: u32 = kani::any();

        {
            let boxed = Box::new_in(alloc, value).unwrap();
            assert_eq!(*boxed, value, "value should be accessible before drop");
        }
    }

    /// Verify Box with layout-checking allocator.
    #[kani::proof]
    fn proof_box_layout_matching() {
        /// Wrapper allocator that verifies layout consistency between alloc and dealloc
        struct LayoutCheckingAllocator<'a> {
            inner: &'a RustSystemAllocator,
        }

        static mut ALLOC_PTR: *mut u8 = core::ptr::null_mut();
        static mut ALLOC_SIZE: usize = 0;
        static mut ALLOC_ALIGN: usize = 0;

        unsafe impl<'a> Allocator for LayoutCheckingAllocator<'a> {
            unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
                let ptr = unsafe { self.inner.alloc(layout)? };

                unsafe {
                    ALLOC_PTR = ptr;
                    ALLOC_SIZE = layout.size();
                    ALLOC_ALIGN = layout.align();
                }

                Ok(ptr)
            }

            unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
                let alloc_ptr = unsafe { core::ptr::read_volatile(&raw const ALLOC_PTR) };
                let alloc_size = unsafe { core::ptr::read_volatile(&raw const ALLOC_SIZE) };
                let alloc_align = unsafe { core::ptr::read_volatile(&raw const ALLOC_ALIGN) };

                assert_eq!(ptr, alloc_ptr, "dealloc pointer must match alloc pointer");
                assert_eq!(
                    layout.size(),
                    alloc_size,
                    "dealloc layout size must match alloc layout size"
                );
                assert_eq!(
                    layout.align(),
                    alloc_align,
                    "dealloc layout align must match alloc layout align"
                );

                unsafe { self.inner.dealloc(ptr, layout) }
            }

            fn memory_statistics(&self) -> crate::MemoryStatistics {
                self.inner.memory_statistics()
            }
        }

        let backing = RustSystemAllocator;
        let alloc = LayoutCheckingAllocator { inner: &backing };
        let value: u32 = kani::any();

        let boxed = Box::new_in(alloc, value);
        kani::assume(boxed.is_ok());

        let boxed = boxed.unwrap();
        assert_eq!(*boxed, value, "value should match");

        // Drop will call dealloc with layout - LayoutCheckingAllocator will verify it matches!
    }

    /// Verify Box leak operation.
    #[kani::proof]
    fn proof_box_leak() {
        let alloc = RustSystemAllocator;
        let value: u32 = kani::any();

        let boxed = Box::new_in(alloc, value);
        assert!(boxed.is_ok(), "allocation should succeed");

        let boxed = boxed.unwrap();
        let leaked: &'static mut u32 = Box::leak(boxed);

        assert_eq!(*leaked, value, "leaked reference should have correct value");

        let new_value: u32 = kani::any();
        *leaked = new_value;
        assert_eq!(*leaked, new_value, "leaked reference should be mutable");
    }

    /// Verify Box equality operations.
    #[kani::proof]
    fn proof_box_equality() {
        let alloc = RustSystemAllocator;
        let value1: u32 = kani::any();
        let value2: u32 = kani::any();

        let box1 = Box::new_in(&alloc, value1);
        let box2 = Box::new_in(&alloc, value1);
        let box3 = Box::new_in(&alloc, value2);

        assert!(
            box1.is_ok() && box2.is_ok() && box3.is_ok(),
            "allocations should succeed"
        );

        let box1 = box1.unwrap();
        let box2 = box2.unwrap();
        let box3 = box3.unwrap();

        if value1 == value2 {
            assert_eq!(box1, box2, "boxes with equal values should be equal");
            assert_eq!(box1, box3, "boxes with equal values should be equal");
        } else {
            assert_eq!(box1, box2, "boxes with same value should be equal");
            assert_ne!(
                box1, box3,
                "boxes with different values should not be equal"
            );
        }
    }

    /// Verify Box ordering operations.
    #[kani::proof]
    fn proof_box_ordering() {
        let alloc = RustSystemAllocator;
        let value1: u32 = kani::any();
        let value2: u32 = kani::any();

        let box1 = Box::new_in(&alloc, value1);
        let box2 = Box::new_in(&alloc, value2);

        assert!(box1.is_ok() && box2.is_ok(), "allocations should succeed");

        let box1 = box1.unwrap();
        let box2 = box2.unwrap();

        if value1 < value2 {
            assert!(box1 < box2, "box ordering should match value ordering");
        } else if value1 > value2 {
            assert!(box1 > box2, "box ordering should match value ordering");
        } else {
            assert_eq!(
                box1.cmp(&box2),
                core::cmp::Ordering::Equal,
                "equal values should compare as equal"
            );
        }
    }
}
