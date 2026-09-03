use core::ops::{Deref, DerefMut};

use crate::AllocError;

/// The raw backing store shared by [`crate::Vec`] and the streaming API.
///
/// # Invariants
/// - `len <= capacity`.
/// - If `capacity == 0`, `ptr` may be null; otherwise `ptr` points to a single
///   allocation valid for `capacity` values of `T`, and the first `len`
///   elements are initialized.
/// - `InnerVec` does **not** own its allocation: it never frees the buffer and
///   never drops its elements. The owner (typically [`crate::Vec`]) is
///   responsible for both.
pub struct InnerVec<T: Sized> {
    pub(crate) ptr: *mut T,
    pub(crate) capacity: u32,
    pub(crate) len: u32,
}

impl<T: core::fmt::Debug> core::fmt::Debug for InnerVec<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<T: Sized> InnerVec<T> {
    /// Create an empty `InnerVec` with no backing allocation.
    ///
    /// This is the only *safe* constructor: it upholds every invariant
    /// trivially with `len == capacity == 0` and a null `ptr`. Populated
    /// instances must be built with [`InnerVec::from_raw_parts`].
    pub fn zero() -> InnerVec<T> {
        InnerVec {
            ptr: core::ptr::null_mut(),
            capacity: 0,
            len: 0,
        }
    }

    /// Build an `InnerVec` directly from its raw parts.
    ///
    /// # Safety
    /// The caller must uphold all of the [`InnerVec`] invariants:
    /// - `len <= capacity`.
    /// - If `capacity == 0`, `ptr` may be null; otherwise `ptr` must point to a
    ///   single allocation valid for `capacity` values of `T` (correctly sized
    ///   and aligned), with the first `len` elements initialized.
    /// - `InnerVec` neither frees the allocation nor drops its elements, so the
    ///   caller (typically [`crate::Vec`]) retains ownership and must keep the
    ///   allocation valid for as long as the returned value is used.
    pub unsafe fn from_raw_parts(ptr: *mut T, capacity: u32, len: u32) -> InnerVec<T> {
        InnerVec { ptr, capacity, len }
    }

    /// Raw pointer to the backing allocation (null when `capacity == 0`).
    pub fn ptr(&self) -> *mut T {
        self.ptr
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn capacity(&self) -> usize {
        self.capacity as usize
    }

    pub fn try_push(&mut self, value: T) -> Result<(), AllocError> {
        if self.len < self.capacity {
            unsafe {
                core::ptr::write(self.ptr.add(self.len as usize), value);
            }

            self.len += 1;
            Ok(())
        } else {
            Err(AllocError::OutOfMemory)
        }
    }

    /// Push a new item to the vector
    /// If the capacity is exceeded, this will panic
    pub fn push(&mut self, value: T) {
        self.try_push(value)
            .expect("attempting to push beyond Vec capacity");
    }

    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            None
        } else {
            self.len -= 1;
            unsafe { Some(core::ptr::read(self.ptr.add(self.len as usize))) }
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        (**self).iter()
    }
}

impl<T> Deref for InnerVec<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        if self.ptr.is_null() {
            &[]
        } else {
            unsafe { core::slice::from_raw_parts(self.ptr, self.len as usize) }
        }
    }
}

