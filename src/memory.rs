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
        if addr + size > self.size {
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
