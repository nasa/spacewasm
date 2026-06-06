use crate::{Allocator, GlobalAllocator};
use core::alloc::Layout;

pub struct Memory {
    ptr: *mut u8,
    size: usize,
}

#[derive(Debug)]
pub enum MemoryError {
    OutOfBounds,
    OutOfMemory,
}

impl Memory {
    pub fn new(size: usize) -> Memory {
        Memory::from(
            unsafe { GlobalAllocator.alloc(Layout::from_size_align(size, 16).unwrap()) }.unwrap(),
            size,
        )
    }

    pub fn from(ptr: *mut u8, size: usize) -> Memory {
        unsafe { ptr.write_bytes(0, size) };
        Memory { ptr, size }
    }

    #[inline]
    fn check_in_bounds(&self, addr: usize, size: usize) -> Result<(), MemoryError> {
        if size > self.size || addr > self.size - size {
            Err(MemoryError::OutOfBounds)
        } else {
            Ok(())
        }
    }

    pub fn store_u8(&self, addr: usize, i: u8) -> Result<(), MemoryError> {
        self.check_in_bounds(addr, 1)?;
        unsafe {
            self.ptr.offset(addr as isize).write(i);
        }
        Ok(())
    }

    pub fn store_u16(&self, addr: usize, i: u16) -> Result<(), MemoryError> {
        self.check_in_bounds(addr, 2)?;
        unsafe {
            self.ptr
                .offset(addr as isize)
                .cast::<u16>()
                .write_unaligned(i);
        }
        Ok(())
    }

    pub fn store_u32(&self, addr: usize, i: u32) -> Result<(), MemoryError> {
        self.check_in_bounds(addr, 4)?;
        unsafe {
            self.ptr
                .offset(addr as isize)
                .cast::<u32>()
                .write_unaligned(i);
        }
        Ok(())
    }

    pub fn store_u64(&self, addr: usize, i: u64) -> Result<(), MemoryError> {
        self.check_in_bounds(addr, 8)?;
        unsafe {
            self.ptr
                .offset(addr as isize)
                .cast::<u64>()
                .write_unaligned(i);
        }
        Ok(())
    }

    pub fn store(&self, addr: usize, data: &[u8]) -> Result<(), MemoryError> {
        self.check_in_bounds(addr, data.len())?;

        unsafe {
            data.as_ptr()
                .copy_to(self.ptr.offset(addr as isize), data.len());
        }
        Ok(())
    }

    pub fn load_u8(&self, addr: usize) -> Result<u8, MemoryError> {
        self.check_in_bounds(addr, 1)?;
        unsafe { Ok(self.ptr.offset(addr as isize).read()) }
    }

    pub fn load_u16(&self, addr: usize) -> Result<u16, MemoryError> {
        self.check_in_bounds(addr, 2)?;
        unsafe {
            Ok(self
                .ptr
                .offset(addr as isize)
                .cast::<u16>()
                .read_unaligned())
        }
    }

    pub fn load_u32(&self, addr: usize) -> Result<u32, MemoryError> {
        self.check_in_bounds(addr, 4)?;
        unsafe {
            Ok(self
                .ptr
                .offset(addr as isize)
                .cast::<u32>()
                .read_unaligned())
        }
    }

    pub fn load_u64(&self, addr: usize) -> Result<u64, MemoryError> {
        self.check_in_bounds(addr, 8)?;
        unsafe {
            Ok(self
                .ptr
                .offset(addr as isize)
                .cast::<u64>()
                .read_unaligned())
        }
    }

    pub fn load(&self, addr: usize, len: usize) -> Result<&[u8], MemoryError> {
        self.check_in_bounds(addr, len)?;
        Ok(unsafe { core::slice::from_raw_parts(self.ptr.offset(addr as isize), len) })
    }

    /// Grow the memory by n pages
    /// If the memory growth succeeds, return the old number of pages
    pub fn grow(&self, n: u32) -> Result<u32, MemoryError> {
        let _ = n;
        Err(MemoryError::OutOfMemory)
    }

    pub fn size(&self) -> u32 {
        (self.size / 65536) as u32
    }
}

impl Drop for Memory {
    fn drop(&mut self) {
        unsafe {
            GlobalAllocator.dealloc(self.ptr, Layout::from_size_align(self.size, 16).unwrap());
        }
    }
}

