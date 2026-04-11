use core::alloc::{Layout, LayoutError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllocError;

impl From<LayoutError> for AllocError {
    fn from(_value: LayoutError) -> Self {
        // TODO(tumbar) Do we want a more interesting alloc error
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

pub unsafe fn alloc(layout: Layout) -> Result<*mut u8, AllocError> {
    unsafe { (*ALLOCATOR).allocate(layout) }
}

pub unsafe fn dealloc(ptr: *mut u8, layout: Layout) {
    unsafe { (*ALLOCATOR).deallocate(ptr, layout) }
}
