use core::alloc::{Layout, LayoutError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AllocError {
    /// Zero sized allocations are undefined and disallowed
    IllegalZeroSize,

    /// Page was too small to fit this allocation
    PageTooSmall(usize),

    /// Not enough pages could be allocated to accommodate this allocation
    OutOfMemory,

    /// A LayoutError occured
    InvalidLayout,
}

impl From<LayoutError> for AllocError {
    fn from(_value: LayoutError) -> Self {
        AllocError::InvalidLayout
    }
}

/// Our allocator trait. This is very similar to [core::alloc::GlobalAlloc].
/// We are not using that trait since it doesn't return Result<...> it just panics
/// if an allocation fails. An adaptor is automatically implemented
pub unsafe trait Allocator {
    unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError>;
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout);
}

unsafe impl<T: core::alloc::GlobalAlloc> Allocator for T {
    unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
        unsafe { Ok(core::alloc::GlobalAlloc::alloc(self, layout)) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { core::alloc::GlobalAlloc::dealloc(self, ptr, layout) }
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

/// Alloc some memory from the heap
pub unsafe fn alloc(layout: Layout) -> Result<*mut u8, AllocError> {
    unsafe { (*ALLOCATOR).alloc(layout) }
}

/// Free some memory from the heap
pub unsafe fn dealloc(ptr: *mut u8, layout: Layout) {
    unsafe { (*ALLOCATOR).dealloc(ptr, layout) }
}
