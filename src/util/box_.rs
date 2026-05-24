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

impl<T: Sized, A: Allocator> Box<T, A> {
    /// Create a new box with a custom allocator
    pub fn new_in(alloc: A, value: T) -> Result<Box<T, A>, AllocError> {
        if size_of::<T>() == 0 {
            Ok(Box { ptr: ptr::null_mut(), alloc })
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
        // Drop the contained value
        unsafe {
            ptr::drop_in_place(self.ptr);
        }

        // Deallocate the memory
        unsafe {
            let layout = Layout::for_value(&self.ptr);
            self.alloc.dealloc(self.ptr as *mut u8, layout);
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
}
