//! Generic implementation helpers behind the C entry points.

use core::ffi::{c_char, c_void};

use spacewasm::{HostName, HostValList, Rc, ValType, Value, WasmMemoryAllocator};

use crate::engine::{Builder, SpacewasmStore, spacewasm_host_fn_t};
use crate::status::{self, spacewasm_run_status_t, spacewasm_status_t, spacewasm_trap_t};
use crate::stream::{CallbackStream, spacewasm_read_fn_t};
use crate::value::{spacewasm_valtype_t, spacewasm_value_t};

/// Interpret a NUL-terminated C string as a Rust `&str`.
///
/// # Safety
/// `ptr` must be NUL-terminated and valid, or null.
unsafe fn cstr<'a>(ptr: *const c_char) -> Result<&'a str, spacewasm_status_t> {
    if ptr.is_null() {
        return Err(status::SPACEWASM_ERR_NULL_ARG);
    }
    // SAFETY: caller guarantees NUL-termination and validity.
    let c = unsafe { core::ffi::CStr::from_ptr(ptr) };
    c.to_str().map_err(|_| status::SPACEWASM_ERR_BAD_UTF8)
}

/// Allocate a `Builder` on the host heap and return an owning raw pointer.
pub fn builder_new(max_modules: usize, max_host_modules: u32) -> *mut Builder {
    match Builder::new(max_modules, max_host_modules) {
        Ok(b) => spacewasm::Box::new(b)
            .map(|boxed| spacewasm::Box::leak(boxed) as *mut Builder)
            .unwrap_or(core::ptr::null_mut()),
        Err(_) => core::ptr::null_mut(),
    }
}

/// # Safety
/// `builder` must be a live pointer from [`builder_new`]; `name` a valid C
/// string; `out_idx` null or a valid `u32` pointer.
pub unsafe fn builder_add_host_module(
    builder: *mut Builder,
    name: *const c_char,
    max_functions: u32,
    out_idx: *mut u32,
) -> spacewasm_status_t {
    let Some(builder) = (unsafe { builder.as_mut() }) else {
        return status::SPACEWASM_ERR_NULL_ARG;
    };
    let name = match unsafe { cstr(name) } {
        Ok(s) => s,
        Err(e) => return e,
    };
    let name = match HostName::try_from_str(name) {
        Ok(n) => n,
        Err(e) => return status::host_name_status(e),
    };
    match builder.add_host_module(name, max_functions) {
        Ok(idx) => {
            if !out_idx.is_null() {
                unsafe { *out_idx = idx };
            }
            status::SPACEWASM_OK
        }
        Err(e) => e,
    }
}

/// # Safety
/// `builder` must be live; `name`/`params_sig`/`returns_sig` valid C strings.
pub unsafe fn builder_add_host_function(
    builder: *mut Builder,
    module_idx: u32,
    name: *const c_char,
    params_sig: *const c_char,
    returns_sig: *const c_char,
    f: spacewasm_host_fn_t,
    userdata: *mut c_void,
) -> spacewasm_status_t {
    let Some(builder) = (unsafe { builder.as_mut() }) else {
        return status::SPACEWASM_ERR_NULL_ARG;
    };
    let name = match unsafe { cstr(name) } {
        Ok(s) => s,
        Err(e) => return e,
    };
    let params_sig = match unsafe { cstr(params_sig) } {
        Ok(s) => s,
        Err(e) => return e,
    };
    let returns_sig = match unsafe { cstr(returns_sig) } {
        Ok(s) => s,
        Err(e) => return e,
    };

    let name = match HostName::try_from_str(name) {
        Ok(n) => n,
        Err(e) => return status::host_name_status(e),
    };
    let params = match HostValList::try_new(params_sig) {
        Ok(p) => p,
        Err(e) => return status::host_val_list_status(e),
    };
    let returns = match HostValList::try_new(returns_sig) {
        Ok(r) => r,
        Err(e) => return status::host_val_list_status(e),
    };

    match builder.add_host_function(module_idx, name, params, returns, f, userdata) {
        Ok(()) => status::SPACEWASM_OK,
        Err(e) => e,
    }
}

