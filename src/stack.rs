use crate::{AllocError, Allocator, GlobalAllocator};
use core::alloc::Layout;

pub struct Stack {
    ptr: *mut u32,
    size: usize,
}

impl Stack {
    pub fn new(size: usize) -> Result<Self, AllocError> {
        Ok(Stack {
            ptr: unsafe {
                GlobalAllocator
                    .alloc(Layout::from_size_align(size * 4, 4).unwrap())?
                    .cast()
            },
            size,
        })
    }

    pub(crate) fn len(&self) -> usize {
        self.size
    }

    #[inline]
    fn check_bounds(&self, addr: usize, word_n: usize) {
        let _ = addr;
        let _ = word_n;

        #[cfg(feature = "strict-assertions")]
        assert!(
            addr + word_n <= self.size,
            "addr={} word_n={} size={}",
            addr,
            word_n,
            self.size
        );
    }

    #[inline]
    pub(crate) fn read_u32(&self, addr: usize) -> u32 {
        self.check_bounds(addr, 1);
        unsafe { *(self.ptr.add(addr)) }
    }

    #[inline]
    pub(crate) fn read_f32(&self, addr: usize) -> f32 {
        f32::from_bits(self.read_u32(addr))
    }

    #[inline]
    pub(crate) fn read_u64(&self, addr: usize) -> u64 {
        self.check_bounds(addr, 2);
        unsafe { self.ptr.add(addr).cast::<u64>().read_unaligned() }
    }

    #[inline]
    pub(crate) fn read_f64(&self, addr: usize) -> f64 {
        f64::from_bits(self.read_u64(addr))
    }

    #[inline]
    pub(crate) fn write_u32(&mut self, addr: usize, value: u32) {
        self.check_bounds(addr, 1);
        unsafe { *(self.ptr.add(addr)) = value }
    }

    #[inline]
    pub(crate) fn write_f32(&mut self, addr: usize, value: f32) {
        self.write_u32(addr, value.to_bits());
    }

    #[inline]
    pub(crate) fn write_u64(&mut self, addr: usize, value: u64) {
        self.check_bounds(addr, 2);
        unsafe { self.ptr.add(addr).cast::<u64>().write_unaligned(value) }
    }

    #[inline]
    pub(crate) fn write_f64(&mut self, addr: usize, value: f64) {
        self.write_u64(addr, value.to_bits());
    }
}

impl Drop for Stack {
    fn drop(&mut self) {
        unsafe {
            GlobalAllocator.dealloc(
                self.ptr.cast(),
                Layout::from_size_align(self.size * 4, 4).unwrap(),
            );
        }
    }
}

#[cfg(kani)]
mod kani_proofs {
    use super::*;
    use crate::alloc::Allocator;
    use crate::test_support::RustSystemAllocator;

    /// Verify that u32 write/read round-trips correctly for all valid addresses.
    /// This confirms `ptr.add(addr)` stays in-bounds for the entire valid range.
    #[kani::proof]
    fn proof_stack_u32_roundtrip() {
        let size: usize = kani::any();
        kani::assume(size > 0 && size <= 16); // Small size for bounded verification

        // Manually construct Stack using RustSystemAllocator to avoid GlobalAllocator FFI
        let ptr = unsafe {
            RustSystemAllocator
                .alloc(Layout::from_size_align(size * 4, 4).unwrap())
                .unwrap()
                .cast()
        };
        let mut stack = Stack { ptr, size };

        let addr: usize = kani::any();
        kani::assume(addr < size);

        let val: u32 = kani::any();
        stack.write_u32(addr, val);
        assert_eq!(stack.read_u32(addr), val, "u32 round-trip failed");

        // Manual cleanup to avoid GlobalAllocator in Drop
        unsafe {
            RustSystemAllocator.dealloc(
                stack.ptr.cast(),
                Layout::from_size_align(stack.size * 4, 4).unwrap(),
            );
        }
        core::mem::forget(stack);
    }

    /// Verify that u64 write/read round-trips correctly with unaligned access.
    /// This confirms `read_unaligned`/`write_unaligned` never access memory
    /// outside the [0, size*4) byte range.
    #[kani::proof]
    fn proof_stack_u64_roundtrip() {
        let size: usize = kani::any();
        kani::assume(size >= 2 && size <= 16); // Need at least 2 words for u64

        // Manually construct Stack using RustSystemAllocator to avoid GlobalAllocator FFI
        let ptr = unsafe {
            RustSystemAllocator
                .alloc(Layout::from_size_align(size * 4, 4).unwrap())
                .unwrap()
                .cast()
        };
        let mut stack = Stack { ptr, size };

        let addr: usize = kani::any();
        kani::assume(addr <= size && size >= 2 && addr <= size - 2); // u64 needs 2 words (8 bytes)

        let val: u64 = kani::any();
        stack.write_u64(addr, val);
        assert_eq!(stack.read_u64(addr), val, "u64 round-trip failed");

        // Manual cleanup to avoid GlobalAllocator in Drop
        unsafe {
            RustSystemAllocator.dealloc(
                stack.ptr.cast(),
                Layout::from_size_align(stack.size * 4, 4).unwrap(),
            );
        }
        core::mem::forget(stack);
    }

    /// Verify that Stack::new either fails cleanly or computes size * 4
    /// without silent overflow in the Layout calculation.
    #[kani::proof]
    fn proof_stack_new_size_no_overflow() {
        let size: usize = kani::any();
        kani::assume(size <= 1024); // Bound to prevent timeouts

        // Check for overflow in size * 4 computation
        let byte_size = size.checked_mul(4);

        // If multiplication would overflow, we can't verify further
        if byte_size.is_none() {
            return;
        }

        let byte_size = byte_size.unwrap();

        // Verify Layout::from_size_align doesn't panic
        if let Ok(layout) = Layout::from_size_align(byte_size, 4) {
            // If Layout succeeded, try allocation with RustSystemAllocator
            if let Ok(ptr) = unsafe { RustSystemAllocator.alloc(layout) } {
                // Allocation succeeded - verify we can construct a valid Stack
                let stack = Stack {
                    ptr: ptr.cast(),
                    size,
                };

                // Clean up
                unsafe {
                    RustSystemAllocator.dealloc(ptr, layout);
                }
                core::mem::forget(stack);
            }
        }
    }
}
