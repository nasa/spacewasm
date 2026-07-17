//! A C-constructible guest linear-memory allocator.
//!
//! The core interpreter takes an `Rc<dyn WasmMemoryAllocator>` when loading a
//! module. C cannot name that type, so [`SpacewasmAllocator`] wraps one behind
//! an opaque handle that C builds from three callbacks (alloc / realloc /
//! dealloc, plus an opaque `userdata`) and destroys when done. The handle is
//! passed to [`spacewasm_store_load_module`](crate::capi::spacewasm_store_load_module)
//! per module load.

use core::alloc::Layout;
use core::ffi::c_void;
use core::ptr::NonNull;

use spacewasm::{AllocError, Box, GlobalAllocator, Rc, WasmMemoryAllocator};

/// Allocate `size` bytes aligned to `align`. Return NULL on failure.
pub type spacewasm_alloc_fn_t =
    Option<unsafe extern "C" fn(userdata: *mut c_void, size: usize, align: usize) -> *mut u8>;

/// Resize the `old_size`-byte allocation at `ptr` (alignment `align`) to
/// `new_size` bytes, moving the contents if needed. Return NULL on failure.
pub type spacewasm_realloc_fn_t = Option<
    unsafe extern "C" fn(
        userdata: *mut c_void,
        ptr: *mut u8,
        old_size: usize,
        new_size: usize,
        align: usize,
    ) -> *mut u8,
>;

/// Free the `size`-byte allocation at `ptr` (alignment `align`).
pub type spacewasm_dealloc_fn_t =
    Option<unsafe extern "C" fn(userdata: *mut c_void, ptr: *mut u8, size: usize, align: usize)>;

/// The three C callbacks (unwrapped) plus their shared user data, adapting a C
/// allocator to [`WasmMemoryAllocator`]. The callbacks receive `(size, align)`
/// pairs rather than a `Layout`, since C has no equivalent type.
struct CAllocator {
    alloc: unsafe extern "C" fn(*mut c_void, usize, usize) -> *mut u8,
    realloc: unsafe extern "C" fn(*mut c_void, *mut u8, usize, usize, usize) -> *mut u8,
    dealloc: unsafe extern "C" fn(*mut c_void, *mut u8, usize, usize),
    userdata: *mut c_void,
}

impl WasmMemoryAllocator for CAllocator {
    fn allocate(&self, layout: Layout) -> Result<NonNull<u8>, AllocError> {
        // SAFETY: `alloc` is a valid C function pointer supplied at handle
        // creation; it is contracted to return either NULL or a pointer to
        // `layout.size()` bytes aligned to `layout.align()`.
        let ptr = unsafe { (self.alloc)(self.userdata, layout.size(), layout.align()) };
        NonNull::new(ptr).ok_or(AllocError::AllocationFailed)
    }

    fn reallocate(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        layout: Layout,
    ) -> Result<NonNull<u8>, AllocError> {
        // SAFETY: `realloc` is a valid C function pointer; `ptr`/`old_layout`
        // describe a live allocation from this allocator.
        let new = unsafe {
            (self.realloc)(
                self.userdata,
                ptr.as_ptr(),
                old_layout.size(),
                layout.size(),
                layout.align(),
            )
        };
        NonNull::new(new).ok_or(AllocError::AllocationFailed)
    }

    fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        // SAFETY: `dealloc` is a valid C function pointer; `ptr`/`layout`
        // describe a live allocation from this allocator.
        unsafe { (self.dealloc)(self.userdata, ptr.as_ptr(), layout.size(), layout.align()) };
    }
}

/// Opaque guest linear-memory allocator handle (`spacewasm_allocator_t`), owning
/// a reference-counted [`WasmMemoryAllocator`] built from C callbacks.
pub struct SpacewasmAllocator {
    inner: Rc<dyn WasmMemoryAllocator>,
}

/// Build an allocator handle from three C callbacks. Returns null if any
/// callback is null or the handle allocation fails.
pub fn allocator_new(
    alloc: spacewasm_alloc_fn_t,
    realloc: spacewasm_realloc_fn_t,
    dealloc: spacewasm_dealloc_fn_t,
    userdata: *mut c_void,
) -> *mut SpacewasmAllocator {
    let (Some(alloc), Some(realloc), Some(dealloc)) = (alloc, realloc, dealloc) else {
        return core::ptr::null_mut();
    };

    let c = CAllocator {
        alloc,
        realloc,
        dealloc,
        userdata,
    };

    match Rc::new(c) {
        Ok(rc) => {
            let inner = rc.into_wasm_memory_allocator();
            Box::new(SpacewasmAllocator { inner })
                .map(|b| Box::leak(b) as *mut SpacewasmAllocator)
                .unwrap_or(core::ptr::null_mut())
        }
        Err(_) => core::ptr::null_mut(),
    }
}

/// Clone the reference-counted allocator out of a handle, for handing to a
/// module load. Returns `None` on a null/invalid handle.
///
/// # Safety
/// `handle` must be null or a live pointer from [`allocator_new`].
pub unsafe fn allocator_clone_rc(
    handle: *const SpacewasmAllocator,
) -> Option<Rc<dyn WasmMemoryAllocator>> {
    let handle = unsafe { handle.as_ref() }?;
    Some(handle.inner.clone())
}

/// Destroy an allocator handle. No-op on null.
///
/// # Safety
/// `handle` must be a live pointer from [`allocator_new`], not already destroyed.
pub unsafe fn allocator_destroy(handle: *mut SpacewasmAllocator) {
    if handle.is_null() {
        return;
    }
    // Reclaim ownership and drop, releasing this handle's reference.
    let _ = unsafe { Box::from_raw(GlobalAllocator, handle) };
}
