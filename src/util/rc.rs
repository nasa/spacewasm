// Portions of this file are derived from the Rust project
// (https://github.com/rust-lang/rust), licensed under Apache-2.0. These
// portions have been modified for SpaceWasm.

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

impl<T: ?Sized + Debug, A: Allocator> Debug for Rc<T, A> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        (**self).fmt(f)
    }
}

impl<T: RefUnwindSafe + ?Sized, A: Allocator + UnwindSafe> UnwindSafe for Rc<T, A> {}
impl<T: RefUnwindSafe + ?Sized, A: Allocator + UnwindSafe> RefUnwindSafe for Rc<T, A> {}

impl<T: ?Sized, A: Allocator> Deref for Rc<T, A> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &unsafe { self.ptr.as_ref() }.value
    }
}

impl<T> Rc<T> {
    pub fn new(value: T) -> Result<Rc<T>, AllocError> {
        Rc::new_in(GlobalAllocator, value)
    }
}

impl<T, A: Allocator + Clone> Rc<T, A> {
    pub fn new_in(alloc: A, value: T) -> Result<Rc<T, A>, AllocError> {
        unsafe {
            Ok(Self::from_inner_in(
                crate::Box::leak(crate::Box::new_in(
                    alloc.clone(),
                    RcInner {
                        count: Cell::new(1),
                        value,
                    },
                )?)
                .into(),
                alloc,
            ))
        }
    }
}

impl<T> Rc<[T]> {
    /// Creates a new `Rc<[T]>` by calling `init` for each element.
    pub fn new_slice<F>(len: usize, init: F) -> Result<Rc<[T]>, AllocError>
    where
        F: FnMut(usize) -> T,
    {
        Rc::new_slice_in(GlobalAllocator, len, init)
    }

    /// Creates a new `Rc<[T]>` with `len` default-initialized elements.
    pub fn new_slice_with_default(len: usize) -> Result<Rc<[T]>, AllocError>
    where
        T: Default,
    {
        Self::new_slice(len, |_| T::default())
    }
}

impl<T, A: Allocator + Clone> Rc<[T], A> {
    /// Creates a new `Rc<[T]>` with uninitialized memory for `len` elements.
    /// Returns a pointer to the start of the slice data and the Rc.
    ///
    /// # Safety
    /// Caller must initialize all `len` elements before using the Rc.
    unsafe fn new_uninit_slice_in(
        alloc: A,
        len: usize,
    ) -> Result<(*mut T, Rc<[T], A>), AllocError> {
        unsafe {
            if len == 0 {
                // For empty slices, just allocate the RcInner with empty slice
                let layout = core::alloc::Layout::new::<Cell<u32>>();
                let ptr = alloc.alloc(layout)?;
                core::ptr::write(ptr as *mut Cell<u32>, Cell::new(1));

                let rc_inner_ptr =
                    core::ptr::slice_from_raw_parts_mut(ptr as *mut (), 0) as *mut RcInner<[T]>;

                let rc = Self::from_inner_in(NonNull::new_unchecked(rc_inner_ptr), alloc);
                return Ok((core::ptr::null_mut(), rc));
            }

            // Calculate the layout we need: Cell<u32> + align padding + [T; len]
            let count_layout = core::alloc::Layout::new::<Cell<u32>>();
            let slice_layout = core::alloc::Layout::array::<T>(len).unwrap();
            let (full_layout, slice_offset) = count_layout.extend(slice_layout).unwrap();
            let full_layout = full_layout.pad_to_align();

            // Allocate new memory for RcInner<[T]>
            let ptr = alloc.alloc(full_layout)?;

            // Write the count field (note: count is u32, not usize)
            core::ptr::write(ptr as *mut Cell<u32>, Cell::new(1));

            // Get pointer to the slice data (uninitialized)
            let slice_ptr = ptr.add(slice_offset) as *mut T;

            // Create the fat pointer for RcInner<[T]>
            let rc_inner_ptr =
                core::ptr::slice_from_raw_parts_mut(ptr as *mut (), len) as *mut RcInner<[T]>;

            let rc = Self::from_inner_in(NonNull::new_unchecked(rc_inner_ptr), alloc);
            Ok((slice_ptr, rc))
        }
    }

