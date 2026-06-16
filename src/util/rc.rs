use crate::{AllocError, Allocator, GlobalAllocator};
use core::cell::Cell;
use core::fmt::{Debug, Formatter};
use core::hint;
use core::ops::Deref;
use core::panic::{RefUnwindSafe, UnwindSafe};
use core::ptr::NonNull;

struct RcInner<T: ?Sized> {
    count: Cell<u32>,
    value: T,
}

pub struct Rc<T: ?Sized, A: Allocator = GlobalAllocator> {
    ptr: NonNull<RcInner<T>>,
    alloc: A,
}

impl<T: ?Sized + Debug> Debug for Rc<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        (**self).fmt(f)
    }
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
}

impl<T> Rc<[T]> {
    /// Creates a new `Rc<[T]>` with uninitialized memory for `len` elements.
    /// Returns a pointer to the start of the slice data and the Rc.
    ///
    /// # Safety
    /// Caller must initialize all `len` elements before using the Rc.
    unsafe fn new_uninit_slice(len: usize) -> Result<(*mut T, Rc<[T]>), AllocError> {
        unsafe {
            if len == 0 {
                // For empty slices, just allocate the RcInner with empty slice
                let layout = core::alloc::Layout::new::<Cell<u32>>();
                let ptr = GlobalAllocator.alloc(layout)?;
                core::ptr::write(ptr as *mut Cell<u32>, Cell::new(1));

                let rc_inner_ptr =
                    core::ptr::slice_from_raw_parts_mut(ptr as *mut (), 0) as *mut RcInner<[T]>;

                let rc = Self::from_inner(NonNull::new_unchecked(rc_inner_ptr));
                return Ok((core::ptr::null_mut(), rc));
            }

            // Calculate the layout we need: Cell<u32> + align padding + [T; len]
            let count_layout = core::alloc::Layout::new::<Cell<u32>>();
            let slice_layout = core::alloc::Layout::array::<T>(len)?;
            let (full_layout, slice_offset) = count_layout.extend(slice_layout)?;
            let full_layout = full_layout.pad_to_align();

            // Allocate new memory for RcInner<[T]>
            let ptr = GlobalAllocator.alloc(full_layout)?;

            // Write the count field (note: count is u32, not usize)
            core::ptr::write(ptr as *mut Cell<u32>, Cell::new(1));

            // Get pointer to the slice data (uninitialized)
            let slice_ptr = ptr.add(slice_offset) as *mut T;

            // Create the fat pointer for RcInner<[T]>
            let rc_inner_ptr =
                core::ptr::slice_from_raw_parts_mut(ptr as *mut (), len) as *mut RcInner<[T]>;

            let rc = Self::from_inner(NonNull::new_unchecked(rc_inner_ptr));
            Ok((slice_ptr, rc))
        }
    }

    /// Creates a new `Rc<[T]>` by calling `init` for each element.
    pub fn new_slice<F>(len: usize, mut init: F) -> Result<Rc<[T]>, AllocError>
    where
        F: FnMut(usize) -> T,
    {
        unsafe {
            let (slice_ptr, rc) = Self::new_uninit_slice(len)?;

            // Initialize each element
            for i in 0..len {
                core::ptr::write(slice_ptr.add(i), init(i));
            }

            Ok(rc)
        }
    }

    /// Creates a new `Rc<[T]>` with `len` default-initialized elements.
    pub fn new_slice_with_default(len: usize) -> Result<Rc<[T]>, AllocError>
    where
        T: Default,
    {
        Self::new_slice(len, |_| T::default())
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
        self.count.get() as usize
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
        self.count.set(strong as u32);

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
        self.count.set(new_count as u32);
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

    #[test]
    fn test_rc_new_slice() {
        let rc = Rc::new_slice(5, |i| (i * 2) as i32).unwrap();

        assert_eq!(rc.len(), 5);
        assert_eq!(&*rc, &[0, 2, 4, 6, 8]);
        assert_eq!(rc.inner().count(), 1);
    }

    #[test]
    fn test_rc_new_slice_empty() {
        let rc: Rc<[i32]> = Rc::new_slice(0, |_| 42i32).unwrap();

        assert_eq!(rc.len(), 0);
        assert_eq!(&*rc, &[]);
        assert_eq!(rc.inner().count(), 1);
    }

    #[test]
    fn test_rc_new_slice_clone() {
        let rc1 = Rc::new_slice(3, |i| i as i32 + 10).unwrap();
        let rc2 = rc1.clone();

        assert_eq!(rc1.inner().count(), 2);
        assert_eq!(rc2.inner().count(), 2);
        assert_eq!(&*rc1, &[10, 11, 12]);
        assert_eq!(&*rc2, &[10, 11, 12]);
    }

    #[test]
    fn test_rc_new_slice_with_default() {
        let rc: Rc<[i32]> = Rc::new_slice_with_default(3).unwrap();

        assert_eq!(rc.len(), 3);
        assert_eq!(&*rc, &[0, 0, 0]);
        assert_eq!(rc.inner().count(), 1);
    }

    #[test]
    fn test_rc_new_slice_with_default_empty() {
        let rc: Rc<[i32]> = Rc::new_slice_with_default(0).unwrap();

        assert_eq!(rc.len(), 0);
        assert_eq!(&*rc, &[]);
        assert_eq!(rc.inner().count(), 1);
    }

    #[test]
    fn test_rc_new_slice_with_default_string() {
        let rc: Rc<[std::string::String]> = Rc::new_slice_with_default(3).unwrap();

        assert_eq!(rc.len(), 3);
        assert_eq!(
            &*rc,
            &[
                std::string::String::new(),
                std::string::String::new(),
                std::string::String::new()
            ]
        );
        assert_eq!(rc.inner().count(), 1);
    }

    #[test]
    fn test_rc_slice_drop_runs_destructors() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        static DROP_COUNT: AtomicUsize = AtomicUsize::new(0);

        struct DropCounter;
        impl Drop for DropCounter {
            fn drop(&mut self) {
                DROP_COUNT.fetch_add(1, Ordering::SeqCst);
            }
        }

        DROP_COUNT.store(0, Ordering::SeqCst);

        {
            let rc = Rc::new_slice(5, |_| DropCounter).unwrap();
            assert_eq!(rc.len(), 5);
            assert_eq!(DROP_COUNT.load(Ordering::SeqCst), 0);
        }

        // All 5 elements should have been dropped
        assert_eq!(DROP_COUNT.load(Ordering::SeqCst), 5);
    }

    #[test]
    fn test_rc_slice_large() {
        let rc = Rc::new_slice(1000, |i| i as u32).unwrap();

        assert_eq!(rc.len(), 1000);
        assert_eq!(rc[0], 0);
        assert_eq!(rc[500], 500);
        assert_eq!(rc[999], 999);
        assert_eq!(rc.inner().count(), 1);
    }
}
