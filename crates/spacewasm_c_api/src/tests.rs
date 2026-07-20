//! End-to-end tests for the C ABI, driven from Rust.
#![cfg(test)]

extern crate std;

use core::ffi::c_void;
use std::alloc::{Layout, alloc, dealloc, realloc};
use std::sync::Mutex;

use crate::SpacewasmAllocator;
use crate::capi::{
    spacewasm_add_host_function, spacewasm_add_host_module, spacewasm_allocator_destroy,
    spacewasm_allocator_new, spacewasm_host_destroy, spacewasm_host_new, spacewasm_host_t,
    spacewasm_store_destroy, spacewasm_store_find_export_func, spacewasm_store_get_result,
    spacewasm_store_invoke, spacewasm_store_load_module, spacewasm_store_module_needs_start,
    spacewasm_store_new, spacewasm_store_run_start, spacewasm_store_run_to_completion,
};
use crate::engine::{SpacewasmCaller, SpacewasmStore, spacewasm_hostcall_result_t};
use crate::status::{self, spacewasm_run_status_t, spacewasm_status_t, spacewasm_trap_t};
use crate::stream::spacewasm_read_result_t;
use crate::value::{spacewasm_valtype_t, spacewasm_value_payload_t, spacewasm_value_t};

/// Serializes tests against the shared process-wide global allocator. See the
/// module docs: the `no_std` page allocator is strict-LIFO, so allocations from
/// concurrent tests must not interleave.
static ALLOC_LOCK: Mutex<()> = Mutex::new(());

/// Install the `std`-backed global allocator once. Idempotent: registering the
/// same allocator again is harmless, and the C ABI has no "already set" error.
fn ensure_global_allocator() {
    let rc = crate::spacewasm_set_global_allocator(
        Some(global_alloc),
        Some(global_dealloc),
        core::ptr::null_mut(),
    );
    assert_eq!(rc, 0, "set_global_allocator failed");
}

/// A minimum alignment matching what the C suite uses (`sizeof(void*)`).
const MIN_ALIGN: usize = core::mem::align_of::<*mut c_void>();

fn layout(size: usize, align: usize) -> Layout {
    Layout::from_size_align(size, align.max(MIN_ALIGN)).expect("bad layout")
}

/// Page-granularity allocator backing the interpreter's internal Rust
/// allocations (the global allocator wraps this in a page allocator).
unsafe extern "C" fn global_alloc(_userdata: *mut c_void, size: usize, align: usize) -> *mut u8 {
    if size == 0 {
        return core::ptr::null_mut();
    }
    unsafe { alloc(layout(size, align)) }
}

unsafe extern "C" fn global_dealloc(
    _userdata: *mut c_void,
    ptr: *mut u8,
    size: usize,
    align: usize,
) {
    if !ptr.is_null() {
        unsafe { dealloc(ptr, layout(size, align)) }
    }
}

// ---- guest linear-memory allocator callbacks --------------------------------

unsafe extern "C" fn mem_alloc(_userdata: *mut c_void, size: usize, align: usize) -> *mut u8 {
    if size == 0 {
        return core::ptr::null_mut();
    }
    unsafe { alloc(layout(size, align)) }
}

unsafe extern "C" fn mem_realloc(
    _userdata: *mut c_void,
    ptr: *mut u8,
    old_size: usize,
    new_size: usize,
    align: usize,
) -> *mut u8 {
    if ptr.is_null() {
        return unsafe { mem_alloc(core::ptr::null_mut(), new_size, align) };
    }
    unsafe { realloc(ptr, layout(old_size, align), new_size) }
}

unsafe extern "C" fn mem_dealloc(_userdata: *mut c_void, ptr: *mut u8, size: usize, align: usize) {
    if !ptr.is_null() {
        unsafe { dealloc(ptr, layout(size, align)) }
    }
}