    /// Creates a new `Rc<[T]>` by calling `init` for each element.
    pub fn new_slice_in<F>(alloc: A, len: usize, mut init: F) -> Result<Rc<[T], A>, AllocError>
    where
        F: FnMut(usize) -> T,
    {
        unsafe {
            let (slice_ptr, rc) = Self::new_uninit_slice_in(alloc, len)?;

            // Initialize each element
            for i in 0..len {
                core::ptr::write(slice_ptr.add(i), init(i));
            }

            Ok(rc)
        }
    }

    /// Creates a new `Rc<[T]>` with `len` default-initialized elements.
    pub fn new_slice_with_default_in(alloc: A, len: usize) -> Result<Rc<[T], A>, AllocError>
    where
        T: Default,
    {
        Self::new_slice_in(alloc, len, |_| T::default())
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

impl<T: ?Sized, A: Allocator> Rc<T, A> {
    #[inline]
    fn is_unique(&self) -> bool {
        self.inner().count() == 1
    }

    /// Convert `Rc<T>` into `Rc<U>` using a coercion function (e.g., for trait objects)
    ///
    /// The coercion function should convert a reference `&T` to `&U`, typically for
    /// trait object coercion like `|x| x as &dyn Trait`.
    ///
    /// # Safety
    /// The caller must ensure that the coercion is valid and that the resulting
    /// `RcInner<U>` has the same memory layout as `RcInner<T>`.
    pub unsafe fn into_dyn<U: ?Sized, F>(self, coerce: F) -> Rc<U, A>
    where
        F: FnOnce(&T) -> &U,
    {
        // Represent a fat pointer as two usize values (data pointer and vtable/length)
        #[repr(C)]
        #[derive(Copy, Clone)]
        struct FatPtr {
            data: *const (),
            meta: usize,
        }

        #[repr(C)]
        union PtrCast<T: ?Sized> {
            ptr: *const T,
            fat: core::mem::ManuallyDrop<FatPtr>,
        }

        unsafe {
            let inner_ptr = self.ptr.as_ptr();

            // Get a reference to the value and coerce it to get the trait object
            let value_ref: &T = &(*inner_ptr).value;
            let trait_ref: &U = coerce(value_ref);

            // Extract the metadata (vtable) from the trait object reference
            let trait_value_fat = PtrCast {
                ptr: trait_ref as *const U,
            };
            let vtable = trait_value_fat.fat.meta;

            // Create a new fat pointer with the RcInner base address and the trait's vtable
            let inner_fat = PtrCast::<RcInner<U>> {
                fat: core::mem::ManuallyDrop::new(FatPtr {
                    data: inner_ptr as *const (),
                    meta: vtable,
                }),
            };

            let trait_inner_ptr: *mut RcInner<U> = inner_fat.ptr as *mut RcInner<U>;

            let trait_ptr = NonNull::new_unchecked(trait_inner_ptr);
            let alloc = core::ptr::read(&self.alloc);
            core::mem::forget(self); // Don't run Drop, we're transferring ownership

            Rc::from_inner_in(trait_ptr, alloc)
        }
    }

    #[inline]
    pub fn get_mut(&mut self) -> Option<&mut T> {
        if Rc::is_unique(self) {
            unsafe { Some(Rc::get_mut_unchecked(self)) }
        } else {
            None
        }
    }

    /// # Safety
    /// The caller must ensure that no other references to the inner value exist.
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
        // `strong` is widened to usize, but the backing field is u32, so the
        // wraparound-to-zero we're detecting only happens in the `as u32`
        // truncation above - check the truncated value, not `strong` itself.
        assert_ne!(strong as u32, 0);
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
    use crate::test_support::RustSystemAllocator;
    extern crate std;

    #[test]
    fn test_rc_basic_creation() {
        let rc = Rc::new_in(RustSystemAllocator, 42).unwrap();
        assert_eq!(*rc, 42);
        assert_eq!(rc.inner().count(), 1);
    }

    #[test]
    fn test_rc_clone_increments_count() {
        let rc1 = Rc::new_in(RustSystemAllocator, 100).unwrap();
        assert_eq!(rc1.inner().count(), 1);

        let rc2 = rc1.clone();
        assert_eq!(rc1.inner().count(), 2);
        assert_eq!(rc2.inner().count(), 2);
        assert_eq!(*rc1, 100);
        assert_eq!(*rc2, 100);
    }

    #[test]
    fn test_rc_drop_decrements_count() {
        let rc1 = Rc::new_in(RustSystemAllocator, 200).unwrap();
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
        let rc1 = Rc::new_in(RustSystemAllocator, std::string::String::from("test")).unwrap();
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
        let mut rc = Rc::new_in(RustSystemAllocator, 42).unwrap();
        assert_eq!(rc.inner().count(), 1);

        // Should be able to get mutable reference when count is 1
        let value = rc.get_mut();
        assert!(value.is_some());
        *value.unwrap() = 100;
        assert_eq!(*rc, 100);
    }

    #[test]
    fn test_rc_get_mut_fails_when_not_unique() {
        let mut rc1 = Rc::new_in(RustSystemAllocator, 42).unwrap();
        let _rc2 = rc1.clone();

        assert_eq!(rc1.inner().count(), 2);

        // Should NOT be able to get mutable reference when count > 1
        let value = rc1.get_mut();
        assert!(value.is_none());
    }

    #[test]
    fn test_rc_get_mut_after_others_dropped() {
        let mut rc1 = Rc::new_in(RustSystemAllocator, 42).unwrap();

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
        let rc = Rc::new_in(RustSystemAllocator, std::string::String::from("hello")).unwrap();
        assert_eq!(rc.len(), 5);
        assert_eq!(&*rc, "hello");
    }

    #[test]
    fn test_rc_is_unique() {
        let rc1 = Rc::new_in(RustSystemAllocator, 42).unwrap();
        assert!(rc1.is_unique());

        let rc2 = rc1.clone();
        assert!(!rc1.is_unique());
        assert!(!rc2.is_unique());

        drop(rc2);
        assert!(rc1.is_unique());
    }

    #[test]
    fn test_rc_with_vec() {
        let rc1 = Rc::new_in(RustSystemAllocator, std::vec![1, 2, 3, 4, 5]).unwrap();
        assert_eq!(rc1.inner().count(), 1);
        assert_eq!(rc1.len(), 5);

        let rc2 = rc1.clone();
        assert_eq!(rc1.inner().count(), 2);
        assert_eq!(rc2.len(), 5);
    }

    #[test]
    fn test_rc_stress_many_clones() {
        let rc1 = Rc::new_in(RustSystemAllocator, 12345).unwrap();
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
        let rc = Rc::new_slice_in(RustSystemAllocator, 5, |i| (i * 2) as i32).unwrap();

        assert_eq!(rc.len(), 5);
        assert_eq!(&*rc, &[0, 2, 4, 6, 8]);
        assert_eq!(rc.inner().count(), 1);
    }

    #[test]
    fn test_rc_new_slice_empty() {
        let rc = Rc::new_slice_in(RustSystemAllocator, 0, |_| 42i32).unwrap();

        assert_eq!(rc.len(), 0);
        assert_eq!(&*rc, &[] as &[i32]);
        assert_eq!(rc.inner().count(), 1);
    }

    #[test]
    fn test_rc_new_slice_clone() {
        let rc1 = Rc::new_slice_in(RustSystemAllocator, 3, |i| i as i32 + 10).unwrap();
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
        assert_eq!(&*rc, &[] as &[i32]);
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
            let rc = Rc::new_slice_in(RustSystemAllocator, 5, |_| DropCounter).unwrap();
            assert_eq!(rc.len(), 5);
            assert_eq!(DROP_COUNT.load(Ordering::SeqCst), 0);
        }

        // All 5 elements should have been dropped
        assert_eq!(DROP_COUNT.load(Ordering::SeqCst), 5);
    }

    #[test]
    fn test_rc_slice_large() {
        let rc = Rc::new_slice_in(RustSystemAllocator, 1000, |i| i as u32).unwrap();

        assert_eq!(rc.len(), 1000);
        assert_eq!(rc[0], 0);
        assert_eq!(rc[500], 500);
        assert_eq!(rc[999], 999);
        assert_eq!(rc.inner().count(), 1);
    }

    #[test]
    fn test_rc_into_dyn() {
        trait MyTrait {
            fn get_value(&self) -> i32;
        }

        struct MyStruct {
            value: i32,
        }

        impl MyTrait for MyStruct {
            fn get_value(&self) -> i32 {
                self.value
            }
        }

        let rc = Rc::new_in(RustSystemAllocator, MyStruct { value: 42 }).unwrap();
        assert_eq!(rc.inner().count(), 1);
        assert_eq!(rc.value, 42);

        // Convert to trait object
        let rc_dyn = unsafe { rc.into_dyn(|x| x as &dyn MyTrait) };
        assert_eq!(rc_dyn.inner().count(), 1);
        assert_eq!(rc_dyn.get_value(), 42);

        // Clone the trait object Rc
        let rc_dyn2 = rc_dyn.clone();
        assert_eq!(rc_dyn.inner().count(), 2);
        assert_eq!(rc_dyn2.get_value(), 42);
    }
}

#[cfg(kani)]
mod kani_proofs {
    use super::*;
    use crate::test_support::RustSystemAllocator;

