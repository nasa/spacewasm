//! The concrete `extern "C"` entry points (`spacewasm_*`) that make up the C
//! ABI.

use core::ffi::c_char;
use core::ffi::c_void;

use crate::abi;
use crate::alloc::{
    self, SpacewasmAllocator, spacewasm_alloc_fn_t, spacewasm_dealloc_fn_t, spacewasm_realloc_fn_t,
};
use crate::engine::{Builder, SpacewasmCaller, SpacewasmStore, spacewasm_host_fn_t};
use crate::host;
use crate::status::{self, spacewasm_run_status_t, spacewasm_status_t, spacewasm_trap_t};
use crate::stream::spacewasm_read_fn_t;
use crate::value::{spacewasm_valtype_t, spacewasm_value_t};

/// Create a guest linear-memory allocator from three C callbacks, returning an
/// opaque handle (or null if any callback is null or allocation fails). The
/// handle is passed to [`spacewasm_store_load_module`] and must be released with
/// [`spacewasm_allocator_destroy`]. `userdata` is passed to every callback.
#[unsafe(no_mangle)]
pub extern "C" fn spacewasm_allocator_new(
    alloc: spacewasm_alloc_fn_t,
    realloc: spacewasm_realloc_fn_t,
    dealloc: spacewasm_dealloc_fn_t,
    userdata: *mut c_void,
) -> *mut SpacewasmAllocator {
    alloc::allocator_new(alloc, realloc, dealloc, userdata)
}

/// Destroy an allocator handle. No-op on null. Any loaded module keeps its own
/// reference to the underlying allocator, so destroying the handle after loading
/// is safe.
///
/// # Safety
/// `allocator` must be a live handle from [`spacewasm_allocator_new`], not
/// already destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_allocator_destroy(allocator: *mut SpacewasmAllocator) {
    unsafe { alloc::allocator_destroy(allocator) }
}

/// Create a new builder sized for at most `max_modules` guest modules and
/// `max_host_modules` host modules. Returns null on allocation failure.
#[unsafe(no_mangle)]
pub extern "C" fn spacewasm_builder_new(max_modules: usize, max_host_modules: u32) -> *mut Builder {
    abi::builder_new(max_modules, max_host_modules)
}

/// Register a host module named `name`, sized for `max_functions` functions,
/// writing its index to `out_idx` (if non-null).
///
/// # Safety
/// See the generated header. `builder` must be a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_builder_add_host_module(
    builder: *mut Builder,
    name: *const c_char,
    max_functions: u32,
    out_idx: *mut u32,
) -> spacewasm_status_t {
    unsafe { abi::builder_add_host_module(builder, name, max_functions, out_idx) }
}

/// Register a host function `name` in host module `module_idx`, with parameter
/// and return signatures given by `params_sig`/`returns_sig` and implemented by
/// callback `f` (passed `userdata` on each call).
///
/// # Safety
/// `builder` must be live; all C strings valid and NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_builder_add_host_function(
    builder: *mut Builder,
    module_idx: u32,
    name: *const c_char,
    params_sig: *const c_char,
    returns_sig: *const c_char,
    f: spacewasm_host_fn_t,
    userdata: *mut c_void,
) -> spacewasm_status_t {
    unsafe {
        abi::builder_add_host_function(
            builder,
            module_idx,
            name,
            params_sig,
            returns_sig,
            f,
            userdata,
        )
    }
}

/// Consume the builder and finish it into a store handle sized with a
/// `stack_size`-byte guest stack and room for `max_code_pages` compiled code
/// pages, writing it to `out_store`. No guest module is loaded yet; use
/// [`spacewasm_store_load_module`] to load one or more.
///
/// # Safety
/// `builder` (consumed), `out_store` valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_builder_finish(
    builder: *mut Builder,
    stack_size: usize,
    max_code_pages: u32,
    out_store: *mut *mut SpacewasmStore,
) -> spacewasm_status_t {
    unsafe { abi::builder_finish(builder, stack_size, max_code_pages, out_store) }
}

/// Load a guest module named `name` onto an existing store by streaming its
/// bytes through the `read` callback (`chunk_size` sizes the scratch buffer, 0
/// for default). This does not run the module's start function; use
/// [`spacewasm_store_module_needs_start`] and [`spacewasm_store_run_start`] for
/// that. `allocator` supplies the guest linear memory (see
/// [`spacewasm_allocator_new`]). Writes the new module's index to
/// `out_module_idx` (if non-null). May be called repeatedly to load several
/// modules onto the same store.
///
/// # Safety
/// `store` and `allocator` must be live handles; `read` a valid callback;
/// `out_module_idx` null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_store_load_module(
    store: *mut SpacewasmStore,
    name: *const c_char,
    read: spacewasm_read_fn_t,
    read_userdata: *mut c_void,
    chunk_size: usize,
    allocator: *mut SpacewasmAllocator,
    out_module_idx: *mut u32,
) -> spacewasm_status_t {
    // SAFETY: `allocator` is null or a live handle per the contract.
    let Some(alloc) = (unsafe { alloc::allocator_clone_rc(allocator) }) else {
        return status::SPACEWASM_ERR_NULL_ARG;
    };
    unsafe {
        abi::store_load_module(
            store,
            name,
            read,
            read_userdata,
            chunk_size,
            alloc,
            out_module_idx,
        )
    }
}

