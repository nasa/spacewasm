//! The concrete `extern "C"` entry points (`spacewasm_*`) that make up the C
//! ABI.

use core::ffi::c_char;
use core::ffi::c_void;

use spacewasm::HostFunction;

use crate::abi;
use crate::alloc::{
    self, SpacewasmAllocator, spacewasm_alloc_fn_t, spacewasm_dealloc_fn_t, spacewasm_realloc_fn_t,
};
use crate::engine::{SpacewasmCaller, SpacewasmStore, spacewasm_host_fn_t};
use crate::host;
use crate::host::CHostFunction;
use crate::status::{self, spacewasm_run_status_t, spacewasm_status_t, spacewasm_trap_t};
use crate::stream::spacewasm_read_fn_t;
use crate::value::{spacewasm_valtype_t, spacewasm_value_t};

macro_rules! check {
    ($val:expr) => {
        match $val {
            Ok(v) => v,
            Err(e) => return e,
        }
    };
}

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

#[repr(C)]
pub struct spacewasm_host_t {
    ptr: *mut spacewasm::HostModule,
    capacity: u32,
    len: u32,
}

impl From<spacewasm::Vec<spacewasm::HostModule>> for spacewasm_host_t {
    fn from(value: spacewasm::Vec<spacewasm::HostModule>) -> Self {
        unsafe { core::mem::transmute(value) }
    }
}

impl From<spacewasm_host_t> for spacewasm::Vec<spacewasm::HostModule> {
    fn from(value: spacewasm_host_t) -> Self {
        unsafe { core::mem::transmute(value) }
    }
}

impl From<&mut spacewasm_host_t> for &mut spacewasm::Vec<spacewasm::HostModule> {
    fn from(value: &mut spacewasm_host_t) -> Self {
        unsafe { core::mem::transmute(value) }
    }
}

/// Create a new host module vector of max_host_module size
///
/// # Safety
/// `host` must be live
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_host_new(
    len: u32,
    dest: *mut spacewasm_host_t,
) -> spacewasm_status_t {
    let v = check!(spacewasm::Vec::<spacewasm::HostModule>::new(len).map_err(status::alloc_status));
    // Safety, dest must be a valid pointer
    unsafe {
        core::ptr::write(dest, v.into());
    };

    spacewasm_status_t::SPACEWASM_OK
}

/// Add a host module named `name` sized for `max_functions` functions and `max_globals` globals
/// writing its index to `out_idx` (if non-null).
///
/// # Safety
/// `host` must be live; all C strings valid and NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_add_host_module(
    host: *mut spacewasm_host_t,
    name: *const c_char,
    max_functions: u32,
    max_globals: u32,
    out_idx: *mut u32,
) -> spacewasm_status_t {
    let functions = check!(spacewasm::Vec::new(max_functions).map_err(status::alloc_status));
    let globals = check!(spacewasm::Vec::new(max_globals).map_err(status::alloc_status));
    let name = check!(unsafe { abi::cstr(name) });
    let name = check!(spacewasm::HostName::try_from_str(name).map_err(status::host_name_status));

    let module = spacewasm::HostModule {
        name,
        globals,
        functions,
        memory: spacewasm::Vec::zero(),
        table: spacewasm::Vec::zero(),
    };

    let host: &mut spacewasm::Vec<spacewasm::HostModule> =
        check!(unsafe { host.as_mut() }.ok_or(spacewasm_status_t::SPACEWASM_ERR_NULL_ARG)).into();
    check!(
        host.try_push(module)
            .ok()
            .ok_or(spacewasm_status_t::SPACEWASM_ERR_CAPACITY)
    );

    if let Some(out_idx) = unsafe { out_idx.as_mut() } {
        *out_idx = (host.len() - 1) as u32;
    }

    spacewasm_status_t::SPACEWASM_OK
}