    /// Verify reference counting correctness: clone increments, drop decrements
    /// Counter invariants: never zero while Rc exists, never overflows
    #[kani::proof]
    fn proof_rc_reference_counting() {
        let rc1 = Rc::new_in(RustSystemAllocator, 42u32);
        kani::assume(rc1.is_ok());
        let rc1 = rc1.unwrap();

        // Initial state: count must be 1
        assert_eq!(rc1.inner().count(), 1, "Initial count must be 1");

        // Clone increments count
        let rc2 = rc1.clone();
        assert_eq!(rc1.inner().count(), 2, "Count must be 2 after first clone");
        assert_eq!(rc2.inner().count(), 2, "Both Rcs must see same count");
        assert_eq!(*rc1, 42, "Value must be accessible through rc1");
        assert_eq!(*rc2, 42, "Value must be accessible through rc2");

        // Second clone increments again
        let rc3 = rc1.clone();
        assert_eq!(rc1.inner().count(), 3, "Count must be 3 after second clone");
        assert_eq!(rc2.inner().count(), 3, "All Rcs must see same count");
        assert_eq!(rc3.inner().count(), 3, "All Rcs must see same count");

        // Drop rc3 - count decrements
        drop(rc3);
        assert_eq!(rc1.inner().count(), 2, "Count must be 2 after dropping rc3");
        assert_eq!(
            rc2.inner().count(),
            2,
            "Both remaining Rcs must see count 2"
        );

        // Drop rc2 - count decrements to 1
        drop(rc2);
        assert_eq!(rc1.inner().count(), 1, "Count must be 1 after dropping rc2");

        // rc1 drops at end of scope - count becomes 0, memory deallocated
    }