fn new_guest_allocator() -> *mut SpacewasmAllocator {
    spacewasm_allocator_new(
        Some(mem_alloc),
        Some(mem_realloc),
        Some(mem_dealloc),
        core::ptr::null_mut(),
    )
}

// ---- value helpers ----------------------------------------------------------

fn i32_val(x: i32) -> spacewasm_value_t {
    spacewasm_value_t {
        tag: spacewasm_valtype_t::SPACEWASM_I32,
        u: spacewasm_value_payload_t { i32_: x },
    }
}

// ---- streaming reader (a cursor over a byte slice) --------------------------

/// A cursor over a byte slice, handing out `step` bytes per read (0 => the whole
/// remaining slice at once). The callback owns the buffer, so it points
/// `out_buf` directly into the slice — no allocation, matching the C cursor.
struct Cursor {
    data: &'static [u8],
    pos: usize,
    step: usize,
}

unsafe extern "C" fn cursor_read(
    userdata: *mut c_void,
    out_buf: *mut *const u8,
    out_len: *mut usize,
) -> spacewasm_read_result_t {
    let c = unsafe { &mut *(userdata as *mut Cursor) };
    let remaining = c.data.len() - c.pos;
    if remaining == 0 {
        unsafe { *out_len = 0 };
        return spacewasm_read_result_t::SPACEWASM_READ_EOF;
    }
    let n = if c.step != 0 && remaining > c.step {
        c.step
    } else {
        remaining
    };
    unsafe {
        *out_buf = c.data.as_ptr().add(c.pos);
        *out_len = n;
    }
    c.pos += n;
    spacewasm_read_result_t::SPACEWASM_READ_OK
}

unsafe extern "C" fn failing_read(
    _userdata: *mut c_void,
    _out_buf: *mut *const u8,
    out_len: *mut usize,
) -> spacewasm_read_result_t {
    unsafe { *out_len = 0 };
    spacewasm_read_result_t::SPACEWASM_READ_ERROR
}

// ---- test wasm modules ------------------------------------------------------

/// `(module (func (export "add") (param i32 i32) (result i32)
///    local.get 0 local.get 1 i32.add))`
#[rustfmt::skip]
static ADD_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x07, 0x01,
    0x60, 0x02, 0x7f, 0x7f, 0x01, 0x7f, 0x03, 0x02, 0x01, 0x00, 0x07,
    0x07, 0x01, 0x03, 0x61, 0x64, 0x64, 0x00, 0x00, 0x0a, 0x09, 0x01,
    0x07, 0x00, 0x20, 0x00, 0x20, 0x01, 0x6a, 0x0b,
];

/// A module importing `env.add_one`, exporting `memory` and a `run` function
/// that calls the import and stores the result to linear memory.
#[rustfmt::skip]
static HOST_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x06, 0x01, 0x60, 0x01, 0x7f,
    0x01, 0x7f, 0x02, 0x0f, 0x01, 0x03, 0x65, 0x6e, 0x76, 0x07, 0x61, 0x64, 0x64, 0x5f,
    0x6f, 0x6e, 0x65, 0x00, 0x00, 0x03, 0x02, 0x01, 0x00, 0x05, 0x03, 0x01, 0x00, 0x01,
    0x07, 0x10, 0x02, 0x06, 0x6d, 0x65, 0x6d, 0x6f, 0x72, 0x79, 0x02, 0x00, 0x03, 0x72,
    0x75, 0x6e, 0x00, 0x01, 0x0a, 0x15, 0x01, 0x13, 0x01, 0x01, 0x7f, 0x20, 0x00, 0x10,
    0x00, 0x21, 0x01, 0x41, 0x00, 0x20, 0x01, 0x36, 0x02, 0x00, 0x20, 0x01, 0x0b,
];

// ---- shared driving helpers -------------------------------------------------

