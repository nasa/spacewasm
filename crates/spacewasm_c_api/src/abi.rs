//! Generic implementation helpers behind the C entry points.

use core::ffi::{c_char, c_void};

use spacewasm::{Rc, ValType, Value, WasmMemoryAllocator};

use crate::engine::SpacewasmStore;
use crate::status::{self, spacewasm_run_status_t, spacewasm_status_t, spacewasm_trap_t};
use crate::stream::{CallbackStream, spacewasm_read_fn_t};
use crate::value::{spacewasm_valtype_t, spacewasm_value_t};

/// Interpret a NUL-terminated C string as a Rust `&str`.
///
/// # Safety
/// `ptr` must be NUL-terminated and valid, or null.
pub(crate) unsafe fn cstr<'a>(ptr: *const c_char) -> Result<&'a str, spacewasm_status_t> {
    if ptr.is_null() {
        return Err(status::SPACEWASM_ERR_NULL_ARG);
    }
    // SAFETY: caller guarantees NUL-termination and validity.
    let c = unsafe { core::ffi::CStr::from_ptr(ptr) };
    c.to_str().map_err(|_| status::SPACEWASM_ERR_BAD_UTF8)
}

/// Stream + compile a guest module via the `read` callback onto an existing
/// store. Does not run the module's start function. Writes the new module's
/// index to `out_module_idx` (if non-null).
///
/// # Safety
/// `store` must be a live pointer from [`SpacewasmStore::new`]; `name` a valid C
/// string; `read` a valid callback; `out_module_idx` null or a valid `u32`
/// pointer. `alloc` supplies the guest linear-memory allocator.
pub unsafe fn store_load_module(
    store: *mut SpacewasmStore,
    name: *const c_char,
    read: spacewasm_read_fn_t,
    read_userdata: *mut c_void,
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

    let mut stream = match CallbackStream::new(read, read_userdata) {
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
/// `store` must be live
pub unsafe fn store_invoke_start(
    store: *mut SpacewasmStore,
    module_idx: u32,
) -> spacewasm_run_status_t {
    let Some(store) = (unsafe { store.as_mut() }) else {
        return spacewasm_run_status_t::SPACEWASM_RUN_TRAP;
    };

    store.invoke_start(module_idx)
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

    let mut buf: [Value; 64] = [Value::I32(0); 64];
    if n > buf.len() {
        return status::SPACEWASM_ERR_CAPACITY;
    }

    // Marshal parameters. `from_raw_parts` requires a non-null pointer even for
    // a zero-length slice, so only build the slice when there are entries (the
    // contract permits a null `params` when `n == 0`).
    if n != 0 {
        let slice = unsafe { core::slice::from_raw_parts(params, n) };
        for (i, v) in slice.iter().enumerate() {
            buf[i] = v.to_value();
        }
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