    /// Verify that Rc properly deallocates when last reference is dropped
    /// Tests the drop path and ensures no memory leaks
    #[kani::proof]
    fn proof_rc_last_drop_deallocates() {
        let value: u32 = kani::any();

        {
            let rc1 = Rc::new_in(RustSystemAllocator, value);
            kani::assume(rc1.is_ok());
            let rc1 = rc1.unwrap();
            assert_eq!(rc1.inner().count(), 1, "Count must be 1");

            {
                let rc2 = rc1.clone();
                assert_eq!(rc1.inner().count(), 2, "Count must be 2");
                assert_eq!(*rc2, value, "Value must match");
                // rc2 drops here
            }

            assert_eq!(rc1.inner().count(), 1, "Count must be 1 after rc2 dropped");
            assert_eq!(*rc1, value, "Value still accessible");
            // rc1 drops here - this triggers deallocation
        }

        // After this scope, all memory must be freed
        // Kani will verify no memory leaks
    }

    /// Verify get_mut returns Some only when unique (count == 1)
    /// Ensures exclusive access invariant is maintained
    #[kani::proof]
    fn proof_rc_get_mut_uniqueness() {
        let value: u32 = kani::any();

        let rc1 = Rc::new_in(RustSystemAllocator, value);
        kani::assume(rc1.is_ok());
        let mut rc1 = rc1.unwrap();

        // When unique, get_mut should succeed
        assert_eq!(rc1.inner().count(), 1, "Count must be 1");
        let mut_ref = rc1.get_mut();
        assert!(mut_ref.is_some(), "get_mut must return Some when unique");

        let new_value: u32 = kani::any();
        *mut_ref.unwrap() = new_value;
        assert_eq!(*rc1, new_value, "Mutation must be visible");

        // After clone, get_mut should fail
        let _rc2 = rc1.clone();
        assert_eq!(rc1.inner().count(), 2, "Count must be 2");
        let mut_ref2 = rc1.get_mut();
        assert!(
            mut_ref2.is_none(),
            "get_mut must return None when not unique"
        );

        // After drop, get_mut should succeed again
        drop(_rc2);
        assert_eq!(rc1.inner().count(), 1, "Count must be 1 again");
        let mut_ref3 = rc1.get_mut();
        assert!(
            mut_ref3.is_some(),
            "get_mut must return Some when unique again"
        );
    }

