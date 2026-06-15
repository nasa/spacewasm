use crate::alloc::{AllocError, Allocator};
use core::alloc::Layout;
use core::cell::UnsafeCell;

#[repr(align(128))]
pub struct StaticAllocator<const SIZE: usize, const DEPTH: usize = 8> {
    inner: UnsafeCell<StackAllocatorInner<SIZE, DEPTH>>,
}

struct StackAllocatorInner<const SIZE: usize, const DEPTH: usize> {
    data: [u8; SIZE],
    allocated: usize,
    allocations: [usize; DEPTH],
    n_allocations: usize,
}

impl<const SIZE: usize, const DEPTH: usize> StaticAllocator<SIZE, DEPTH> {
    pub const fn new() -> Self {
        StaticAllocator {
            inner: UnsafeCell::new(StackAllocatorInner {
                data: [0; SIZE],
                allocated: 0,
                allocations: [0; DEPTH],
                n_allocations: 0,
            }),
        }
    }
}

// Note: Kani has issues with generic const parameters in trait impls.
// This impl works fine in regular Rust but causes Kani verification to fail
// when analyzing code that doesn't use concrete type parameters.
#[cfg(not(kani))]
unsafe impl<const SIZE: usize, const DEPTH: usize> Allocator for StaticAllocator<SIZE, DEPTH> {
    unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
        unsafe { (*self.inner.get()).alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { (*self.inner.get()).dealloc(ptr, layout) }.unwrap()
    }

    fn memory_statistics(&self) -> crate::MemoryStatistics {
        let inner = unsafe { &*self.inner.get() };
        crate::MemoryStatistics {
            total_bytes: inner.allocated as i32,
            pad_bytes: 0,
        }
    }
}


impl<const SIZE: usize, const DEPTH: usize> StackAllocatorInner<SIZE, DEPTH> {
    fn alloc(&mut self, layout: Layout) -> Result<*mut u8, AllocError> {
        // This stack is manually aligned to 128, we don't support more than that since
        // addresses are computed as offsets of the start of this data segment
        if layout.align() > 128 {
            return Err(AllocError::InvalidAlignment);
        }

        if self.n_allocations >= DEPTH {
            return Err(AllocError::StackAllocationTooDeep);
        }

        let mut start_address = self.allocated;
        if start_address % layout.align() > 0 {
            let alignment_offset = layout.align() - start_address % layout.align();
            start_address += alignment_offset;
        }

        let final_address = start_address + layout.size();
        if final_address <= SIZE {
            // Track this allocation
            self.allocations[self.n_allocations] = start_address;
            self.n_allocations += 1;

            self.allocated = final_address;
            Ok(&raw mut self.data[start_address])
        } else {
            Err(AllocError::OutOfMemory)
        }
    }

    fn dealloc(&mut self, ptr: *mut u8, layout: Layout) -> Result<(), AllocError> {
        let _ = layout;

        if self.n_allocations > 0 {
            self.n_allocations -= 1;
            let expected_address = self.allocations[self.n_allocations];
            let base_address = &raw const self.data[0] as usize;
            if base_address > ptr as usize {
                return Err(AllocError::StackDeallocationInvariantViolation);
            }

            let ptr_offset = ptr as usize - base_address;

            if ptr_offset != expected_address {
                Err(AllocError::StackDeallocationInvariantViolation)
            } else {
                // FIXME(tumbar)
                //   We could technically go back slightly further since this allocation
                //   may have imposed some alignment bytes. We are not tracking this information
                //   at the moment...
                //   We could potentially use lower bits in the expected_address to encode this
                self.allocated = expected_address;
                Ok(())
            }
        } else {
            Err(AllocError::StackDeallocationInvariantViolation)
        }
    }
}

#[cfg(kani)]
pub mod kani_proofs {
    use super::*;

