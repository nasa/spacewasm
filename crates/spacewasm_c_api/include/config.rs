// Compile-time limits for the C API build (the default configuration).
// To override this file set `SPACEWASM_CONFIG` in your `build.rs`.

/// Number of pages the global allocator can allocate.
pub const GLOBAL_ALLOCATOR_MAX_PAGES: usize = 16;

/// Size of each global allocator page, which bounds the largest allocation.
pub const GLOBAL_ALLOCATOR_PAGE_SIZE: usize = 8192;