    /// Verify is_unique correctly identifies when Rc has no other references
    #[kani::proof]
    fn proof_rc_is_unique() {
        let rc1 = Rc::new_in(RustSystemAllocator, 100u32);
        kani::assume(rc1.is_ok());
        let rc1 = rc1.unwrap();

        // Initially unique
        assert!(rc1.is_unique(), "Must be unique initially");
        assert_eq!(rc1.inner().count(), 1, "Count must be 1");

        // After clone, not unique
        let rc2 = rc1.clone();
        assert!(!rc1.is_unique(), "Must not be unique after clone");
        assert!(!rc2.is_unique(), "Must not be unique after clone");
        assert_eq!(rc1.inner().count(), 2, "Count must be 2");

        // After drop, unique again
        drop(rc2);
        assert!(rc1.is_unique(), "Must be unique again after drop");
        assert_eq!(rc1.inner().count(), 1, "Count must be 1 again");
    }

    /// Verify Rc::new_slice allocates correct layout and initializes all elements
    /// Tests slice-specific allocation path
    #[kani::proof]
    #[kani::unwind(5)] // Limit loop unrolling (len <= 4)
    fn proof_rc_new_slice_allocation() {
        // Use small symbolic length for tractable verification
        let len: usize = kani::any();
        kani::assume(len <= 4); // Keep small for verification tractability

        let rc = Rc::new_slice_in(RustSystemAllocator, len, |i| i as u32);
        kani::assume(rc.is_ok());
        let rc = rc.unwrap();

        // Verify length
        assert_eq!(rc.len(), len, "Slice length must match requested length");
        assert_eq!(rc.inner().count(), 1, "Count must be 1");

        // Verify all elements are initialized correctly
        for i in 0..len {
            assert_eq!(rc[i], i as u32, "Element must be initialized correctly");
        }
    }

    /// Verify Rc::new_slice with empty slice (edge case)
    #[kani::proof]
    fn proof_rc_new_slice_empty() {
        let rc = Rc::new_slice_in(RustSystemAllocator, 0, |_| 42u32);
        kani::assume(rc.is_ok());
        let rc = rc.unwrap();

        assert_eq!(rc.len(), 0, "Empty slice must have length 0");
        assert_eq!(rc.inner().count(), 1, "Count must be 1");

        // Clone and verify count
        let rc2 = rc.clone();
        assert_eq!(rc.inner().count(), 2, "Count must be 2 after clone");
        assert_eq!(rc2.len(), 0, "Cloned slice must also be empty");
    }

    /// Verify Rc slice drops all elements properly
    /// Ensures drop order and completeness
    #[kani::proof]
    #[kani::unwind(5)] // Limit loop unrolling (len <= 4)
    fn proof_rc_slice_drop_elements() {
        // Use DropCounter to track drops
        static mut DROP_COUNT: u32 = 0;

        struct DropCounter(u32);
        impl Drop for DropCounter {
            fn drop(&mut self) {
                unsafe {
                    DROP_COUNT += 1;
                }
            }
        }

        unsafe {
            DROP_COUNT = 0;
        }

        let len: usize = kani::any();
        kani::assume(len > 0 && len <= 4); // Small for tractability

        {
            let rc = Rc::new_slice_in(RustSystemAllocator, len, |i| DropCounter(i as u32));
            kani::assume(rc.is_ok());
            let rc = rc.unwrap();

            assert_eq!(rc.len(), len, "Length must match");
            assert_eq!(unsafe { DROP_COUNT }, 0, "No drops yet");

            // Clone doesn't drop elements
            let rc2 = rc.clone();
            assert_eq!(rc.inner().count(), 2, "Count must be 2");
            assert_eq!(unsafe { DROP_COUNT }, 0, "Still no drops");

            drop(rc2);
            assert_eq!(
                unsafe { DROP_COUNT },
                0,
                "Still no drops after dropping rc2"
            );
            // rc drops here - should drop all elements
        }

        // All elements should be dropped exactly once
        assert_eq!(
            unsafe { DROP_COUNT },
            len as u32,
            "All elements must be dropped exactly once"
        );
    }

