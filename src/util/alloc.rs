use core::alloc::{Layout, LayoutError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AllocError {
    /// Zero sized allocations are undefined and disallowed
    IllegalZeroSize,

    /// Page was too small to fit this allocation
    PageTooSmall,

    /// Not enough pages could be allocated to accommodate this allocation
    OutOfMemory,

    /// A LayoutError occurred
    InvalidLayout,

    /// A generic allocation failure
    AllocationFailed,

    /// Stack-based heap allocations only support up 128-bit alignment
    InvalidAlignment,

    /// Stack-based heap allocation surpassed the supported nested allocation count
    StackAllocationTooDeep,

    /// Stack-based heap requires allocation and deallocation to occur in reverse order.
    /// This rule is checked during deallocation. If it is not held, this error will be thrown.
    /// This error is also raised when attempting to free a stack address when there no more
    /// allocations held by the StackAllocator
    StackDeallocationInvariantViolation,
}

impl From<LayoutError> for AllocError {
    fn from(_value: LayoutError) -> Self {
        AllocError::InvalidLayout
    }
}

#[derive(Debug, Default, Clone)]
pub struct MemoryStatistics {
    pub total_bytes: i32,
    pub pad_bytes: i32,
}

/// Computes the delta between two different statistic samples
impl core::ops::Sub for MemoryStatistics {
    type Output = MemoryStatistics;

    fn sub(self, rhs: Self) -> Self::Output {
        MemoryStatistics {
            total_bytes: self.total_bytes - rhs.total_bytes,
            pad_bytes: self.pad_bytes - rhs.pad_bytes,
        }
    }
}

impl core::ops::AddAssign for MemoryStatistics {
    fn add_assign(&mut self, rhs: Self) {
        self.total_bytes += rhs.total_bytes;
        self.pad_bytes += rhs.pad_bytes;
    }
}

/// Our allocator trait. This is very similar to [core::alloc::GlobalAlloc].
/// We are not using that trait since it doesn't return Result<...> it just panics
/// if an allocation fails. An adaptor is automatically implemented
pub unsafe trait Allocator {
    unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError>;
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout);
    fn memory_statistics(&self) -> MemoryStatistics;
}

unsafe impl<T: Allocator> Allocator for &T {
    unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
        unsafe { (**self).alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { (**self).dealloc(ptr, layout) }
    }

    fn memory_statistics(&self) -> MemoryStatistics {
        (**self).memory_statistics()
    }
}

pub struct GlobalAllocator;
unsafe impl Allocator for GlobalAllocator {
    unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
        unsafe { (*ALLOCATOR).alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { (*ALLOCATOR).dealloc(ptr, layout) }
    }

    fn memory_statistics(&self) -> MemoryStatistics {
        unsafe { (*ALLOCATOR).memory_statistics() }
    }
}

struct UnimplementedAllocator;
unsafe impl Allocator for UnimplementedAllocator {
    unsafe fn alloc(&self, _layout: Layout) -> Result<*mut u8, AllocError> {
        unimplemented!()
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        unimplemented!()
    }

    fn memory_statistics(&self) -> MemoryStatistics {
        unimplemented!()
    }
}

static UNIMPLEMENTED: UnimplementedAllocator = UnimplementedAllocator;
static mut ALLOCATOR: *const dyn Allocator = &raw const UNIMPLEMENTED;

struct AllocatorSetter<'a> {
    _marker: &'a dyn Allocator,
}

impl<'a> AllocatorSetter<'a> {
    fn new(allocator: &'a dyn Allocator) -> Self {
        unsafe {
            ALLOCATOR = core::mem::transmute(allocator);
        }
        AllocatorSetter { _marker: allocator }
    }
}

impl<'a> Drop for AllocatorSetter<'a> {
    fn drop(&mut self) {
        unsafe {
            ALLOCATOR = &raw const UNIMPLEMENTED;
        }
    }
}

pub fn run<A, F, T>(allocator: &A, f: F) -> T
where
    A: Allocator,
    F: FnOnce() -> T,
{
    let _guard = AllocatorSetter::new(allocator);
    f()
}