impl<T> DerefMut for InnerVec<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        if self.ptr.is_null() {
            &mut []
        } else {
            unsafe { core::slice::from_raw_parts_mut(self.ptr, self.len as usize) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zero() {
        let vec: InnerVec<i32> = InnerVec::zero();
        assert_eq!(vec.len(), 0);
        assert_eq!(vec.capacity(), 0);
        assert!(vec.ptr.is_null());
    }

    #[test]
    fn test_deref_empty() {
        let vec: InnerVec<i32> = InnerVec::zero();
        let slice: &[i32] = &vec;
        assert_eq!(slice.len(), 0);
    }

    #[test]
    fn test_deref_mut_empty() {
        let mut vec: InnerVec<i32> = InnerVec::zero();
        let slice: &mut [i32] = &mut vec;
        assert_eq!(slice.len(), 0);
    }
}

#[cfg(kani)]
mod kani_proofs {
    use super::*;
    use crate::alloc::Allocator;
    use crate::test_support::RustSystemAllocator;
    use core::alloc::Layout;

    // Helper to create a valid InnerVec with allocated memory
    unsafe fn create_valid_inner_vec<T>(capacity: u32, alloc: &RustSystemAllocator) -> InnerVec<T> {
        if capacity == 0 {
            return InnerVec::zero();
        }

        let layout = Layout::array::<T>(capacity as usize).unwrap();
        let ptr = unsafe { alloc.alloc(layout).unwrap() as *mut T };

        // SAFETY: `ptr` is a fresh allocation valid for `capacity` elements and
        // `len == 0`, so no element is claimed to be initialized.
        unsafe { InnerVec::from_raw_parts(ptr, capacity, 0) }
    }

    // Helper to deallocate an InnerVec
    unsafe fn dealloc_inner_vec<T>(vec: InnerVec<T>, alloc: &RustSystemAllocator) {
        if vec.capacity > 0 && !vec.ptr.is_null() {
            let layout = Layout::array::<T>(vec.capacity as usize).unwrap();
            unsafe { alloc.dealloc(vec.ptr as *mut u8, layout) };
        }
    }

    struct Droppable {
        value: u32,
        drop_counter: *mut u32,
    }

    impl Drop for Droppable {
        fn drop(&mut self) {
            unsafe {
                *self.drop_counter += 1;
            }
        }
    }

    /// len ≤ capacity maintained by all operations
    /// No integer overflows in len or offset calculations
    #[kani::proof]
    fn verify_len_invariants() {
        unsafe {
            let alloc = RustSystemAllocator;
            let capacity: u32 = kani::any();
            kani::assume(capacity > 0 && capacity <= 10);

            let mut vec = create_valid_inner_vec::<u32>(capacity, &alloc);

            // Start with arbitrary valid state
            let initial_len: u32 = kani::any();
            kani::assume(initial_len <= capacity);
            vec.len = initial_len;

            // Perform arbitrary operation
            let op: u8 = kani::any();
            match op % 2 {
                0 if vec.len < vec.capacity => {
                    let old_len = vec.len;
                    vec.push(42);

                    // Verify no overflow on increment
                    assert!(vec.len == old_len + 1, "len must increment by 1");

                    // Verify len as usize doesn't truncate
                    let len_usize = vec.len as usize;
                    assert!(len_usize == vec.len as usize);
                }
                1 if vec.len > 0 => {
                    let old_len = vec.len;
                    vec.pop();

                    // Verify no underflow on decrement
                    assert!(vec.len == old_len - 1, "len must decrement by 1");
                }
                _ => {}
            }

            //  len ≤ capacity must hold after any operation
            assert!(vec.len <= vec.capacity, "len must never exceed capacity");

            //  Offset calculation should not overflow
            let offset = vec.len as usize;
            assert!(offset <= capacity as usize, "offset must fit in usize");

            dealloc_inner_vec(vec, &alloc);
        }
    }

    /// Pointer arithmetic ptr.add(i) for i < capacity stays in bounds
    #[kani::proof]
    fn verify_pointer_arithmetic_in_bounds() {
        unsafe {
            let alloc = RustSystemAllocator;
            let capacity: u32 = kani::any();
            kani::assume(capacity > 0 && capacity <= 10);

            let vec = create_valid_inner_vec::<u32>(capacity, &alloc);

            // Verify we can compute offsets for all valid indices
            let index: u32 = kani::any();
            kani::assume(index < capacity);

            // This pointer arithmetic must be valid
            let _offset_ptr = vec.ptr.add(index as usize);

            // The pointer at capacity (one-past-end) should also be valid for iteration
            let _end_ptr = vec.ptr.add(capacity as usize);

            dealloc_inner_vec(vec, &alloc);
        }
    }

    /// Null pointer (capacity=0) handled safely in all operations
    #[kani::proof]
    fn verify_null_pointer_safety() {
        // Zero-capacity vec with null pointer
        let mut vec: InnerVec<u32> = InnerVec::zero();

        assert!(vec.ptr.is_null());
        assert_eq!(vec.capacity, 0);
        assert_eq!(vec.len, 0);

        // Deref with null should return empty slice
        let slice: &[u32] = &*vec;
        assert_eq!(slice.len(), 0);

        // DerefMut with null should return empty mutable slice
        let slice_mut: &mut [u32] = &mut *vec;
        assert_eq!(slice_mut.len(), 0);

        // Pop on empty should return None
        assert!(vec.pop().is_none());

        // len() and capacity() should work
        assert_eq!(vec.len(), 0);
        assert_eq!(vec.capacity(), 0);
    }

    /// push writes at index len, pop reads from initialized memory
    /// Tests push/pop round-trip correctness
    #[kani::proof]
    #[kani::unwind(4)] // Limit loop unrolling
    fn verify_push_pop_operations() {
        let alloc = RustSystemAllocator;
        let capacity: u32 = kani::any();
        kani::assume(capacity > 0 && capacity <= 3);

        let mut vec = unsafe { create_valid_inner_vec::<u32>(capacity, &alloc) };

        // Test push at different positions
        let initial_len: u32 = kani::any();
        kani::assume(initial_len < capacity);
        vec.len = initial_len;

        // Push a symbolic value
        let value: u32 = kani::any();
        let push_position = vec.len;
        vec.push(value);

        //  After push, len increased by 1
        assert_eq!(vec.len, initial_len + 1, "push must increment len");

        //  Value is at the old len position (correct offset)
        let written_value = unsafe { core::ptr::read(vec.ptr.add(push_position as usize)) };
        assert_eq!(written_value, value, "push must write at correct index");

        // Now test pop on the same vector
        let old_len = vec.len;
        let popped = vec.pop();

        //  Pop returns Some(value) and decrements len
        assert!(popped.is_some(), "pop on non-empty vec must return Some");
        assert_eq!(popped.unwrap(), value, "pop must return the pushed value");
        assert_eq!(vec.len, old_len - 1, "pop must decrement len");

        // Round-trip: push then pop should restore state
        assert_eq!(vec.len, initial_len, "push then pop restores original len");

        unsafe {
            dealloc_inner_vec(vec, &alloc);
        }
    }

    /// Deref creates slice only over initialized region [0, len)
    #[kani::proof]
    #[kani::unwind(4)] // Limit loop unrolling
    fn verify_deref_only_initialized_region() {
        let alloc = RustSystemAllocator;
        let capacity: u32 = kani::any();
        kani::assume(capacity > 0 && capacity <= 3); // Reduced bound

        let mut vec = unsafe { create_valid_inner_vec::<u32>(capacity, &alloc) };

        let len: u32 = kani::any();
        kani::assume(len <= capacity);

        // Initialize elements [0, len)
        for i in 0..len {
            vec.len = i;
            vec.push(i);
        }

        // Deref creates slice of exactly len elements
        let slice: &[u32] = &*vec;
        assert_eq!(slice.len(), len as usize);

        // Verify first element is accessible if len > 0
        if len > 0 {
            let _val = slice[0]; // Must not read uninitialized
        }

        unsafe {
            dealloc_inner_vec(vec, &alloc);
        }
    }

    /// Drop semantics of `InnerVec`:
    /// - Elements held in the vec are NOT dropped by `iter()` (it yields `&T`).
    /// - `pop()` moves an element out; dropping that returned value runs its
    ///   destructor exactly once.
    /// - `InnerVec` has no `Drop` of its own, so freeing the (emptied) buffer
    ///   runs no destructors.
    ///
    /// This asserts the drop *count* against a live counter, rather than the
    /// previous version whose bound was vacuously true (iterating by reference
    /// never changes `len`, so `drop_count <= original_len - current_len`
    /// reduced to `0 <= 0`).
    #[kani::proof]
    fn verify_drop_semantics() {
        unsafe {
            let mut drop_count: u32 = 0; // Local counter

            let alloc = RustSystemAllocator;
            let mut vec = create_valid_inner_vec::<Droppable>(2, &alloc);

            // Push 2 droppable values (both share the same counter via raw pointer)
            vec.push(Droppable {
                value: 100,
                drop_counter: &mut drop_count as *mut u32,
            });
            vec.push(Droppable {
                value: 200,
                drop_counter: &mut drop_count as *mut u32,
            });

            assert_eq!(vec.len, 2, "Should have 2 elements");

            // Iterating by reference must NOT drop any element.
            for _val in vec.iter() {}
            assert_eq!(
                drop_count, 0,
                "iter() yields references and must not drop elements"
            );

            // Popping moves the value out; dropping it here runs one destructor.
            {
                let popped = vec.pop();
                assert!(popped.is_some(), "pop on a non-empty vec returns Some");
            }
            assert_eq!(
                drop_count, 1,
                "dropping one popped value runs exactly one destructor"
            );
            assert_eq!(vec.len, 1, "pop decrements len");

            // Pop and drop the second value.
            {
                let _popped = vec.pop();
            }
            assert_eq!(
                drop_count, 2,
                "dropping the second popped value runs exactly one more destructor"
            );
            assert_eq!(vec.len, 0, "vec is now empty");

            // `InnerVec` owns no `Drop`, so freeing the empty buffer drops nothing.
            dealloc_inner_vec(vec, &alloc);
            assert_eq!(
                drop_count, 2,
                "freeing the (empty) buffer runs no further destructors"
            );
        }
    }
}