    // Macro to generate concrete Allocator implementations for specific size combinations.
    // Needed because Kani has issues with generic const parameters.
    macro_rules! impl_allocator_for_size {
        ($size:expr, $depth:expr) => {
            unsafe impl Allocator for StaticAllocator<$size, $depth> {
                unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
                    unsafe { (*self.inner.get()).alloc(layout) }
                }

                unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
                    unsafe { (*self.inner.get()).dealloc(ptr, layout) }.unwrap()
                }

                fn memory_statistics(&self) -> crate::MemoryStatistics {
                    let inner = unsafe { &*self.inner.get() };
                    crate::MemoryStatistics {
                        total_bytes: inner.allocated as i32,
                        pad_bytes: 0,
                    }
                }
            }
        };
    }

    /// FixedSizeAllocator: Non-generic allocator wrapper for use in Kani proofs.
    /// This avoids the need for concrete implementations of every size combination.
    /// Uses a fixed-size StaticAllocator<4096, 8> internally, which is large enough for all tests.
    pub struct FixedSizeAllocator {
        inner: StaticAllocator<4096, 8>,
    }

    impl FixedSizeAllocator {
        pub const fn new() -> Self {
            Self {
                inner: StaticAllocator::new(),
            }
        }
    }

    unsafe impl Allocator for FixedSizeAllocator {
        unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
            unsafe { self.inner.alloc(layout) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { self.inner.dealloc(ptr, layout) }
        }

        fn memory_statistics(&self) -> crate::MemoryStatistics {
            self.inner.memory_statistics()
        }
    }

    // Concrete implementations needed for static_alloc.rs's own Kani proofs
    impl_allocator_for_size!(128, 8);
    impl_allocator_for_size!(512, 8);
    impl_allocator_for_size!(1024, 3);
    impl_allocator_for_size!(1024, 8);
    // Concrete impl needed for FixedSizeAllocator's inner StaticAllocator
    impl_allocator_for_size!(4096, 8);

    /// No overlapping allocations
    /// LIFO deallocation enforcement
    /// Allocation counter consistency
    /// Allocated pointer monotonicity
    #[kani::proof]
    fn proof_alloc() {
        let alloc = StaticAllocator::<1024, 8>::new();
        unsafe {
            let layout = Layout::from_size_align(16, 8).unwrap();

            // Initial state
            let inner0 = &*alloc.inner.get();
            assert!(inner0.n_allocations == 0, "Initial counter must be 0");
            let allocated0 = inner0.allocated;

            // First allocation
            let ptr1 = alloc.alloc(layout).unwrap();
            let inner1 = &*alloc.inner.get();
            let base = &raw const inner1.data[0] as usize;
            let offset1 = ptr1 as usize - base;

            // Counter incremented
            assert!(inner1.n_allocations == 1, "Counter must increment after alloc");

            // Monotonicity
            assert!(
                inner1.allocated >= allocated0,
                "Allocated must be monotonic during alloc"
            );

            let allocated1 = inner1.allocated;

            // Second allocation
            let ptr2 = alloc.alloc(layout).unwrap();
            let inner2 = &*alloc.inner.get();
            let offset2 = ptr2 as usize - base;

            // Counter incremented again
            assert!(inner2.n_allocations == 2, "Counter must increment after second alloc");

            // Monotonicity
            assert!(
                inner2.allocated >= allocated1,
                "Allocated must be monotonic during second alloc"
            );

            // No overlap - ptr2 must start after ptr1 ends
            assert!(
                offset2 >= offset1 + layout.size(),
                "Second allocation must not overlap first"
            );

            // Test LIFO enforcement - try wrong order deallocation
            let inner_test = &mut *alloc.inner.get();
            let wrong_order_result = inner_test.dealloc(ptr1, layout);
            assert!(
                matches!(wrong_order_result, Err(AllocError::StackDeallocationInvariantViolation)),
                "Must reject out-of-order deallocation"
            );
            // Note: After failed dealloc, allocator state is corrupted (n_allocations decremented)
            // This is OK because public API panics on error. Reset for correct test:

            // Correct LIFO deallocation order
            let alloc2 = StaticAllocator::<1024, 8>::new();
            let ptr1b = alloc2.alloc(layout).unwrap();
            let ptr2b = alloc2.alloc(layout).unwrap();

            alloc2.dealloc(ptr2b, layout);
            let inner3 = &*alloc2.inner.get();
            // Counter decremented
            assert!(inner3.n_allocations == 1, "Counter must decrement after dealloc");

            alloc2.dealloc(ptr1b, layout);
            let inner4 = &*alloc2.inner.get();
            // Counter back to 0
            assert!(inner4.n_allocations == 0, "Counter must be 0 after all deallocs");
        }
    }

    #[kani::proof]
    fn proof_out_of_memory() {
        let alloc = StaticAllocator::<128, 8>::new();
        unsafe {
            let layout = Layout::from_size_align(100, 8).unwrap();
            let _ptr1 = alloc.alloc(layout).unwrap();
            let result = alloc.alloc(layout);
            assert!(matches!(result, Err(AllocError::OutOfMemory)));
        }
    }

    /// Allocation depth must not exceed DEPTH
    #[kani::proof]
    fn proof_depth_limit() {
        let alloc = StaticAllocator::<1024, 3>::new(); // DEPTH=3
        unsafe {
            let layout = Layout::from_size_align(16, 8).unwrap();

            // Allocate up to DEPTH (3) successfully
            let _ptr1 = alloc.alloc(layout).unwrap();
            let inner1 = &*alloc.inner.get();
            assert!(inner1.n_allocations == 1, "Counter must be 1");

            let _ptr2 = alloc.alloc(layout).unwrap();
            let inner2 = &*alloc.inner.get();
            assert!(inner2.n_allocations == 2, "Counter must be 2");

            let _ptr3 = alloc.alloc(layout).unwrap();
            let inner3 = &*alloc.inner.get();
            assert!(inner3.n_allocations == 3, "Counter must be 3");

            // Fourth allocation must fail with StackAllocationTooDeep
            let result = alloc.alloc(layout);
            assert!(
                matches!(result, Err(AllocError::StackAllocationTooDeep)),
                "Must reject allocation beyond DEPTH"
            );

            // Counter should not have incremented
            let inner4 = &*alloc.inner.get();
            assert!(
                inner4.n_allocations == 3,
                "Counter must not increment on failed alloc"
            );
        }
    }

    /// Pointer alignment calculations
    #[kani::proof]
    fn proof_alignment_correctness() {
        let alloc = StaticAllocator::<1024, 8>::new();

        unsafe {
            // Test all valid power-of-2 alignments up to 128
            let align: usize = kani::any();
            kani::assume(align > 0 && align <= 128);
            kani::assume(align.is_power_of_two());

            // Symbolic size for the allocation
            let size: usize = kani::any();
            kani::assume(size > 0 && size <= 256);

            let layout = Layout::from_size_align(size, align).unwrap();

            // Perform allocation
            match alloc.alloc(layout) {
                Ok(ptr) => {
                    let ptr_addr = ptr as usize;

                    // Verify pointer is aligned
                    assert!(
                        ptr_addr % align == 0,
                        "Returned pointer must be aligned to requested alignment"
                    );

                    // Verify pointer is within buffer bounds
                    let inner = &*alloc.inner.get();
                    let base_addr = &raw const inner.data[0] as usize;
                    assert!(ptr_addr >= base_addr, "Pointer must be >= base address");
                    assert!(
                        ptr_addr < base_addr + 1024,
                        "Pointer must be within buffer"
                    );

                    // Verify allocated pointer hasn't exceeded buffer
                    assert!(inner.allocated <= 1024, "Allocated must not exceed SIZE");
                }
                Err(e) => {
                    // Errors are acceptable (OOM, alignment too large, etc.)
                    assert!(
                        matches!(
                            e,
                            AllocError::OutOfMemory
                                | AllocError::InvalidAlignment
                                | AllocError::StackAllocationTooDeep
                        ),
                        "Only valid allocation errors allowed"
                    );
                }
            }
        }
    }

    /// Alignment padding calculation
    /// Padding calculation correctness
    /// No integer overflow in pointer arithmetic
    #[kani::proof]
    fn proof_alignment_padding_calculation() {
        unsafe {
            // Create allocator with some existing allocations to test misaligned starts
            let alloc = StaticAllocator::<512, 8>::new();

            // Make a small allocation to create misalignment
            let layout1 = Layout::from_size_align(7, 1).unwrap();
            let _ = alloc.alloc(layout1);

            // Now allocated = 7 (misaligned for larger alignments)

            // Test alignment with various alignments
            let align: usize = kani::any();
            kani::assume(align > 0 && align <= 128);
            kani::assume(align.is_power_of_two());

            let size: usize = kani::any();
            kani::assume(size > 0 && size <= 100);

            let layout = Layout::from_size_align(size, align).unwrap();

            let inner = &mut *alloc.inner.get();
            let start_before = inner.allocated;

            match inner.alloc(layout) {
                Ok(ptr) => {
                    let ptr_addr = ptr as usize;
                    let base_addr = &raw const inner.data[0] as usize;
                    let ptr_offset = ptr_addr - base_addr;

                    // Verify the alignment padding calculation was correct
                    // If start_before was misaligned, padding should have been added
                    if start_before % align != 0 {
                        let expected_padding = align - (start_before % align);

                        assert!(
                            start_before.checked_add(expected_padding).is_some(),
                            "Adding padding must not overflow"
                        );
                        assert!(
                            start_before + expected_padding <= 512,
                            "Start address with padding must be in bounds"
                        );

                        assert!(
                            ptr_offset == start_before + expected_padding,
                            "Padding calculation must be correct"
                        );
                        assert!(
                            expected_padding < align,
                            "Alignment padding must be < align"
                        );
                    } else {
                        assert!(
                            ptr_offset == start_before,
                            "No padding needed when already aligned"
                        );
                    }

                    assert!(
                        ptr_offset.checked_add(size).is_some(),
                        "Adding size to offset must not overflow"
                    );
                    assert!(
                        ptr_offset + size <= 512,
                        "Final address must not exceed buffer size"
                    );

                    assert!(ptr_addr % align == 0, "Pointer must be aligned");

                    assert!(
                        inner.allocated <= 512,
                        "Allocated must not overflow"
                    );
                }
                Err(_) => {
                    // Allocation failure is acceptable
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alloc_dealloc() {
        let alloc = StaticAllocator::<1024, 8>::new();
        unsafe {
            let layout = Layout::from_size_align(16, 8).unwrap();
            let ptr1 = alloc.alloc(layout).unwrap();
            let ptr2 = alloc.alloc(layout).unwrap();

            alloc.dealloc(ptr2, layout);
            alloc.dealloc(ptr1, layout);
        }
    }

    #[test]
    fn test_out_of_memory() {
        let alloc = StaticAllocator::<128, 8>::new();
        unsafe {
            let layout = Layout::from_size_align(100, 8).unwrap();
            let _ptr1 = alloc.alloc(layout).unwrap();
            let result = alloc.alloc(layout);
            assert!(matches!(result, Err(AllocError::OutOfMemory)));
        }
    }

    #[test]
    fn test_too_deep() {
        let alloc = StaticAllocator::<1024, 2>::new();
        unsafe {
            let layout = Layout::from_size_align(16, 8).unwrap();
            let _ptr1 = alloc.alloc(layout).unwrap();
            let _ptr2 = alloc.alloc(layout).unwrap();
            let result = alloc.alloc(layout);
            assert!(matches!(result, Err(AllocError::StackAllocationTooDeep)));
        }
    }

    #[test]
    fn test_invalid_alignment() {
        let alloc = StaticAllocator::<1024, 8>::new();
        unsafe {
            let layout = Layout::from_size_align(16, 256).unwrap();
            let result = alloc.alloc(layout);
            assert!(matches!(result, Err(AllocError::InvalidAlignment)));
        }
    }

    #[test]
    fn test_dealloc_wrong_order() {
        let alloc = StaticAllocator::<1024, 8>::new();
        unsafe {
            let layout = Layout::from_size_align(16, 8).unwrap();
            let ptr1 = alloc.alloc(layout).unwrap();
            let _ptr2 = alloc.alloc(layout).unwrap();

            let result = (*alloc.inner.get()).dealloc(ptr1, layout);
            assert!(matches!(
                result,
                Err(AllocError::StackDeallocationInvariantViolation)
            ));
        }
    }

    #[test]
    fn test_dealloc_empty() {
        let alloc = StaticAllocator::<1024, 8>::new();
        unsafe {
            let layout = Layout::from_size_align(16, 8).unwrap();
            let ptr = core::ptr::null_mut();
            let result = (*alloc.inner.get()).dealloc(ptr, layout);
            assert!(matches!(
                result,
                Err(AllocError::StackDeallocationInvariantViolation)
            ));
        }
    }
}
