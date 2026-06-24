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

impl<const SIZE: usize, const DEPTH: usize> Default for StaticAllocator<SIZE, DEPTH> {
    fn default() -> Self {
        Self::new()
    }
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
        if !start_address.is_multiple_of(layout.align()) {
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