/// Create an empty (no host modules) store with the given capacities.
fn new_store(stack_size: usize, max_modules: u32, max_code_pages: u32) -> *mut SpacewasmStore {
    let mut host = core::mem::MaybeUninit::<spacewasm_host_t>::uninit();
    let st = unsafe { spacewasm_host_new(0, host.as_mut_ptr()) };
    assert_eq!(st, status::SPACEWASM_OK, "host_new");

    let mut store: *mut SpacewasmStore = core::ptr::null_mut();
    let st = unsafe {
        spacewasm_store_new(
            host.as_mut_ptr(),
            stack_size,
            max_modules,
            max_code_pages,
            &mut store,
        )
    };
    assert_eq!(st, status::SPACEWASM_OK, "store_new");
    store
}

/// Stream one module onto an existing store in `step`-byte chunks, then run its
/// start function if it declares one. Returns the module index on success.
fn load_module_onto(
    alloc: *mut SpacewasmAllocator,
    store: *mut SpacewasmStore,
    name: &core::ffi::CStr,
    data: &'static [u8],
    step: usize,
) -> Result<u32, spacewasm_status_t> {
    let mut cursor = Cursor { data, pos: 0, step };
    let mut idx = 0u32;
    let st = unsafe {
        spacewasm_store_load_module(
            store,
            name.as_ptr(),
            Some(cursor_read),
            &mut cursor as *mut Cursor as *mut c_void,
            alloc,
            &mut idx,
        )
    };
    if st != status::SPACEWASM_OK {
        return Err(st);
    }

    let mut needs_start = false;
    let st = unsafe { spacewasm_store_module_needs_start(store, idx, &mut needs_start) };
    if st != status::SPACEWASM_OK {
        return Err(st);
    }
    if needs_start {
        let mut trap = spacewasm_trap_t::SPACEWASM_TRAP_NONE;
        let run = unsafe { spacewasm_store_run_start(store, idx, 0, &mut trap) };
        if run != spacewasm_run_status_t::SPACEWASM_RUN_FINISHED {
            return Err(status::SPACEWASM_ERR_WRONG_STATE);
        }
    }
    Ok(idx)
}

/// Invoke a 2-arg i32 function and run it to completion, returning its result.
fn invoke_add(
    store: *mut SpacewasmStore,
    module: u32,
    func: u32,
    a: i32,
    b: i32,
) -> Result<i32, spacewasm_status_t> {
    let params = [i32_val(a), i32_val(b)];
    let st = unsafe { spacewasm_store_invoke(store, module, func, params.as_ptr(), params.len()) };
    if st != status::SPACEWASM_OK {
        return Err(st);
    }
    let mut trap = spacewasm_trap_t::SPACEWASM_TRAP_NONE;
    let run = unsafe { spacewasm_store_run_to_completion(store, 0, &mut trap) };
    assert_eq!(
        run,
        spacewasm_run_status_t::SPACEWASM_RUN_FINISHED,
        "run (trap={trap:?})"
    );
    let mut out = i32_val(0);
    let st =
        unsafe { spacewasm_store_get_result(store, spacewasm_valtype_t::SPACEWASM_I32, &mut out) };
    if st != status::SPACEWASM_OK {
        return Err(st);
    }
    Ok(unsafe { out.u.i32_ })
}

/// Load `ADD_WASM`, invoke `add(1, 2)`, and tear everything down. Used by the
/// no-leak lifecycle test.
fn run_add_once() {
    let store = new_store(1024, 1, 256);
    let alloc = new_guest_allocator();
    let idx = load_module_onto(alloc, store, c"main", ADD_WASM, 0).expect("load");
    let mut func = 0u32;
    let st = unsafe { spacewasm_store_find_export_func(store, idx, c"add".as_ptr(), &mut func) };
    assert_eq!(st, status::SPACEWASM_OK, "find");
    assert_eq!(invoke_add(store, idx, func, 1, 2).expect("invoke"), 3);
    unsafe {
        spacewasm_store_destroy(store);
        spacewasm_allocator_destroy(alloc);
    }
}

// ---- host callback ----------------------------------------------------------

