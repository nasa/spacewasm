use crate::Rc;
use crate::{AllocError, MemType};
use core::alloc::Layout;
use core::fmt::{Debug, Formatter};
use core::ptr::NonNull;

/// An allocator for allocating Wasm pages
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

impl<T: WasmMemoryAllocator> Rc<T> {
    pub fn into_wasm_memory_allocator(self) -> Rc<dyn WasmMemoryAllocator>
    where
        T: WasmMemoryAllocator + 'static,
    {
        unsafe { self.into_dyn(|x| x as &dyn WasmMemoryAllocator) }
    }
}

pub struct Memory {
    ptr: *mut u8,
    size: usize,
    limits: MemType,
    allocator: Option<Rc<dyn WasmMemoryAllocator>>,
}

impl Debug for Memory {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Memory").finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryError {
    OutOfBounds,
    OutOfMemory,
    AllocationFailed,
    PageTooSmall,
}

impl From<AllocError> for MemoryError {
    fn from(e: AllocError) -> MemoryError {
        match e {
            AllocError::AllocationFailed => MemoryError::AllocationFailed,
            AllocError::OutOfMemory => MemoryError::OutOfMemory,
            AllocError::PageTooSmall => MemoryError::PageTooSmall,
        }
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

    pub fn new(ty: MemType, allocator: Rc<dyn WasmMemoryAllocator>) -> Result<Memory, AllocError> {
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
}

impl Memory {
    #[inline]
    fn check_in_bounds(&self, addr: usize, size: usize) -> Result<(), MemoryError> {
        if size > self.size || addr > self.size - size {
            Err(MemoryError::OutOfBounds)
        } else {
            Ok(())
        }
    }

    /// Resolve a guest memory access `base + offset` to a byte address.
    ///
    /// The sum is evaluated in `u64` and narrowed back to `usize`, so it can
    /// never wrap on 32-bit targets, where `usize` is 32-bit and `base + offset`
    /// may exceed `u32::MAX`. An address that does not fit `usize` is rejected as
    /// out of bounds rather than aliasing a valid cell. The resulting address is
    /// still bounds-checked against the memory size by the individual load/store
    /// operations.
    #[inline]
    pub fn effective_address(base: u32, offset: u32) -> Result<usize, MemoryError> {
        usize::try_from(base as u64 + offset as u64).map_err(|_| MemoryError::OutOfBounds)
    }

    pub fn store_u8(&self, addr: usize, i: u8) -> Result<(), MemoryError> {
        self.check_in_bounds(addr, 1)?;
        unsafe {
            self.ptr.add(addr).write(i);
        }
        Ok(())
    }

    pub fn store_u16(&self, addr: usize, i: u16) -> Result<(), MemoryError> {
        self.check_in_bounds(addr, 2)?;
        unsafe {
            self.ptr.add(addr).cast::<u16>().write_unaligned(i);
        }
        Ok(())
    }

    pub fn store_u32(&self, addr: usize, i: u32) -> Result<(), MemoryError> {
        self.check_in_bounds(addr, 4)?;
        unsafe {
            self.ptr.add(addr).cast::<u32>().write_unaligned(i);
        }
        Ok(())
    }

    pub fn store_u64(&self, addr: usize, i: u64) -> Result<(), MemoryError> {
        self.check_in_bounds(addr, 8)?;
        unsafe {
            self.ptr.add(addr).cast::<u64>().write_unaligned(i);
        }
        Ok(())
    }

    pub fn store(&self, addr: usize, data: &[u8]) -> Result<(), MemoryError> {
        self.check_in_bounds(addr, data.len())?;

        unsafe {
            data.as_ptr().copy_to(self.ptr.add(addr), data.len());
        }
        Ok(())
    }

    pub fn load_u8(&self, addr: usize) -> Result<u8, MemoryError> {
        self.check_in_bounds(addr, 1)?;
        unsafe { Ok(self.ptr.add(addr).read()) }
    }

    pub fn load_u16(&self, addr: usize) -> Result<u16, MemoryError> {
        self.check_in_bounds(addr, 2)?;
        unsafe { Ok(self.ptr.add(addr).cast::<u16>().read_unaligned()) }
    }

    pub fn load_u32(&self, addr: usize) -> Result<u32, MemoryError> {
        self.check_in_bounds(addr, 4)?;
        unsafe { Ok(self.ptr.add(addr).cast::<u32>().read_unaligned()) }
    }

    pub fn load_u64(&self, addr: usize) -> Result<u64, MemoryError> {
        self.check_in_bounds(addr, 8)?;
        unsafe { Ok(self.ptr.add(addr).cast::<u64>().read_unaligned()) }
    }

    pub fn load(&self, addr: usize, len: usize) -> Result<&[u8], MemoryError> {
        self.check_in_bounds(addr, len)?;
        Ok(unsafe { core::slice::from_raw_parts(self.ptr.add(addr), len) })
    }

    /// Grow the memory by n pages
    /// If the memory growth succeeds, return the old number of pages
    pub fn grow(&mut self, n: u32) -> Result<u32, MemoryError> {
        let Some(total_pages) = self.size().checked_add(n) else {
            return Err(MemoryError::OutOfMemory);
        };

        if !self.limits.can_hold(total_pages) {
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
            let new_ptr = unsafe { self.ptr.add(old_size) };
            unsafe {
                new_ptr.write_bytes(0, Self::PAGE_SIZE * n as usize);
            }

            self.size = new_size;

            Ok((old_size / Self::PAGE_SIZE) as u32)
        } else {
            Err(MemoryError::OutOfMemory)
        }
    }

    pub fn mem_type(&self) -> MemType {
        self.limits
    }

    pub fn size(&self) -> u32 {
        (self.size / Self::PAGE_SIZE) as u32
    }

    pub fn is_zero(&self) -> bool {
        self.ptr.is_null()
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

#[cfg(kani)]
mod kani_proofs {
    use super::*;
    use crate::Allocator;
    extern crate std;

    use crate::test_support::RustSystemAllocator;

    #[kani::proof]
    fn proof_store_load_correctness() {
        let alloc = RustSystemAllocator;
        let size = 64;

        let ptr = unsafe {
            alloc
                .alloc(Layout::from_size_align(size, 16).unwrap())
                .unwrap()
        };
        let mem = Memory {
            ptr,
            size,
            limits: MemType::zero(),
            allocator: None,
        };

        // Test all integer sizes with symbolic values and addresses
        let addr: usize = kani::any();
        kani::assume(addr <= size - 8); // Reserve space for largest type (u64)

        // Test u8 store/load
        let val_u8: u8 = kani::any();
        mem.store_u8(addr, val_u8).unwrap();
        assert_eq!(mem.load_u8(addr).unwrap(), val_u8, "u8 round-trip failed");

        // Test u16 store/load
        let val_u16: u16 = kani::any();
        mem.store_u16(addr, val_u16).unwrap();
        assert_eq!(
            mem.load_u16(addr).unwrap(),
            val_u16,
            "u16 round-trip failed"
        );

        // Test u32 store/load
        let val_u32: u32 = kani::any();
        mem.store_u32(addr, val_u32).unwrap();
        assert_eq!(
            mem.load_u32(addr).unwrap(),
            val_u32,
            "u32 round-trip failed"
        );

        // Test u64 store/load
        let val_u64: u64 = kani::any();
        mem.store_u64(addr, val_u64).unwrap();
        assert_eq!(
            mem.load_u64(addr).unwrap(),
            val_u64,
            "u64 round-trip failed"
        );

        // Test out-of-bounds detection
        assert!(
            mem.store_u32(size - 2, 0).is_err(),
            "Out-of-bounds store must fail"
        );
        assert!(
            mem.store_u32(size, 0).is_err(),
            "Store at boundary must fail"
        );
        assert!(
            mem.load_u32(size - 2).is_err(),
            "Out-of-bounds load must fail"
        );
        assert!(mem.load_u32(size).is_err(), "Load at boundary must fail");

        // Test addresses that cause overflow in bounds check
        let overflow_addr = usize::MAX - 1;
        assert!(
            mem.store_u32(overflow_addr, 0).is_err(),
            "Overflow address must be rejected"
        );
        assert!(
            mem.load_u32(overflow_addr).is_err(),
            "Overflow address must be rejected"
        );

        // Verify safe addresses have lossless isize conversion
        if addr <= isize::MAX as usize {
            let offset = addr as isize;
            assert!(offset >= 0, "Valid addr converts to non-negative offset");
            assert!(offset as usize == addr, "isize conversion must be lossless");
        }

        // Prevent Drop from calling GlobalAllocator FFI
        core::mem::forget(mem);
        unsafe { alloc.dealloc(ptr, Layout::from_size_align(size, 16).unwrap()) };
    }

    #[kani::proof]
    fn proof_byte_slice_operations() {
        let alloc = RustSystemAllocator;
        let size = 16;

        let ptr = unsafe {
            alloc
                .alloc(Layout::from_size_align(size, 16).unwrap())
                .unwrap()
        };
        let mem = Memory {
            ptr,
            size,
            limits: MemType::zero(),
            allocator: None,
        };

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
        assert!(
            mem.store(size - 2, &[1, 2, 3, 4]).is_err(),
            "Out-of-bounds store must fail"
        );
        assert!(
            mem.load(size - 2, 4).is_err(),
            "Out-of-bounds load must fail"
        );

        // Prevent Drop from calling GlobalAllocator FFI
        core::mem::forget(mem);
        unsafe { alloc.dealloc(ptr, Layout::from_size_align(size, 16).unwrap()) };
    }

    /// The effective address is the exact sum of `base` and `offset`; it is
    /// never silently wrapped modulo the pointer width.
    #[kani::proof]
    fn proof_effective_address_no_wrap() {
        let base: u32 = kani::any();
        let offset: u32 = kani::any();
        let ea = Memory::effective_address(base, offset).unwrap();
        assert_eq!(ea as u64, base as u64 + offset as u64);
    }
}