    /// Make sure new_slice_with_default_in works as expected
    #[kani::proof]
    fn proof_rc_new_slice_with_default() {
        let len: usize = kani::any();
        kani::assume(len <= 2); // Keep state explosion away

        let rc: Result<Rc<[u32], _>, _> = Rc::new_slice_with_default_in(RustSystemAllocator, len);
        kani::assume(rc.is_ok());
        let rc = rc.unwrap();

        assert_eq!(rc.len(), len, "Slice length must match requested length");
        assert_eq!(rc.inner().count(), 1, "Count must be 1");

        if len > 0 {
            assert_eq!(rc[0], 0u32, "First element must be default-initialized to 0");
        }
    }

    /// Verify Rc deref returns correct value and doesn't violate aliasing
    #[kani::proof]
    fn proof_rc_deref_correctness() {
        let value: u32 = kani::any();

        let rc1 = Rc::new_in(RustSystemAllocator, value);
        kani::assume(rc1.is_ok());
        let rc1 = rc1.unwrap();

        // Deref must return the original value
        assert_eq!(*rc1, value, "Deref must return original value");

        // Multiple derefs from clones must all see same value
        let rc2 = rc1.clone();
        let rc3 = rc1.clone();

        assert_eq!(*rc1, value, "rc1 deref must return value");
        assert_eq!(*rc2, value, "rc2 deref must return value");
        assert_eq!(*rc3, value, "rc3 deref must return value");

        // All refs point to same underlying data
        let ref1 = &*rc1 as *const u32;
        let ref2 = &*rc2 as *const u32;
        let ref3 = &*rc3 as *const u32;

        assert_eq!(ref1, ref2, "All derefs must point to same address");
        assert_eq!(ref2, ref3, "All derefs must point to same address");
    }

    /// Verify count increments correctly right up to the edge of overflow.
    /// Directly seeds the private count field near `u32::MAX` instead of
    /// looping from 1, since reaching that boundary by cloning one at a
    /// time is infeasible to unwind (it would take billions of iterations).
    #[kani::proof]
    fn proof_rc_count_increment_near_max() {
        let rc1 = Rc::new_in(RustSystemAllocator, 42u32);
        kani::assume(rc1.is_ok());
        let rc1 = rc1.unwrap();

        let count: u32 = kani::any();
        kani::assume(count >= u32::MAX - 2 && count < u32::MAX);
        rc1.inner().count.set(count);

        let rc2 = rc1.clone();
        assert_eq!(
            rc1.inner().count(),
            (count + 1) as usize,
            "Count must increment correctly up to the boundary"
        );

        // Prevent Drop from decrementing a count it never incremented for real
        core::mem::forget(rc1);
        core::mem::forget(rc2);
    }

    /// Verify that incrementing the count past `u32::MAX` aborts instead of
    /// silently wrapping to 0 (which would cause a premature deallocation
    /// while other `Rc`s are still alive).
    #[kani::proof]
    #[kani::should_panic]
    fn proof_rc_count_overflow_aborts() {
        let rc1 = Rc::new_in(RustSystemAllocator, 42u32);
        kani::assume(rc1.is_ok());
        let rc1 = rc1.unwrap();

        rc1.inner().count.set(u32::MAX);
        let rc2 = rc1.clone();

        // The clone above is expected to abort before reaching here. If CBMC
        // continues past the failed assertion anyway, prevent Drop from
        // running on the corrupted (wrapped-to-0) count.
        core::mem::forget(rc1);
        core::mem::forget(rc2);
    }
}