/// Host implementation of `env.add_one`: returns `param + 1`.
unsafe extern "C" fn add_one(
    _caller: *mut SpacewasmCaller,
    _userdata: *mut c_void,
    params: *const spacewasm_value_t,
    n: usize,
    out: *mut spacewasm_value_t,
) -> spacewasm_hostcall_result_t {
    if n != 1 {
        return spacewasm_hostcall_result_t::SPACEWASM_TRAP;
    }
    let arg = unsafe { (*params).u.i32_ };
    unsafe { *out = i32_val(arg + 1) };
    spacewasm_hostcall_result_t::SPACEWASM_CONTINUE
}

// ---- test cases (one per C `test_*` function) -------------------------------

#[test]
fn add_module_invoke() {
    let _guard = ALLOC_LOCK.lock().unwrap();
    ensure_global_allocator();

    let store = new_store(1024, 1, 256);
    let alloc = new_guest_allocator();

    let idx = load_module_onto(alloc, store, c"main", ADD_WASM, 0).expect("load");
    let mut func = 0u32;
    let st = unsafe { spacewasm_store_find_export_func(store, idx, c"add".as_ptr(), &mut func) };
    assert_eq!(st, status::SPACEWASM_OK, "find");

    assert_eq!(invoke_add(store, idx, func, 20, 22).expect("invoke"), 42);

    unsafe {
        spacewasm_store_destroy(store);
        spacewasm_allocator_destroy(alloc);
    }
}

#[test]
fn two_modules_on_one_store() {
    let _guard = ALLOC_LOCK.lock().unwrap();
    ensure_global_allocator();

    let store = new_store(1024, 2, 256);
    let alloc = new_guest_allocator();

    let a = load_module_onto(alloc, store, c"a", ADD_WASM, 0).expect("load a");
    let b = load_module_onto(alloc, store, c"b", ADD_WASM, 0).expect("load b");
    assert_eq!((a, b), (0, 1), "module indices");

    let mut func_a = 0u32;
    let mut func_b = 0u32;
    unsafe {
        assert_eq!(
            spacewasm_store_find_export_func(store, 0, c"add".as_ptr(), &mut func_a),
            status::SPACEWASM_OK
        );
        assert_eq!(
            spacewasm_store_find_export_func(store, 1, c"add".as_ptr(), &mut func_b),
            status::SPACEWASM_OK
        );
    }

    // Invoke module 1 first, then 0, to prove the index selects the target.
    assert_eq!(invoke_add(store, 1, func_b, 100, 1).expect("b"), 101);
    assert_eq!(invoke_add(store, 0, func_a, 20, 22).expect("a"), 42);

    unsafe {
        spacewasm_store_destroy(store);
        spacewasm_allocator_destroy(alloc);
    }
}

#[test]
fn streaming_load() {
    let _guard = ALLOC_LOCK.lock().unwrap();
    ensure_global_allocator();

    let store = new_store(1024, 1, 256);
    let alloc = new_guest_allocator();

    // Force many small 7-byte chunks.
    let idx = load_module_onto(alloc, store, c"main", ADD_WASM, 7).expect("streaming load");
    let mut func = 0u32;
    let st = unsafe { spacewasm_store_find_export_func(store, idx, c"add".as_ptr(), &mut func) };
    assert_eq!(st, status::SPACEWASM_OK, "find");

    assert_eq!(invoke_add(store, idx, func, 30, 12).expect("invoke"), 42);

    unsafe {
        spacewasm_store_destroy(store);
        spacewasm_allocator_destroy(alloc);
    }
}

