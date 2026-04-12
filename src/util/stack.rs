use crate::alloc::{AllocError, Allocator};
use core::alloc::Layout;
use core::cell::UnsafeCell;

#[repr(align(128))]
pub struct StackAllocator<const SIZE: usize, const DEPTH: usize = 8> {
    inner: UnsafeCell<StackAllocatorInner<SIZE, DEPTH>>,
}

struct StackAllocatorInner<const SIZE: usize, const DEPTH: usize> {
    data: [u8; SIZE],
    allocated: usize,
    allocations: [usize; DEPTH],
    n_allocations: usize,
}

impl<const SIZE: usize, const DEPTH: usize> StackAllocator<SIZE, DEPTH> {
    pub fn new() -> Self {
        StackAllocator {
            inner: UnsafeCell::new(StackAllocatorInner {
                data: [0; SIZE],
                allocated: 0,
                allocations: [0; DEPTH],
                n_allocations: 0,
            }),
        }
    }
}

unsafe impl<const SIZE: usize, const DEPTH: usize> Allocator for StackAllocator<SIZE, DEPTH> {
    unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
        unsafe { (*self.inner.get()).alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { (*self.inner.get()).dealloc(ptr, layout) }.unwrap()
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
            if ptr as usize != expected_address {
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
