//! Runtime-registered global heap allocator, backed by C callbacks.

use core::alloc::Layout;
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicPtr, Ordering};

use spacewasm::{AllocError, Allocator, MemoryStatistics, PageAllocator};

// TODO(tumbar) Make this configurable.
const MAX_PAGES: usize = 16;
const PAGE_SIZE: usize = 8192;

/// Allocate `size` bytes aligned to `align`. Return NULL on failure. Per page allocation.
pub type spacewasm_global_alloc_fn_t =
    Option<unsafe extern "C" fn(userdata: *mut c_void, size: usize, align: usize) -> *mut u8>;

/// Free the `size`-byte allocation at `ptr` (alignment `align`). Per page deallocation.
pub type spacewasm_global_dealloc_fn_t =
    Option<unsafe extern "C" fn(userdata: *mut c_void, ptr: *mut u8, size: usize, align: usize)>;

struct CPageBackend {
    /// `spacewasm_global_alloc_fn_t`, type-erased to a data pointer.
    alloc: AtomicPtr<()>,
    /// `spacewasm_global_dealloc_fn_t`, type-erased to a data pointer.
    dealloc: AtomicPtr<()>,
    /// Opaque user data passed to both callbacks.
    userdata: AtomicPtr<c_void>,
}

impl CPageBackend {
    const fn new() -> Self {
        CPageBackend {
            alloc: AtomicPtr::new(ptr::null_mut()),
            dealloc: AtomicPtr::new(ptr::null_mut()),
            userdata: AtomicPtr::new(ptr::null_mut()),
        }
    }
}

unsafe impl Allocator for CPageBackend {
    unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
        let f = self.alloc.load(Ordering::Acquire);
        if f.is_null() {
            // No allocator registered yet.
            return Err(AllocError::AllocationFailed);
        }
        // SAFETY: `f` was stored from a `spacewasm_global_alloc_fn_t` fn pointer
        // by `spacewasm_set_global_allocator`; the transmute recovers that exact
        // type. Function and data pointers share a width on supported targets.
        let alloc: unsafe extern "C" fn(*mut c_void, usize, usize) -> *mut u8 =
            unsafe { core::mem::transmute(f) };
        let ptr = unsafe {
            alloc(
                self.userdata.load(Ordering::Acquire),
                layout.size(),
                layout.align(),
            )
        };
        // A NULL page allocation means the C backend is out of memory.
        if ptr.is_null() {
            Err(AllocError::OutOfMemory)
        } else {
            Ok(ptr)
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let f = self.dealloc.load(Ordering::Acquire);
        if f.is_null() {
            // No allocator registered: nothing to free against.
            return;
        }
        // SAFETY: as in `alloc`, `f` recovers the registered
        // `spacewasm_global_dealloc_fn_t`.
        let dealloc: unsafe extern "C" fn(*mut c_void, *mut u8, usize, usize) =
            unsafe { core::mem::transmute(f) };
        unsafe {
            dealloc(
                self.userdata.load(Ordering::Acquire),
                ptr,
                layout.size(),
                layout.align(),
            )
        };
    }

    fn memory_statistics(&self) -> MemoryStatistics {
        MemoryStatistics {
            total_bytes: 0,
            pad_bytes: 0,
        }
    }
}

static BACKEND: CPageBackend = CPageBackend::new();

spacewasm::global_allocator!(
    PageAllocator<'static, MAX_PAGES>,
    PageAllocator::new(&BACKEND, PAGE_SIZE)
);

/// Install the process-wide heap allocator backing the interpreter.
/// `alloc`/`dealloc` are called at *page* granularity.
///
/// # Safety
/// `alloc`/`dealloc` must remain valid for the lifetime of the process and
/// honor the requested size/alignment. `userdata` must outlive all allocations.
#[unsafe(no_mangle)]
pub extern "C" fn spacewasm_set_global_allocator(
    alloc: spacewasm_global_alloc_fn_t,
    dealloc: spacewasm_global_dealloc_fn_t,
    userdata: *mut c_void,
) -> i32 {
    let (Some(alloc), Some(dealloc)) = (alloc, dealloc) else {
        return 1; // a null callback
    };

    // Publish userdata before the callbacks so a reader that observes a
    // non-null `alloc` also sees the matching userdata.
    BACKEND.userdata.store(userdata, Ordering::Release);
    BACKEND.dealloc.store(dealloc as *mut (), Ordering::Release);
    BACKEND.alloc.store(alloc as *mut (), Ordering::Release);
    0
}