#[test]
fn streaming_read_error() {
    let _guard = ALLOC_LOCK.lock().unwrap();
    ensure_global_allocator();

    let store = new_store(1024, 1, 256);
    let alloc = new_guest_allocator();
    assert!(!alloc.is_null(), "allocator_new");

    let mut idx = 0u32;
    let st = unsafe {
        spacewasm_store_load_module(
            store,
            c"main".as_ptr(),
            Some(failing_read),
            core::ptr::null_mut(),
            alloc,
            &mut idx,
        )
    };
    unsafe { spacewasm_allocator_destroy(alloc) };
    assert_eq!(st, status::SPACEWASM_ERR_STREAM, "expected ERR_STREAM");

    unsafe { spacewasm_store_destroy(store) };
}

#[test]
fn host_function_and_memory() {
    let _guard = ALLOC_LOCK.lock().unwrap();
    ensure_global_allocator();

    let mut host = core::mem::MaybeUninit::<spacewasm_host_t>::uninit();
    assert_eq!(
        unsafe { spacewasm_host_new(1, host.as_mut_ptr()) },
        status::SPACEWASM_OK,
        "host_new"
    );

    let mut hmod = 0u32;
    unsafe {
        assert_eq!(
            spacewasm_add_host_module(host.as_mut_ptr(), c"env".as_ptr(), 1, 0, &mut hmod),
            status::SPACEWASM_OK,
            "add_host_module"
        );
        assert_eq!(
            spacewasm_add_host_function(
                host.as_mut_ptr(),
                hmod,
                c"add_one".as_ptr(),
                c"i".as_ptr(),
                c"i".as_ptr(),
                Some(add_one),
                core::ptr::null_mut(),
            ),
            status::SPACEWASM_OK,
            "add_host_function"
        );
    }

    let mut store: *mut SpacewasmStore = core::ptr::null_mut();
    assert_eq!(
        unsafe { spacewasm_store_new(host.as_mut_ptr(), 1024, 1, 256, &mut store) },
        status::SPACEWASM_OK,
        "store_new"
    );

    let alloc = new_guest_allocator();
    let idx = load_module_onto(alloc, store, c"main", HOST_WASM, 0).expect("load host module");

    let mut func = 0u32;
    let st = unsafe { spacewasm_store_find_export_func(store, idx, c"run".as_ptr(), &mut func) };
    assert_eq!(st, status::SPACEWASM_OK, "find run");

    let params = [i32_val(41)];
    assert_eq!(
        unsafe { spacewasm_store_invoke(store, idx, func, params.as_ptr(), params.len()) },
        status::SPACEWASM_OK,
        "invoke"
    );
    let mut trap = spacewasm_trap_t::SPACEWASM_TRAP_NONE;
    assert_eq!(
        unsafe { spacewasm_store_run_to_completion(store, 0, &mut trap) },
        spacewasm_run_status_t::SPACEWASM_RUN_FINISHED,
        "run (trap={trap:?})"
    );
    let mut out = i32_val(0);
    assert_eq!(
        unsafe { spacewasm_store_get_result(store, spacewasm_valtype_t::SPACEWASM_I32, &mut out,) },
        status::SPACEWASM_OK,
        "result"
    );
    assert_eq!(unsafe { out.u.i32_ }, 42, "add_one(41)");

    unsafe {
        spacewasm_store_destroy(store);
        spacewasm_allocator_destroy(alloc);
    }
}