/// Register a host function `name` in host module `module_idx`, with parameter
/// and return signatures given by `params_sig`/`returns_sig` and implemented by
/// callback `f` (passed `userdata` on each call).
///
/// # Safety
/// `host` must be live; all C strings valid and NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_add_host_function(
    host: *mut spacewasm_host_t,
    module_idx: u32,
    name: *const c_char,
    params_sig: *const c_char,
    returns_sig: *const c_char,
    f: spacewasm_host_fn_t,
    userdata: *mut c_void,
) -> spacewasm_status_t {
    let name = check!(unsafe { abi::cstr(name) });
    let params_sig = check!(unsafe { abi::cstr(params_sig) });
    let returns_sig = check!(unsafe { abi::cstr(returns_sig) });

    let name = check!(spacewasm::HostName::try_from_str(name).map_err(status::host_name_status));
    let params =
        check!(spacewasm::HostValList::try_new(params_sig).map_err(status::host_val_list_status));
    let returns =
        check!(spacewasm::HostValList::try_new(returns_sig).map_err(status::host_val_list_status));

    let host: &mut spacewasm::Vec<spacewasm::HostModule> =
        check!(unsafe { host.as_mut() }.ok_or(spacewasm_status_t::SPACEWASM_ERR_NULL_ARG)).into();

    let f = check!(f.ok_or(status::SPACEWASM_ERR_NULL_ARG));

    let trampoline = CHostFunction::new(f, userdata);
    let host_fn = match HostFunction::try_new(name, params, returns, move |state, args| {
        trampoline.call(state, args)
    })
    .map_err(status::host_val_list_status)
    {
        Ok(f) => f,
        Err(e) => return e,
    };

    match host.get_mut(module_idx as usize) {
        Some(m) => match m.functions.try_push(host_fn) {
            Ok(()) => spacewasm_status_t::SPACEWASM_OK,
            Err(_) => spacewasm_status_t::SPACEWASM_ERR_CAPACITY,
        },
        None => spacewasm_status_t::SPACEWASM_ERR_NOT_FOUND,
    }
}

/// Load a guest module named `name` onto an existing store by streaming its
/// bytes through the `read` callback. The callback owns the buffer backing each
/// chunk (see [`spacewasm_read_fn_t`]). This does not run the module's start
/// function; use [`spacewasm_store_module_needs_start`] and
/// [`spacewasm_store_run_start`] for that. `allocator` supplies the guest linear
/// memory (see [`spacewasm_allocator_new`]). Writes the new module's index to
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
    allocator: *mut SpacewasmAllocator,
    out_module_idx: *mut u32,
) -> spacewasm_status_t {
    // SAFETY: `allocator` is null or a live handle per the contract.
    let Some(alloc) = (unsafe { alloc::allocator_clone_rc(allocator) }) else {
        return status::SPACEWASM_ERR_NULL_ARG;
    };
    unsafe { abi::store_load_module(store, name, read, read_userdata, alloc, out_module_idx) }
}

/// Consume the host module vector `host` and finish it into a store handle,
/// written to `out_store`. The store is sized with a `stack_size`-byte guest
/// stack, room for `max_modules` guest modules (≤ 256), and `max_code_pages`
/// compiled code pages. No guest module is loaded yet; use
/// [`spacewasm_store_load_module`] to load one or more.
///
/// `host` may be null to create a store with no host modules. The host vector
/// is always consumed (its handle must not be used or destroyed afterwards),
/// whether or not the store is created successfully.
///
/// # Safety
/// `host` must be null or a live handle from [`spacewasm_host_new`], not already
/// consumed/destroyed; `out_store` must be a valid pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_store_new(
    host: *mut spacewasm_host_t,
    stack_size: usize,
    max_modules: u32,
    max_code_pages: u32,
    out_store: *mut *mut SpacewasmStore,
) -> spacewasm_status_t {
    if out_store.is_null() {
        return status::SPACEWASM_ERR_NULL_ARG;
    }

    // Take ownership of the host modules (consuming the handle), or start from
    // an empty set when none were supplied.
    let host_modules: spacewasm::Vec<spacewasm::HostModule> = if host.is_null() {
        check!(spacewasm::Vec::new(0).map_err(status::alloc_status))
    } else {
        unsafe { host.read() }.into()
    };

    let store = check!(SpacewasmStore::new(
        stack_size,
        max_modules as usize,
        max_code_pages,
        host_modules,
    ));

    let boxed = check!(spacewasm::Box::new(store).map_err(status::alloc_status));
    unsafe { *out_store = spacewasm::Box::leak(boxed) as *mut SpacewasmStore };
    status::SPACEWASM_OK
}

/// Destroy a host vector that was never consumed into a store. No-op on null.
///
/// # Safety
/// `host` must be a live handle, not already consumed/destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_host_destroy(host: *mut spacewasm_host_t) {
    if host.is_null() {
        return;
    }
    // `spacewasm_host_t` is a plain `#[repr(C)]` struct with no `Drop`; convert
    // to the owning `Vec` so its allocation (and each `HostModule`) is freed.
    let modules: spacewasm::Vec<spacewasm::HostModule> = unsafe { host.read() }.into();
    core::mem::drop(modules);
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