/// Destroy a builder that was never consumed by a load. No-op on null.
///
/// # Safety
/// `builder` must be a live handle, not already consumed/destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_builder_destroy(builder: *mut Builder) {
    unsafe { abi::builder_destroy(builder) }
}

/// Look up the exported function named `name` in module `module_idx` and write
/// its index to `out_index`.
///
/// # Safety
/// `store` must be live; `name` valid; `out_index` valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_store_find_export_func(
    store: *mut SpacewasmStore,
    module_idx: u32,
    name: *const c_char,
    out_index: *mut u32,
) -> spacewasm_status_t {
    unsafe { abi::store_find_export_func(store, module_idx, name, out_index) }
}

/// Report whether module `module_idx` declares a start function that should be
/// run (via [`spacewasm_store_run_start`]) before the module is used, writing
/// the answer to `out_needs_start`.
///
/// # Safety
/// `store` must be live; `out_needs_start` valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_store_module_needs_start(
    store: *mut SpacewasmStore,
    module_idx: u32,
    out_needs_start: *mut bool,
) -> spacewasm_status_t {
    unsafe { abi::store_module_needs_start(store, module_idx, out_needs_start) }
}

/// Run the start function of module `module_idx` (if any) for up to `fuel`
/// instructions, writing any trap to `out_trap`. Returns whether the start
/// function finished, trapped, paused, or ran out of fuel. A module with no
/// start function returns [`spacewasm_run_status_t::SPACEWASM_RUN_FINISHED`]
/// immediately. If it runs out of fuel, call again to resume.
///
/// # Safety
/// `store` must be live; `out_trap` null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_store_run_start(
    store: *mut SpacewasmStore,
    module_idx: u32,
    fuel: usize,
    out_trap: *mut spacewasm_trap_t,
) -> spacewasm_run_status_t {
    unsafe { abi::store_run_start(store, module_idx, fuel, out_trap) }
}

/// Set up a call to exported function `func_index` of module `module_idx` with
/// the `n` arguments in `params`. Does not run the function; drive execution
/// with [`spacewasm_store_run`].
///
/// # Safety
/// `store` must be live; `params` valid for `n` entries.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_store_invoke(
    store: *mut SpacewasmStore,
    module_idx: u32,
    func_index: u32,
    params: *const spacewasm_value_t,
    n: usize,
) -> spacewasm_status_t {
    unsafe { abi::store_invoke(store, module_idx, func_index, params, n) }
}

/// Run the pending invocation for up to `fuel` units of work, writing any trap
/// to `out_trap`. Returns whether the call finished, trapped, or ran out of fuel.
///
/// # Safety
/// `store` must be live; `out_trap` null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_store_run(
    store: *mut SpacewasmStore,
    fuel: usize,
    out_trap: *mut spacewasm_trap_t,
) -> spacewasm_run_status_t {
    unsafe { abi::store_run(store, fuel, out_trap) }
}

/// Run the pending invocation to completion, slicing execution into
/// `fuel_per_slice` chunks (0 for unbounded), writing any trap to `out_trap`.
///
/// # Safety
/// `store` must be live; `out_trap` null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_store_run_to_completion(
    store: *mut SpacewasmStore,
    fuel_per_slice: usize,
    out_trap: *mut spacewasm_trap_t,
) -> spacewasm_run_status_t {
    unsafe { abi::store_run_to_completion(store, fuel_per_slice, out_trap) }
}

/// Fetch the result of the last completed call, coerced to `expected`, into
/// `out`.
///
/// # Safety
/// `store` must be live; `out` valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_store_get_result(
    store: *mut SpacewasmStore,
    expected: spacewasm_valtype_t,
    out: *mut spacewasm_value_t,
) -> spacewasm_status_t {
    unsafe { abi::store_get_result(store, expected, out) }
}

/// Destroy a store and free its resources. No-op on null.
///
/// # Safety
/// `store` must be a live handle, not already destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_store_destroy(store: *mut SpacewasmStore) {
    unsafe { abi::store_destroy(store) }
}

/// Read `len` bytes of guest linear memory starting at `addr` into `dst`.
/// Intended for use from within a host function.
///
/// # Safety
/// `caller` must be a live caller handle; `dst` valid for `len`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_mem_read(
    caller: *mut SpacewasmCaller,
    addr: u32,
    dst: *mut u8,
    len: usize,
) -> spacewasm_status_t {
    unsafe { host::mem_read(caller, addr, dst, len) }
}

/// Write `len` bytes from `src` into guest linear memory starting at `addr`.
/// Intended for use from within a host function.
///
/// # Safety
/// `caller` must be a live caller handle; `src` valid for `len`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_mem_write(
    caller: *mut SpacewasmCaller,
    addr: u32,
    src: *const u8,
    len: usize,
) -> spacewasm_status_t {
    unsafe { host::mem_write(caller, addr, src, len) }
}

/// Write the current size of guest linear memory, in pages, to `out_pages`.
///
/// # Safety
/// `caller` must be a live caller handle; `out_pages` valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_mem_size(
    caller: *mut SpacewasmCaller,
    out_pages: *mut u32,
) -> spacewasm_status_t {
    unsafe { host::mem_size(caller, out_pages) }
}