/// Consume the builder, build the host modules, and allocate the store handle
/// (with a `stack_size`-byte guest stack and room for `max_code_pages` compiled
/// code pages), writing it to `out_store`. No guest module is loaded yet; use
/// [`store_load_module`] for that.
///
/// # Safety
/// `builder` must be a live pointer from [`builder_new`] (consumed here);
/// `out_store` a valid pointer.
pub unsafe fn builder_finish(
    builder: *mut Builder,
    stack_size: usize,
    max_code_pages: u32,
    out_store: *mut *mut SpacewasmStore,
) -> spacewasm_status_t {
    if builder.is_null() || out_store.is_null() {
        // Free the builder if it was provided but we cannot proceed.
        if !builder.is_null() {
            unsafe { builder_destroy(builder) };
        }
        return status::SPACEWASM_ERR_NULL_ARG;
    }

    // Reclaim ownership of the builder (consumes it).
    let builder = unsafe { spacewasm::Box::from_raw(spacewasm::GlobalAllocator, builder) };
    let builder = spacewasm::Box::into_inner(builder);

    match builder.finish(stack_size, max_code_pages) {
        Ok(store) => {
            unsafe { *out_store = spacewasm::Box::leak(store) as *mut SpacewasmStore };
            status::SPACEWASM_OK
        }
        Err(e) => e,
    }
}

/// Stream + compile a guest module via the `read` callback (`chunk_size` sizes
/// the scratch buffer, 0 selects a default) onto an existing store. Does not run
/// the module's start function. Writes the new module's index to
/// `out_module_idx` (if non-null).
///
/// # Safety
/// `store` must be a live pointer from [`builder_finish`]; `name` a valid C
/// string; `read` a valid callback; `out_module_idx` null or a valid `u32`
/// pointer. `alloc` supplies the guest linear-memory allocator.
#[allow(clippy::too_many_arguments)]
pub unsafe fn store_load_module(
    store: *mut SpacewasmStore,
    name: *const c_char,
    read: spacewasm_read_fn_t,
    read_userdata: *mut c_void,
    chunk_size: usize,
    alloc: Rc<dyn WasmMemoryAllocator>,
    out_module_idx: *mut u32,
) -> spacewasm_status_t {
    let Some(store) = (unsafe { store.as_mut() }) else {
        return status::SPACEWASM_ERR_NULL_ARG;
    };

    let name = match unsafe { cstr(name) } {
        Ok(s) => s,
        Err(e) => return e,
    };

    let mut stream = match CallbackStream::new(read, read_userdata, chunk_size) {
        Ok(s) => s,
        Err(e) => return e,
    };

    match store.load_module(name, &mut stream, alloc) {
        Ok(idx) => {
            if !out_module_idx.is_null() {
                unsafe { *out_module_idx = idx };
            }
            status::SPACEWASM_OK
        }
        // If the callback reported an I/O error, surface that rather than a
        // generic parse failure.
        Err(e) if stream.errored() => {
            let _ = e;
            status::SPACEWASM_ERR_STREAM
        }
        Err(e) => e,
    }
}

/// # Safety
/// `builder` must be a live pointer from [`builder_new`] not already consumed.
pub unsafe fn builder_destroy(builder: *mut Builder) {
    if builder.is_null() {
        return;
    }
    // Reclaim and drop.
    let _ = unsafe { spacewasm::Box::from_raw(spacewasm::GlobalAllocator, builder) };
}

/// # Safety
/// `store` must be live; `name` a valid C string; `out_index` valid.
pub unsafe fn store_find_export_func(
    store: *mut SpacewasmStore,
    module_idx: u32,
    name: *const c_char,
    out_index: *mut u32,
) -> spacewasm_status_t {
    let Some(store) = (unsafe { store.as_ref() }) else {
        return status::SPACEWASM_ERR_NULL_ARG;
    };
    if out_index.is_null() {
        return status::SPACEWASM_ERR_NULL_ARG;
    }
    let name = match unsafe { cstr(name) } {
        Ok(s) => s,
        Err(e) => return e,
    };
    match store.find_export_func(module_idx, name) {
        Ok(idx) => {
            unsafe { *out_index = idx as u32 };
            status::SPACEWASM_OK
        }
        Err(e) => e,
    }
}

/// # Safety
/// `store` must be live; `out_needs_start` a valid `bool` pointer.
pub unsafe fn store_module_needs_start(
    store: *mut SpacewasmStore,
    module_idx: u32,
    out_needs_start: *mut bool,
) -> spacewasm_status_t {
    let Some(store) = (unsafe { store.as_ref() }) else {
        return status::SPACEWASM_ERR_NULL_ARG;
    };
    if out_needs_start.is_null() {
        return status::SPACEWASM_ERR_NULL_ARG;
    }
    match store.module_needs_start(module_idx) {
        Ok(needs) => {
            unsafe { *out_needs_start = needs };
            status::SPACEWASM_OK
        }
        Err(e) => e,
    }
}

