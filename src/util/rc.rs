use crate::{AllocError, Allocator, GlobalAllocator};
use core::cell::Cell;
use core::hint;
use core::ops::Deref;
use core::panic::{RefUnwindSafe, UnwindSafe};
use core::ptr::NonNull;

struct RcInner<T: ?Sized> {
    count: Cell<usize>,
    value: T,
}

pub struct Rc<T: ?Sized, A: Allocator = GlobalAllocator> {
    ptr: NonNull<RcInner<T>>,
    alloc: A,
}

impl<T: RefUnwindSafe + ?Sized, A: Allocator + UnwindSafe> UnwindSafe for Rc<T, A> {}
impl<T: RefUnwindSafe + ?Sized, A: Allocator + UnwindSafe> RefUnwindSafe for Rc<T, A> {}

impl<T: ?Sized> Deref for Rc<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &unsafe { self.ptr.as_ref() }.value
    }
}

impl<T> Rc<T> {
    pub fn new(value: T) -> Result<Rc<T>, AllocError> {
        unsafe {
            Ok(Self::from_inner(
                crate::Box::leak(crate::Box::new(RcInner {
                    count: Cell::new(1),
                    value,
                })?)
                .into(),
            ))
        }
    }

    #[inline]
    fn is_unique(&self) -> bool {
        self.inner().count() == 1
    }

    #[inline]
    pub fn get_mut(&mut self) -> Option<&mut T> {
        if Rc::is_unique(self) {
            unsafe { Some(Rc::get_mut_unchecked(self)) }
        } else {
            None
        }
    }

    #[inline]
    pub unsafe fn get_mut_unchecked(&mut self) -> &mut T {
        // We are careful to *not* create a reference covering the "count" fields, as
        // this would conflict with accesses to the reference counts (e.g. by `Weak`).
        unsafe { &mut (*self.ptr.as_ptr()).value }
    }
}

impl<T: ?Sized, A: Allocator + Clone> Clone for Rc<T, A> {
    /// Makes a clone of the `Rc` pointer.
    ///
    /// This creates another pointer to the same allocation, increasing the
    /// strong reference count.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::rc::Rc;
    ///
    /// let five = Rc::new(5);
    ///
    /// let _ = Rc::clone(&five);
    /// ```
    #[inline]
    fn clone(&self) -> Self {
        unsafe {
            self.inner().inc();
            Self::from_inner_in(self.ptr, self.alloc.clone())
        }
    }
}

impl<T: ?Sized> Rc<T> {
    #[inline]
    unsafe fn from_inner(ptr: NonNull<RcInner<T>>) -> Self {
        unsafe { Self::from_inner_in(ptr, GlobalAllocator) }
    }
}

impl<T: ?Sized, A: Allocator> Rc<T, A> {
    #[inline(always)]
    fn inner(&self) -> &RcInner<T> {
        // This unsafety is ok because while this Rc is alive we're guaranteed
        // that the inner pointer is valid.
        unsafe { self.ptr.as_ref() }
    }

    #[inline]
    unsafe fn from_inner_in(ptr: NonNull<RcInner<T>>, alloc: A) -> Self {
        Self { ptr, alloc }
    }
}

impl<T: ?Sized> RcInner<T> {
    #[inline]
    fn count(&self) -> usize {
        self.count.get()
    }

    #[inline]
    fn inc(&self) {
        let count = self.count();

        // We insert an `assume` here to hint LLVM at an otherwise
        // missed optimization.
        // SAFETY: The reference count will never be zero when this is
        // called.
        unsafe {
            hint::assert_unchecked(count != 0);
        }

        let strong = count.wrapping_add(1);
        self.count.set(strong);

        // We want to abort on overflow instead of dropping the value.
        // Checking for overflow after the store instead of before
        // allows for slightly better code generation.
        assert_ne!(count, 0);
    }

    #[inline]
    fn dec(&self) -> usize {
        let count = self.count();

        // We insert an `assume` here to hint LLVM at an otherwise
        // missed optimization.
        // SAFETY: The reference count will never be zero when this is
        // called (we're currently holding a reference).
        unsafe {
            hint::assert_unchecked(count != 0);
        }

        let new_count = count - 1;
        self.count.set(new_count);
        new_count
    }
}