#[cfg(kani)]
mod kani_proofs {
    use super::*;
    use crate::StaticAllocator;

    #[kani::proof]
    fn proof_store_load_correctness() {
        let alloc = StaticAllocator::<256, 8>::new();
        let size = 64;

        unsafe {
            let ptr = alloc.alloc(Layout::from_size_align(size, 16).unwrap()).unwrap();
            let mem = Memory::from(ptr, size);

            // Test all integer sizes with symbolic values and addresses
            let addr: usize = kani::any();
            kani::assume(addr <= size - 8);  // Reserve space for largest type (u64)

            // Test u8 store/load
            let val_u8: u8 = kani::any();
            mem.store_u8(addr, val_u8).unwrap();
            assert_eq!(mem.load_u8(addr).unwrap(), val_u8, "u8 round-trip failed");

            // Test u16 store/load
            let val_u16: u16 = kani::any();
            mem.store_u16(addr, val_u16).unwrap();
            assert_eq!(mem.load_u16(addr).unwrap(), val_u16, "u16 round-trip failed");

            // Test u32 store/load
            let val_u32: u32 = kani::any();
            mem.store_u32(addr, val_u32).unwrap();
            assert_eq!(mem.load_u32(addr).unwrap(), val_u32, "u32 round-trip failed");

            // Test u64 store/load
            let val_u64: u64 = kani::any();
            mem.store_u64(addr, val_u64).unwrap();
            assert_eq!(mem.load_u64(addr).unwrap(), val_u64, "u64 round-trip failed");

            // Test out-of-bounds detection
            assert!(mem.store_u32(size - 2, 0).is_err(), "Out-of-bounds store must fail");
            assert!(mem.store_u32(size, 0).is_err(), "Store at boundary must fail");
            assert!(mem.load_u32(size - 2).is_err(), "Out-of-bounds load must fail");
            assert!(mem.load_u32(size).is_err(), "Load at boundary must fail");

            // Test addresses that cause overflow in bounds check
            let overflow_addr = usize::MAX - 1;
            assert!(mem.store_u32(overflow_addr, 0).is_err(), "Overflow address must be rejected");
            assert!(mem.load_u32(overflow_addr).is_err(), "Overflow address must be rejected");

            // Verify safe addresses have lossless isize conversion
            if addr <= isize::MAX as usize {
                let offset = addr as isize;
                assert!(offset >= 0, "Valid addr converts to non-negative offset");
                assert!(offset as usize == addr, "isize conversion must be lossless");
            }

            // Prevent Drop from calling GlobalAllocator FFI
            core::mem::forget(mem);
            alloc.dealloc(ptr, Layout::from_size_align(size, 16).unwrap());
        }
    }

    #[kani::proof]
    fn proof_byte_slice_operations() {
        let alloc = StaticAllocator::<128, 8>::new();
        let size = 16;

        let ptr = unsafe { alloc.alloc(Layout::from_size_align(size, 16).unwrap()).unwrap() };
        let mem = Memory::from(ptr, size);

        // Test zero initialization at one symbolic address
        let zero_addr: usize = kani::any();
        kani::assume(zero_addr < size);
        assert_eq!(mem.load_u8(zero_addr).unwrap(), 0, "Memory must be zero-initialized");

        // Test fixed-size byte slice (4 bytes) at symbolic address
        let addr: usize = kani::any();
        kani::assume(addr <= size - 4);

        // Use symbolic 4-byte array
        let data: [u8; 4] = kani::any();

        // Store and load back
        mem.store(addr, &data).unwrap();
        let loaded = mem.load(addr, 4).unwrap();
        assert_eq!(loaded, &data, "Byte slice round-trip failed");

        // Test empty slice
        mem.store(0, &[]).unwrap();
        assert_eq!(mem.load(0, 0).unwrap().len(), 0, "Empty slice must work");

        // Test out-of-bounds slice access
        assert!(mem.store(size - 2, &[1, 2, 3, 4]).is_err(), "Out-of-bounds store must fail");
        assert!(mem.load(size - 2, 4).is_err(), "Out-of-bounds load must fail");

        // Prevent Drop from calling GlobalAllocator FFI
        core::mem::forget(mem);
        unsafe { alloc.dealloc(ptr, Layout::from_size_align(size, 16).unwrap()) };
    }

}
