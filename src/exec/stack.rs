use crate::{Allocator, GlobalAllocator};
use core::alloc::Layout;

pub struct Stack {
    ptr: *mut u32,
    size: usize,
}

impl Stack {
    pub fn new(size: usize) -> Self {
        Stack {
            ptr: unsafe {
                GlobalAllocator
                    .alloc(Layout::from_size_align(size * 4, 4).unwrap())
                    .unwrap()
                    .cast()
            },
            size,
        }
    }

    #[inline]
    fn check_bounds(&self, addr: usize, word_n: usize) {
        // TODO(tumbar) Allow this assertion to be disabled via feature flag. We can already check
        //              for stack overflow at the callsite so this simply verifies implementation
        //              correctness. This is good during development/security fuzzing but not useful
        //              during runtime.
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
