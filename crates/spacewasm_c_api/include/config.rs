/// Maximum control frame depth per function.
pub const MAX_CONTROL_FRAMES: usize = 64;

/// Maximum stack depth per function.
pub const MAX_STACK_DEPTH: usize = 256;

/// Number of pages the global allocator can allocate.
pub const GLOBAL_ALLOCATOR_MAX_PAGES: usize = 16;

/// Size of each global allocator page.
///
/// Smaller page sizes will restrict single large allocations to that size.
/// Large allocations can come from modules with large amounts of functions to represent Vec<Func>
pub const GLOBAL_ALLOCATOR_PAGE_SIZE: usize = 8192;
