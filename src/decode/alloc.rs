use core::alloc::{Layout, LayoutError};

// TODO(tumbar) Do we want a more interesting alloc error?
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllocError;

impl From<LayoutError> for AllocError {
    fn from(_value: LayoutError) -> Self {
        AllocError
    }
}

pub unsafe trait Allocator {
    unsafe fn allocate(&self, layout: Layout) -> Result<*mut u8, AllocError>;
    unsafe fn deallocate(&self, ptr: *mut u8, layout: Layout);
}

struct UnimplementedAllocator;
unsafe impl Allocator for UnimplementedAllocator {
    unsafe fn allocate(&self, _layout: Layout) -> Result<*mut u8, AllocError> {
        unimplemented!()
    }

    unsafe fn deallocate(&self, _ptr: *mut u8, _layout: Layout) {
        unimplemented!()
    }
}

static DEFAULT_ALLOCATOR: UnimplementedAllocator = UnimplementedAllocator;
static mut ALLOCATOR: *const dyn Allocator = &raw const DEFAULT_ALLOCATOR;

pub unsafe fn init(allocator: *const dyn Allocator) {
    unsafe {
        ALLOCATOR = allocator;
    }
}

/// Alloc some memory from the heap
pub unsafe fn alloc(layout: Layout) -> Result<*mut u8, AllocError> {
    unsafe { (*ALLOCATOR).allocate(layout) }
}

/// Free some memory from the heap
pub unsafe fn dealloc(ptr: *mut u8, layout: Layout) {
    unsafe { (*ALLOCATOR).deallocate(ptr, layout) }
}