#[test]
fn error_paths() {
    let _guard = ALLOC_LOCK.lock().unwrap();
    ensure_global_allocator();

    // max_modules > 256 -> store_new returns ERR_BAD_ARG (consumes the host).
    let mut host = core::mem::MaybeUninit::<spacewasm_host_t>::uninit();
    assert_eq!(
        unsafe { spacewasm_host_new(0, host.as_mut_ptr()) },
        status::SPACEWASM_OK
    );
    let mut store: *mut SpacewasmStore = core::ptr::null_mut();
    assert_eq!(
        unsafe { spacewasm_store_new(host.as_mut_ptr(), 1024, 257, 256, &mut store) },
        status::SPACEWASM_ERR_BAD_ARG,
        "oversized max_modules"
    );

    // Bad signature char -> ERR_BAD_SIGNATURE, no panic.
    assert_eq!(
        unsafe { spacewasm_host_new(1, host.as_mut_ptr()) },
        status::SPACEWASM_OK
    );
    let mut hmod = 0u32;
    unsafe {
        assert_eq!(
            spacewasm_add_host_module(host.as_mut_ptr(), c"env".as_ptr(), 1, 0, &mut hmod),
            status::SPACEWASM_OK
        );
        assert_eq!(
            spacewasm_add_host_function(
                host.as_mut_ptr(),
                hmod,
                c"bad".as_ptr(),
                c"x".as_ptr(),
                c"".as_ptr(),
                Some(add_one),
                core::ptr::null_mut(),
            ),
            status::SPACEWASM_ERR_BAD_SIGNATURE,
            "bad signature"
        );
        spacewasm_host_destroy(host.as_mut_ptr());
    }

    // Malformed wasm -> parse error; the store is still created fine.
    assert_eq!(
        unsafe { spacewasm_host_new(0, host.as_mut_ptr()) },
        status::SPACEWASM_OK
    );
    assert_eq!(
        unsafe { spacewasm_store_new(host.as_mut_ptr(), 1024, 1, 256, &mut store) },
        status::SPACEWASM_OK,
        "store_new"
    );
    static JUNK: &[u8] = &[0, 1, 2, 3, 4, 5, 6, 7];
    let alloc = new_guest_allocator();
    assert!(!alloc.is_null(), "allocator_new");
    let st = load_module_onto(alloc, store, c"main", JUNK, 0);
    unsafe { spacewasm_allocator_destroy(alloc) };
    assert_eq!(st, Err(status::SPACEWASM_ERR_PARSE), "expected ERR_PARSE");

    unsafe { spacewasm_store_destroy(store) };
}

#[test]
fn null_arg_handling() {
    let _guard = ALLOC_LOCK.lock().unwrap();
    ensure_global_allocator();

    let store = new_store(1024, 1, 256);
    let alloc = new_guest_allocator();
    assert!(!alloc.is_null(), "allocator_new");

    // NULL name to load_module.
    let mut cursor = Cursor {
        data: ADD_WASM,
        pos: 0,
        step: 0,
    };
    let mut idx = 0u32;
    let st = unsafe {
        spacewasm_store_load_module(
            store,
            core::ptr::null(),
            Some(cursor_read),
            &mut cursor as *mut Cursor as *mut c_void,
            alloc,
            &mut idx,
        )
    };
    unsafe { spacewasm_allocator_destroy(alloc) };
    assert_eq!(st, status::SPACEWASM_ERR_NULL_ARG, "null name");

    // NULL store to find_export_func.
    let mut func = 0u32;
    let st = unsafe {
        spacewasm_store_find_export_func(core::ptr::null_mut(), 0, c"add".as_ptr(), &mut func)
    };
    assert_eq!(st, status::SPACEWASM_ERR_NULL_ARG, "null store");

    unsafe { spacewasm_store_destroy(store) };
}

#[test]
fn statistics_available() {
    let _guard = ALLOC_LOCK.lock().unwrap();
    ensure_global_allocator();

    // Just confirm the statistics entry point is wired and returns.
    let stats = crate::spacewasm_memory_statistics();
    let _ = (stats.total_bytes, stats.pad_bytes);
}

/// Create and destroy many stores; the tracked live-byte total must return to
/// its baseline, validating drop order and that names/closures are freed.
#[test]
fn no_leak_across_lifecycle() {
    let _guard = ALLOC_LOCK.lock().unwrap();
    ensure_global_allocator();

    run_add_once(); // absorb one-time allocations
    let baseline = crate::spacewasm_memory_statistics().total_bytes;
    for _ in 0..50 {
        run_add_once();
    }
    let after = crate::spacewasm_memory_statistics().total_bytes;
    assert_eq!(
        after, baseline,
        "memory drifted: baseline={baseline} after={after}"
    );
}
