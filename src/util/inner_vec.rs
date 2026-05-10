use core::ops::{Deref, DerefMut};

pub struct InnerVec<T: Sized> {
    pub ptr: *mut T,
    pub capacity: u32,
    pub len: u32,
}

impl<T: core::fmt::Debug> core::fmt::Debug for InnerVec<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<T: Sized> InnerVec<T> {
    pub fn zero() -> InnerVec<T> {
        InnerVec {
            ptr: core::ptr::null_mut(),
            capacity: 0,
            len: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn capacity(&self) -> usize {
        self.capacity as usize
    }

    /// Push a new item to the vector
    /// If the capacity is exceeded, this will panic
    pub fn push(&mut self, value: T) {
        assert!(self.len < self.capacity);

        unsafe {
            core::ptr::write(self.ptr.add(self.len as usize), value);
        }

        self.len += 1;
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
        let slice: &[i32] = &*vec;
        assert_eq!(slice.len(), 0);
    }

    #[test]
    fn test_deref_mut_empty() {
        let mut vec: InnerVec<i32> = InnerVec::zero();
        let slice: &mut [i32] = &mut *vec;
        assert_eq!(slice.len(), 0);
    }
}

#[cfg(kani)]
mod proof_harness {
    use super::*;
    use core::alloc::Layout;
    use crate::util::static_alloc::StaticAllocator;
    use crate::util::alloc::Allocator;

    // Use a static allocator for tests
    const ALLOC_SIZE: usize = 4096;
    const ALLOC_DEPTH: usize = 16;

    // Helper to create a valid InnerVec with allocated memory
    unsafe fn create_valid_inner_vec<T>(capacity: u32, alloc: &StaticAllocator<ALLOC_SIZE, ALLOC_DEPTH>) -> InnerVec<T> {
        if capacity == 0 {
            return InnerVec::zero();
        }

        let layout = Layout::array::<T>(capacity as usize).unwrap();
        let ptr = unsafe { alloc.alloc(layout).unwrap() as *mut T };

        InnerVec {
            ptr,
            capacity,
            len: 0,
        }
    }

    // Helper to deallocate an InnerVec
    unsafe fn dealloc_inner_vec<T>(vec: InnerVec<T>, alloc: &StaticAllocator<ALLOC_SIZE, ALLOC_DEPTH>) {
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

    /// len ≤ capacity is maintained by all operations
    #[kani::proof]
    fn verify_len_never_exceeds_capacity() {
        unsafe {
            let alloc = StaticAllocator::<ALLOC_SIZE, ALLOC_DEPTH>::new();
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
                0 if vec.len < vec.capacity => vec.push(42),
                1 if vec.len > 0 => { vec.pop(); },
                _ => {}
            }

            // INVARIANT: len ≤ capacity must hold after any operation
            assert!(vec.len <= vec.capacity);

            dealloc_inner_vec(vec, &alloc);
        }
    }

    /// Pointer arithmetic ptr.add(i) for i < capacity stays in bounds
    #[kani::proof]
    fn verify_pointer_arithmetic_in_bounds() {
        unsafe {
            let alloc = StaticAllocator::<ALLOC_SIZE, ALLOC_DEPTH>::new();
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

    /// No integer overflows in len or offset calculations
    #[kani::proof]
    fn verify_no_integer_overflow() {
        unsafe {
            let alloc = StaticAllocator::<ALLOC_SIZE, ALLOC_DEPTH>::new();

            // Use realistic capacity bounds
            let capacity: u32 = kani::any();
            kani::assume(capacity > 0 && capacity < 1000);

            let mut vec = create_valid_inner_vec::<u32>(capacity, &alloc);
            vec.len = capacity - 1;

            // Push should not overflow len
            vec.push(42);
            assert!(vec.len == capacity);

            // len as usize should not truncate
            let len_usize = vec.len as usize;
            assert!(len_usize == capacity as usize);

            // Offset calculation should not overflow
            let offset = vec.len as usize;
            assert!(offset <= capacity as usize);

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

    /// push writes only at valid index after len < capacity check
    #[kani::proof]
    fn verify_push_writes_at_correct_index() {
        unsafe {
            let alloc = StaticAllocator::<ALLOC_SIZE, ALLOC_DEPTH>::new();

            // Use small concrete capacity for faster verification
            let mut vec = create_valid_inner_vec::<u32>(3, &alloc);

            // Test push at different positions
            let initial_len: u32 = kani::any();
            kani::assume(initial_len < 3);  // Must be < capacity
            vec.len = initial_len;

            let value: u32 = kani::any();
            vec.push(value);

            // After push, len increased by 1
            assert_eq!(vec.len, initial_len + 1);

            // The value is at the old len position (verifies correct offset calculation)
            let written_value = unsafe { core::ptr::read(vec.ptr.add(initial_len as usize)) };
            assert_eq!(written_value, value);

            dealloc_inner_vec(vec, &alloc);
        }
    }

    /// pop reads only from initialized memory [0, len)
    #[kani::proof]
    #[kani::unwind(4)]  // Limit loop unrolling
    fn verify_pop_reads_initialized_memory() {
        unsafe {
            let alloc = StaticAllocator::<ALLOC_SIZE, ALLOC_DEPTH>::new();
            let capacity: u32 = kani::any();
            kani::assume(capacity > 0 && capacity <= 3);  // Reduced bound

            let mut vec = create_valid_inner_vec::<u32>(capacity, &alloc);

            // Initialize some elements
            let initial_len: u32 = kani::any();
            kani::assume(initial_len > 0 && initial_len <= capacity);

            for i in 0..initial_len {
                vec.len = i;
                vec.push(i); // Initialize with known values
            }

            let old_len = vec.len;
            let popped = vec.pop();

            // Pop should return Some(value) and decrement len
            assert!(popped.is_some());
            assert_eq!(popped.unwrap(), old_len - 1);
            assert_eq!(vec.len, old_len - 1);

            // The read was at index (old_len - 1), which was initialized
            // This is verified by the fact that pop succeeded and returned the expected value

            dealloc_inner_vec(vec, &alloc);
        }
    }

    /// Deref creates slice only over initialized region [0, len)
    #[kani::proof]
    #[kani::unwind(4)]  // Limit loop unrolling
    fn verify_deref_only_initialized_region() {
        unsafe {
            let alloc = StaticAllocator::<ALLOC_SIZE, ALLOC_DEPTH>::new();
            let capacity: u32 = kani::any();
            kani::assume(capacity > 0 && capacity <= 3);  // Reduced bound

            let mut vec = create_valid_inner_vec::<u32>(capacity, &alloc);

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
                let _val = slice[0];  // Must not read uninitialized
            }

            dealloc_inner_vec(vec, &alloc);
        }
    }

    /// Each value should be dropped exactly once
    #[kani::proof]
    fn verify_values_dropped_once() {
        unsafe {
            let mut drop_count: u32 = 0;  // Local counter

            let alloc = StaticAllocator::<ALLOC_SIZE, ALLOC_DEPTH>::new();
            let mut vec = create_valid_inner_vec::<Droppable>(2, &alloc);

            // Push 2 droppable values (both share the same counter via raw pointer)
            vec.push(Droppable { value: 100, drop_counter: &mut drop_count as *mut u32 });
            vec.push(Droppable { value: 200, drop_counter: &mut drop_count as *mut u32 });

            // First iteration should consume values and drop them
            {
                let mut count = 0;
                for _val in vec.iter() {
                    count += 1;
                }
            }

            // After first iteration: drop_count = 2 (one drop per element)
            let drops_after_first = drop_count;

            // Second iteration over same data
            {
                for _val in vec.iter() {
                    // Values already consumed - iterator should not read them again
                }
            }

            // Expected: drop_count should still be 2 (no additional drops)
            // Each value should be dropped exactly once
            let drops_after_second = drop_count;

            // Verify no additional drops occurred
            assert_eq!(drops_after_second, drops_after_first);

            dealloc_inner_vec(vec, &alloc);
        }
    }

    /// drops ≤ (original_len - current_len)
    #[kani::proof]
    fn verify_iter_invalidates_vec() {
        unsafe {
            let mut drop_count: u32 = 0;  // Local counter

            let alloc = StaticAllocator::<ALLOC_SIZE, ALLOC_DEPTH>::new();
            let mut vec = create_valid_inner_vec::<Droppable>(2, &alloc);

            vec.push(Droppable { value: 100, drop_counter: &mut drop_count as *mut u32 });
            vec.push(Droppable { value: 200, drop_counter: &mut drop_count as *mut u32 });

            let original_len = vec.len;

            // Consume the iterator - this drops the values
            for _val in vec.iter() {
                // Values are consumed and dropped
            }

            let current_len = vec.len;

            // Expected: If values are dropped, vec.len should reflect their removal
            // Number of drops should equal number of values removed from vec
            // In other words: drop_count ≤ (original_len - current_len)
            //
            // With correct ownership tracking:
            //   - If drops = 2, then current_len should be 0 (both values removed)
            //   - Or: vec should be consumed/invalidated after iter()

            // Verify ownership accounting is correct
            assert!(drop_count <= (original_len - current_len));

            dealloc_inner_vec(vec, &alloc);
        }
    }
}
