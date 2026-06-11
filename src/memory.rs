use crate::{AllocError, MemType};
use core::alloc::Layout;
use core::ptr::NonNull;

/// An allocator for allocating WASM pages
pub trait WasmMemoryAllocator {
    /// Allocate a new memory region for linear memory
    fn allocate(&self, layout: Layout) -> Result<NonNull<u8>, AllocError>;

    /// Reallocate a memory region moving data if needed
    fn reallocate(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        layout: Layout,
    ) -> Result<NonNull<u8>, AllocError>;

    /// Deallocate memory that has been allocated
    fn deallocate(&self, ptr: NonNull<u8>, layout: Layout);
}

pub struct Memory {
    ptr: *mut u8,
    size: usize,
    limits: MemType,
    allocator: Option<&'static dyn WasmMemoryAllocator>,
}

// Please don't use this in flight...
impl Clone for Memory {
    fn clone(&self) -> Self {
        if let Some(allocator) = self.allocator {
            let ptr = allocator
                .allocate(Layout::from_size_align(self.size, 16).unwrap())
                .unwrap()
                .as_ptr();

            unsafe {
                self.ptr.copy_to(ptr, self.size);
            }

            Memory {
                ptr,
                size: self.size,
                limits: self.limits,
                allocator: Some(allocator),
            }
        } else {
            Memory::zero()
        }
    }
}

#[derive(Debug, Clone)]
pub enum MemoryError {
    OutOfBounds,
    OutOfMemory,
    AllocError(AllocError),
}

impl From<AllocError> for MemoryError {
    fn from(e: AllocError) -> MemoryError {
        MemoryError::AllocError(e)
    }
}

impl Default for Memory {
    fn default() -> Self {
        Memory::zero()
    }
}

impl Memory {
    // TODO(tumbar) Implement the custom page size proposal
    const PAGE_SIZE: usize = 65536;

    pub fn zero() -> Memory {
        Memory {
            ptr: core::ptr::null_mut(),
            size: 0,
            limits: MemType::zero(),
            allocator: None,
        }
    }

    pub fn new(
        ty: MemType,
        allocator: &'static dyn WasmMemoryAllocator,
    ) -> Result<Memory, AllocError> {
        let size = (ty.min() as usize) * Self::PAGE_SIZE;
        let ptr = allocator
            .allocate(Layout::from_size_align(size, 16).unwrap())?
            .as_ptr();

        // Clear the pages
        unsafe {
            ptr.write_bytes(0, size);
        }

        Ok(Memory {
            ptr,
            size,
            limits: ty,
            allocator: Some(allocator),
        })
    }

    #[inline]
    fn check_in_bounds(&self, addr: usize, size: usize) -> Result<(), MemoryError> {
        if addr + size > self.size {
            Err(MemoryError::OutOfBounds)
        } else {
            Ok(())
        }
    }

    pub fn store_u8(&mut self, addr: usize, i: u8) -> Result<(), MemoryError> {
        self.check_in_bounds(addr, 1)?;
        unsafe {
            self.ptr.offset(addr as isize).write(i);
        }
        Ok(())
    }

    pub fn store_u16(&mut self, addr: usize, i: u16) -> Result<(), MemoryError> {
        self.check_in_bounds(addr, 2)?;
        unsafe {
            self.ptr
                .offset(addr as isize)
                .cast::<u16>()
                .write_unaligned(i);
        }
        Ok(())
    }

    pub fn store_u32(&mut self, addr: usize, i: u32) -> Result<(), MemoryError> {
        self.check_in_bounds(addr, 4)?;
        unsafe {
            self.ptr
                .offset(addr as isize)
                .cast::<u32>()
                .write_unaligned(i);
        }
        Ok(())
    }

    pub fn store_u64(&mut self, addr: usize, i: u64) -> Result<(), MemoryError> {
        self.check_in_bounds(addr, 8)?;
        unsafe {
            self.ptr
                .offset(addr as isize)
                .cast::<u64>()
                .write_unaligned(i);
        }
        Ok(())
    }

    pub fn store(&mut self, addr: usize, data: &[u8]) -> Result<(), MemoryError> {
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
    pub fn grow(&mut self, n: u32) -> Result<u32, MemoryError> {
        if !self.limits.can_hold(self.size() + n) {
            return Err(MemoryError::OutOfMemory);
        }

        let old_size = self.size;
        let new_size = (Self::PAGE_SIZE * n as usize) + self.size;
        if let Some(allocator) = &self.allocator
            && let Some(ptr) = NonNull::new(self.ptr)
        {
            self.ptr = allocator
                .reallocate(
                    ptr,
                    Layout::from_size_align(old_size, 16).unwrap(),
                    Layout::from_size_align(new_size, 16).unwrap(),
                )?
                .as_ptr();

            // Clear the new memory
            let new_ptr = unsafe { self.ptr.offset(old_size as isize) };
            unsafe {
                new_ptr.write_bytes(0, Self::PAGE_SIZE * n as usize);
            }

            self.size = new_size;

            Ok((old_size / Self::PAGE_SIZE) as u32)
        } else {
            Err(MemoryError::OutOfMemory)
        }
    }

    pub fn size(&self) -> u32 {
        (self.size / Self::PAGE_SIZE) as u32
    }
}

impl Drop for Memory {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            let allocator = self.allocator.take().unwrap();
            allocator.deallocate(
                NonNull::new(self.ptr).unwrap(),
                Layout::from_size_align(self.size, 16).unwrap(),
            )
        }
    }
}
