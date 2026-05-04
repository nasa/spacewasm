extern crate std;

use crate::{Allocator, GlobalAllocator};
use core::alloc::Layout;

pub struct Memory {
    ptr: *mut u8,
    size: usize,
}

#[derive(Debug)]
pub struct MemoryOutOfBounds;

impl Memory {
    pub fn new(size: usize) -> Memory {
        Memory {
            ptr: unsafe { GlobalAllocator.alloc(Layout::from_size_align(size, 16).unwrap()) }
                .unwrap(),
            size,
        }
    }

    pub fn from(ptr: *mut u8, size: usize) -> Memory {
        Memory { ptr, size }
    }

    #[inline]
    fn check_in_bounds(&self, addr: usize, size: usize) -> Result<(), MemoryOutOfBounds> {
        if addr + size > self.size {
            std::eprintln!("OOB addr={addr}, size={size}");
            Err(MemoryOutOfBounds)
        } else {
            Ok(())
        }
    }

    pub fn store_u8(&self, addr: usize, i: u8) -> Result<(), MemoryOutOfBounds> {
        self.check_in_bounds(addr, 1)?;
        unsafe {
            self.ptr.offset(addr as isize).write(i);
        }
        Ok(())
    }

    pub fn store_u16(&self, addr: usize, i: u16) -> Result<(), MemoryOutOfBounds> {
        self.check_in_bounds(addr, 2)?;
        unsafe {
            self.ptr
                .offset(addr as isize)
                .cast::<u16>()
                .write_unaligned(i);
        }
        Ok(())
    }

    pub fn store_u32(&self, addr: usize, i: u32) -> Result<(), MemoryOutOfBounds> {
        self.check_in_bounds(addr, 4)?;
        unsafe {
            self.ptr
                .offset(addr as isize)
                .cast::<u32>()
                .write_unaligned(i);
        }
        Ok(())
    }

    pub fn store_u64(&self, addr: usize, i: u64) -> Result<(), MemoryOutOfBounds> {
        self.check_in_bounds(addr, 8)?;
        unsafe {
            self.ptr
                .offset(addr as isize)
                .cast::<u64>()
                .write_unaligned(i);
        }
        Ok(())
    }

    pub fn store(&self, addr: usize, data: &[u8]) -> Result<(), MemoryOutOfBounds> {
        self.check_in_bounds(addr, data.len())?;

        unsafe {
            data.as_ptr()
                .copy_to(self.ptr.offset(addr as isize), data.len());
        }
        Ok(())
    }

    pub fn load_u8(&self, addr: usize) -> Result<u8, MemoryOutOfBounds> {
        self.check_in_bounds(addr, 1)?;
        unsafe { Ok(self.ptr.offset(addr as isize).read()) }
    }

    pub fn load_u16(&self, addr: usize) -> Result<u16, MemoryOutOfBounds> {
        self.check_in_bounds(addr, 2)?;
        unsafe {
            Ok(self
                .ptr
                .offset(addr as isize)
                .cast::<u16>()
                .read_unaligned())
        }
    }

    pub fn load_u32(&self, addr: usize) -> Result<u32, MemoryOutOfBounds> {
        self.check_in_bounds(addr, 4)?;
        unsafe {
            Ok(self
                .ptr
                .offset(addr as isize)
                .cast::<u32>()
                .read_unaligned())
        }
    }

    pub fn load_u64(&self, addr: usize) -> Result<u64, MemoryOutOfBounds> {
        self.check_in_bounds(addr, 8)?;
        unsafe {
            Ok(self
                .ptr
                .offset(addr as isize)
                .cast::<u64>()
                .read_unaligned())
        }
    }

    pub fn size_pages(&self) -> u32 {
        (self.size / 65536) as u32
    }
}