/// # Safety
/// `store` must be live; `out_trap` null or a valid `spacewasm_trap_t` pointer.
pub unsafe fn store_run_start(
    store: *mut SpacewasmStore,
    module_idx: u32,
    fuel: usize,
    out_trap: *mut spacewasm_trap_t,
) -> spacewasm_run_status_t {
    let Some(store) = (unsafe { store.as_mut() }) else {
        return spacewasm_run_status_t::SPACEWASM_RUN_TRAP;
    };
    let (rs, trap) = store.run_start(module_idx, fuel);
    if !out_trap.is_null() {
        unsafe { *out_trap = trap };
    }
    rs
}

/// # Safety
/// `store` must be live; `params` valid for `n` entries (or null if `n==0`).
pub unsafe fn store_invoke(
    store: *mut SpacewasmStore,
    module_idx: u32,
    func_index: u32,
    params: *const spacewasm_value_t,
    n: usize,
) -> spacewasm_status_t {
    let Some(store) = (unsafe { store.as_mut() }) else {
        return status::SPACEWASM_ERR_NULL_ARG;
    };
    if params.is_null() && n != 0 {
        return status::SPACEWASM_ERR_NULL_ARG;
    }
    if func_index > u16::MAX as u32 {
        return status::SPACEWASM_ERR_BAD_ARG;
    }

    // Marshal parameters.
    let slice = unsafe { core::slice::from_raw_parts(params, n) };
    let mut buf: [Value; 64] = [Value::I32(0); 64];
    if n > buf.len() {
        return status::SPACEWASM_ERR_CAPACITY;
    }
    for (i, v) in slice.iter().enumerate() {
        buf[i] = v.to_value();
    }

    match store.invoke(module_idx, func_index as u16, &buf[..n]) {
        Ok(()) => status::SPACEWASM_OK,
        Err(e) => e,
    }
}

/// # Safety
/// `store` must be live; `out_trap` null or a valid `spacewasm_trap_t` pointer.
pub unsafe fn store_run(
    store: *mut SpacewasmStore,
    fuel: usize,
    out_trap: *mut spacewasm_trap_t,
) -> spacewasm_run_status_t {
    let Some(store) = (unsafe { store.as_mut() }) else {
        return spacewasm_run_status_t::SPACEWASM_RUN_TRAP;
    };
    let (rs, trap) = store.run(fuel);
    if !out_trap.is_null() {
        unsafe { *out_trap = trap };
    }
    rs
}

/// Run repeatedly with `fuel_per_slice` until the interpreter stops requesting
/// more fuel (i.e. it finishes, traps, pauses, or errors).
///
/// # Safety
/// See [`store_run`].
pub unsafe fn store_run_to_completion(
    store: *mut SpacewasmStore,
    fuel_per_slice: usize,
    out_trap: *mut spacewasm_trap_t,
) -> spacewasm_run_status_t {
    let Some(store) = (unsafe { store.as_mut() }) else {
        return spacewasm_run_status_t::SPACEWASM_RUN_TRAP;
    };
    let fuel = if fuel_per_slice == 0 {
        usize::MAX
    } else {
        fuel_per_slice
    };
    loop {
        let (rs, trap) = store.run(fuel);
        if rs != spacewasm_run_status_t::SPACEWASM_RUN_OUT_OF_FUEL {
            if !out_trap.is_null() {
                unsafe { *out_trap = trap };
            }
            return rs;
        }
    }
}

/// # Safety
/// `store` must be live; `out` a valid `spacewasm_value_t` pointer.
pub unsafe fn store_get_result(
    store: *mut SpacewasmStore,
    expected: spacewasm_valtype_t,
    out: *mut spacewasm_value_t,
) -> spacewasm_status_t {
    let Some(store) = (unsafe { store.as_ref() }) else {
        return status::SPACEWASM_ERR_NULL_ARG;
    };
    if out.is_null() {
        return status::SPACEWASM_ERR_NULL_ARG;
    }
    match store.get_result(ValType::from(expected)) {
        Some(v) => {
            unsafe { *out = v };
            status::SPACEWASM_OK
        }
        None => status::SPACEWASM_ERR_NOT_FOUND,
    }
}

/// # Safety
/// `store` must be a live pointer from [`builder_finish`], not already destroyed.
pub unsafe fn store_destroy(store: *mut SpacewasmStore) {
    if store.is_null() {
        return;
    }
    let _ = unsafe { spacewasm::Box::from_raw(spacewasm::GlobalAllocator, store) };
}