impl<T: ?Sized, A: Allocator> Drop for Rc<T, A> {
    fn drop(&mut self) {
        unsafe {
            let new_count = self.inner().dec();

            if new_count == 0 {
                // This was the last reference, deallocate the inner value
                // SAFETY: We just decremented the count to 0, so we're the last reference
                core::ptr::drop_in_place(self.ptr.as_ptr());

                // Deallocate the memory
                let layout = core::alloc::Layout::for_value(self.ptr.as_ref());
                self.alloc.dealloc(self.ptr.as_ptr() as *mut u8, layout);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;

    #[test]
    fn test_rc_basic_creation() {
        let rc = Rc::new(42).unwrap();
        assert_eq!(*rc, 42);
        assert_eq!(rc.inner().count(), 1);
    }

    #[test]
    fn test_rc_clone_increments_count() {
        let rc1 = Rc::new(100).unwrap();
        assert_eq!(rc1.inner().count(), 1);

        let rc2 = rc1.clone();
        assert_eq!(rc1.inner().count(), 2);
        assert_eq!(rc2.inner().count(), 2);
        assert_eq!(*rc1, 100);
        assert_eq!(*rc2, 100);
    }

    #[test]
    fn test_rc_drop_decrements_count() {
        let rc1 = Rc::new(200).unwrap();
        assert_eq!(rc1.inner().count(), 1);

        {
            let rc2 = rc1.clone();
            assert_eq!(rc1.inner().count(), 2);
            assert_eq!(rc2.inner().count(), 2);
            // rc2 drops here
        }

        // After rc2 is dropped, count should be back to 1
        assert_eq!(rc1.inner().count(), 1);
    }

    #[test]
    fn test_rc_multiple_clones() {
        let rc1 = Rc::new(std::string::String::from("test")).unwrap();
        assert_eq!(rc1.inner().count(), 1);

        let rc2 = rc1.clone();
        assert_eq!(rc1.inner().count(), 2);

        let rc3 = rc1.clone();
        assert_eq!(rc1.inner().count(), 3);

        let rc4 = rc2.clone();
        assert_eq!(rc1.inner().count(), 4);

        drop(rc2);
        assert_eq!(rc1.inner().count(), 3);

        drop(rc3);
        assert_eq!(rc1.inner().count(), 2);

        drop(rc4);
        assert_eq!(rc1.inner().count(), 1);
    }

    #[test]
    fn test_rc_get_mut_when_unique() {
        let mut rc = Rc::new(42).unwrap();
        assert_eq!(rc.inner().count(), 1);

        // Should be able to get mutable reference when count is 1
        let value = rc.get_mut();
        assert!(value.is_some());
        *value.unwrap() = 100;
        assert_eq!(*rc, 100);
    }

    #[test]
    fn test_rc_get_mut_fails_when_not_unique() {
        let mut rc1 = Rc::new(42).unwrap();
        let _rc2 = rc1.clone();

        assert_eq!(rc1.inner().count(), 2);

        // Should NOT be able to get mutable reference when count > 1
        let value = rc1.get_mut();
        assert!(value.is_none());
    }

    #[test]
    fn test_rc_get_mut_after_others_dropped() {
        let mut rc1 = Rc::new(42).unwrap();

        {
            let _rc2 = rc1.clone();
            let _rc3 = rc1.clone();
            assert_eq!(rc1.inner().count(), 3);

            // Can't get mutable access while others exist
            assert!(rc1.get_mut().is_none());
            // _rc2 and _rc3 drop here
        }

        // Now we should be able to get mutable access
        assert_eq!(rc1.inner().count(), 1);
        let value = rc1.get_mut();
        assert!(value.is_some());
        *value.unwrap() = 999;
        assert_eq!(*rc1, 999);
    }

    #[test]
    fn test_rc_deref() {
        let rc = Rc::new(std::string::String::from("hello")).unwrap();
        assert_eq!(rc.len(), 5);
        assert_eq!(&*rc, "hello");
    }

    #[test]
    fn test_rc_is_unique() {
        let rc1 = Rc::new(42).unwrap();
        assert!(rc1.is_unique());

        let rc2 = rc1.clone();
        assert!(!rc1.is_unique());
        assert!(!rc2.is_unique());

        drop(rc2);
        assert!(rc1.is_unique());
    }

    #[test]
    fn test_rc_with_vec() {
        let rc1 = Rc::new(std::vec![1, 2, 3, 4, 5]).unwrap();
        assert_eq!(rc1.inner().count(), 1);
        assert_eq!(rc1.len(), 5);

        let rc2 = rc1.clone();
        assert_eq!(rc1.inner().count(), 2);
        assert_eq!(rc2.len(), 5);
    }

    #[test]
    fn test_rc_stress_many_clones() {
        let rc1 = Rc::new(12345).unwrap();
        let mut clones = std::vec![];

        // Create 100 clones
        for _ in 0..100 {
            clones.push(rc1.clone());
        }

        assert_eq!(rc1.inner().count(), 101); // Original + 100 clones

        // Drop half of them
        clones.truncate(50);
        assert_eq!(rc1.inner().count(), 51); // Original + 50 remaining clones

        // Drop the rest
        clones.clear();
        assert_eq!(rc1.inner().count(), 1); // Just the original
    }
}
