//! End-to-end tests for the C ABI, driven from Rust.
#![cfg(test)]

extern crate std;

use core::ffi::c_void;
use std::alloc::{Layout, alloc, dealloc, realloc};
use std::sync::Mutex;

use crate::SpacewasmAllocator;
use crate::capi::*;
use crate::engine::{
    SpacewasmCaller, SpacewasmStore, spacewasm_compiler_options_t, spacewasm_hostcall_result_t,
};
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

/// `(module (func (export "boom") (result i32) unreachable))` — traps on call.
#[rustfmt::skip]
static TRAP_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x60, 0x00, 0x01,
    0x7f, 0x03, 0x02, 0x01, 0x00, 0x07, 0x08, 0x01, 0x04, 0x62, 0x6f, 0x6f, 0x6d, 0x00,
    0x00, 0x0a, 0x05, 0x01, 0x03, 0x00, 0x00, 0x0b,
];

/// A module with a `start` function that writes `42` to linear memory at offset
/// 0, exporting `memory` and a `get` function that reads it back.
#[rustfmt::skip]
static START_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x60, 0x00, 0x00,
    0x60, 0x00, 0x01, 0x7f, 0x03, 0x03, 0x02, 0x00, 0x01, 0x05, 0x03, 0x01, 0x00, 0x01,
    0x07, 0x07, 0x01, 0x03, 0x67, 0x65, 0x74, 0x00, 0x01, 0x08, 0x01, 0x00, 0x0a, 0x13,
    0x02, 0x09, 0x00, 0x41, 0x00, 0x41, 0x2a, 0x36, 0x02, 0x00, 0x0b, 0x07, 0x00, 0x41,
    0x00, 0x28, 0x02, 0x00, 0x0b,
];

/// `(module (func (export "spin") (param i32) (result i32) ...))` — busy-loops
/// `param` times. Used to exercise fuel slicing (out-of-fuel, resume).
#[rustfmt::skip]
static LOOP_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x06, 0x01, 0x60, 0x01, 0x7f,
    0x01, 0x7f, 0x03, 0x02, 0x01, 0x00, 0x07, 0x08, 0x01, 0x04, 0x73, 0x70, 0x69, 0x6e,
    0x00, 0x00, 0x0a, 0x1e, 0x01, 0x1c, 0x01, 0x01, 0x7f, 0x02, 0x40, 0x03, 0x40, 0x20,
    0x01, 0x20, 0x00, 0x4f, 0x0d, 0x01, 0x20, 0x01, 0x41, 0x01, 0x6a, 0x21, 0x01, 0x0c,
    0x00, 0x0b, 0x0b, 0x20, 0x01, 0x0b,
];

/// `(module (func $s unreachable) (start $s))` — its start function traps.
#[rustfmt::skip]
static TRAP_START_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x04, 0x01, 0x60, 0x00, 0x00,
    0x03, 0x02, 0x01, 0x00, 0x08, 0x01, 0x00, 0x0a, 0x05, 0x01, 0x03, 0x00, 0x00, 0x0b,
];

// ---- shared driving helpers -------------------------------------------------

/// Default compiler options bounding a test store to `max_code_pages` pages.
fn opts(max_code_pages: u32) -> spacewasm_compiler_options_t {
    spacewasm_compiler_options_t {
        allow_memory_grow: false,
        max_backpatch_iterations: 0,
        max_code_pages,
    }
}

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
            opts(max_code_pages),
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

    let run = unsafe { spacewasm_store_module_invoke_start(store, idx) };

    // The error codes don't really matter, just spin the start function
    match run {
        spacewasm_run_status_t::SPACEWASM_RUN_FINISHED => Ok(idx),
        spacewasm_run_status_t::SPACEWASM_RUN_OUT_OF_FUEL => {
            // We must spin the start function
            loop {
                let mut trap = spacewasm_trap_t::SPACEWASM_TRAP_NONE;
                let run = unsafe { spacewasm_store_run(store, 10000, &mut trap) };
                if run == spacewasm_run_status_t::SPACEWASM_RUN_FINISHED {
                    break Ok(idx);
                } else if run != spacewasm_run_status_t::SPACEWASM_RUN_OUT_OF_FUEL {
                    break Err(status::SPACEWASM_ERR_WRONG_STATE);
                }
            }
        }
        spacewasm_run_status_t::SPACEWASM_RUN_PAUSE => Err(status::SPACEWASM_ERR_WRONG_STATE),
        spacewasm_run_status_t::SPACEWASM_RUN_TRAP => Err(status::SPACEWASM_ERR_WRONG_STATE),
        spacewasm_run_status_t::SPACEWASM_RUN_READER_ERROR => Err(status::SPACEWASM_ERR_STREAM),
    }
}

fn run_to_completion(
    store: *mut SpacewasmStore,
    trap: &mut spacewasm_trap_t,
) -> spacewasm_run_status_t {
    loop {
        let run = unsafe { spacewasm_store_run(store, 10000, trap) };
        if run != spacewasm_run_status_t::SPACEWASM_RUN_OUT_OF_FUEL {
            break run;
        }
    }
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
    let run = run_to_completion(store, &mut trap);
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
        unsafe { spacewasm_store_new(host.as_mut_ptr(), 1024, 1, opts(256), &mut store) },
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
        run_to_completion(store, &mut trap),
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
        unsafe { spacewasm_store_new(host.as_mut_ptr(), 1024, 257, opts(256), &mut store) },
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
        unsafe { spacewasm_store_new(host.as_mut_ptr(), 1024, 1, opts(256), &mut store) },
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

// ---- pure status-mapping tests ----------------------------------------------

#[test]
fn trap_reason_codes_map() {
    use spacewasm::TrapReason::*;
    let cases = [
        (Unreachable, spacewasm_trap_t::SPACEWASM_TRAP_UNREACHABLE),
        (Host, spacewasm_trap_t::SPACEWASM_TRAP_HOST),
        (
            DivideByZero,
            spacewasm_trap_t::SPACEWASM_TRAP_DIVIDE_BY_ZERO,
        ),
        (
            InvalidTableIndex,
            spacewasm_trap_t::SPACEWASM_TRAP_INVALID_TABLE_INDEX,
        ),
        (
            InvalidTableFunctionType,
            spacewasm_trap_t::SPACEWASM_TRAP_INVALID_TABLE_FUNCTION_TYPE,
        ),
        (
            UninitializedTableElement,
            spacewasm_trap_t::SPACEWASM_TRAP_UNINITIALIZED_TABLE_ELEMENT,
        ),
        (
            GlobalGetFailed,
            spacewasm_trap_t::SPACEWASM_TRAP_GLOBAL_GET_FAILED,
        ),
        (
            GlobalSetFailed,
            spacewasm_trap_t::SPACEWASM_TRAP_GLOBAL_SET_FAILED,
        ),
        (OutOfMemory, spacewasm_trap_t::SPACEWASM_TRAP_OUT_OF_MEMORY),
        (
            MemoryRefNotUnique,
            spacewasm_trap_t::SPACEWASM_TRAP_MEMORY_REF_NOT_UNIQUE,
        ),
        (
            MemoryOutOfBounds,
            spacewasm_trap_t::SPACEWASM_TRAP_MEMORY_OUT_OF_BOUNDS,
        ),
        (
            StackOverflow,
            spacewasm_trap_t::SPACEWASM_TRAP_STACK_OVERFLOW,
        ),
        (
            UnrepresentableResult,
            spacewasm_trap_t::SPACEWASM_TRAP_UNREPRESENTABLE_RESULT,
        ),
        (
            IntegerOverflow,
            spacewasm_trap_t::SPACEWASM_TRAP_INTEGER_OVERFLOW,
        ),
        (
            BadConversionToInteger,
            spacewasm_trap_t::SPACEWASM_TRAP_BAD_CONVERSION_TO_INTEGER,
        ),
    ];
    for (reason, code) in cases {
        assert_eq!(status::trap_reason_code(reason), code, "{reason:?}");
    }
}

#[test]
fn alloc_status_maps() {
    use spacewasm::AllocError::*;
    assert_eq!(
        status::alloc_status(AllocationFailed),
        status::SPACEWASM_ERR_ALLOC_FAILED
    );
    assert_eq!(
        status::alloc_status(OutOfMemory),
        status::SPACEWASM_ERR_OUT_OF_MEMORY
    );
    assert_eq!(
        status::alloc_status(PageTooSmall),
        status::SPACEWASM_ERR_PAGE_TOO_SMALL
    );
}

#[test]
fn memory_status_maps() {
    use spacewasm::MemoryError::*;
    assert_eq!(
        status::memory_status(OutOfBounds),
        status::SPACEWASM_ERR_MEM_OUT_OF_BOUNDS
    );
    assert_eq!(
        status::memory_status(OutOfMemory),
        status::SPACEWASM_ERR_OUT_OF_MEMORY
    );
    assert_eq!(
        status::memory_status(AllocationFailed),
        status::SPACEWASM_ERR_ALLOC_FAILED
    );
    assert_eq!(
        status::memory_status(PageTooSmall),
        status::SPACEWASM_ERR_PAGE_TOO_SMALL
    );
}

#[test]
fn invoke_status_maps() {
    use spacewasm::InvokeError::*;
    assert_eq!(
        status::invoke_status(ParamLenMismatch),
        status::SPACEWASM_ERR_PARAM_LEN_MISMATCH
    );
    assert_eq!(
        status::invoke_status(ParamTypeMismatch),
        status::SPACEWASM_ERR_PARAM_TYPE_MISMATCH
    );
    assert_eq!(
        status::invoke_status(StackOverflow),
        status::SPACEWASM_ERR_STACK_OVERFLOW
    );
}

#[test]
fn simple_error_mappers() {
    use spacewasm::{HostNameError, HostValListError, SectionDecodeError, ValidationError};

    let pe = spacewasm::ParseError::new(0, SectionDecodeError::new(ValidationError::Eof));
    assert_eq!(status::parse_status(&pe), status::SPACEWASM_ERR_PARSE);

    assert_eq!(
        status::host_name_status(HostNameError),
        status::SPACEWASM_ERR_NAME_TOO_LONG
    );
    assert_eq!(
        status::host_val_list_status(HostValListError),
        status::SPACEWASM_ERR_BAD_SIGNATURE
    );
}

#[test]
fn run_status_maps() {
    use spacewasm::InterpreterResult;

    assert_eq!(
        status::run_status(&InterpreterResult::Finished),
        (
            spacewasm_run_status_t::SPACEWASM_RUN_FINISHED,
            spacewasm_trap_t::SPACEWASM_TRAP_NONE
        )
    );
    assert_eq!(
        status::run_status(&InterpreterResult::OutOfFuel),
        (
            spacewasm_run_status_t::SPACEWASM_RUN_OUT_OF_FUEL,
            spacewasm_trap_t::SPACEWASM_TRAP_NONE
        )
    );
    assert_eq!(
        status::run_status(&InterpreterResult::Pause),
        (
            spacewasm_run_status_t::SPACEWASM_RUN_PAUSE,
            spacewasm_trap_t::SPACEWASM_TRAP_NONE
        )
    );
    assert_eq!(
        status::run_status(&InterpreterResult::Trap(
            spacewasm::TrapReason::DivideByZero
        )),
        (
            spacewasm_run_status_t::SPACEWASM_RUN_TRAP,
            spacewasm_trap_t::SPACEWASM_TRAP_DIVIDE_BY_ZERO
        )
    );
    assert_eq!(
        status::run_status(&InterpreterResult::ReaderError(
            spacewasm::IrReaderError::InvalidAddress
        )),
        (
            spacewasm_run_status_t::SPACEWASM_RUN_READER_ERROR,
            spacewasm_trap_t::SPACEWASM_TRAP_NONE
        )
    );
}

// ---- value marshalling tests ------------------------------------------------

#[test]
fn value_round_trips_all_types() {
    use spacewasm::Value;

    let values = [
        Value::I32(-7),
        Value::I64(0x0123_4567_89ab_cdef),
        Value::F32(3.5),
        Value::F64(-2.25),
    ];
    for v in values {
        let c = spacewasm_value_t::from_value(v);
        assert_eq!(c.to_value(), v, "round trip {v:?}");
    }
}

#[test]
fn value_from_raw_reinterprets_by_type() {
    use spacewasm::{RawValue, ValType, Value};

    assert_eq!(
        spacewasm_value_t::from_raw(RawValue::from_i32(-1), ValType::I32).to_value(),
        Value::I32(-1)
    );
    assert_eq!(
        spacewasm_value_t::from_raw(RawValue::from_i64(9), ValType::I64).to_value(),
        Value::I64(9)
    );
    assert_eq!(
        spacewasm_value_t::from_raw(RawValue::from_f32(1.5), ValType::F32).to_value(),
        Value::F32(1.5)
    );
    assert_eq!(
        spacewasm_value_t::from_raw(RawValue::from_f64(6.5), ValType::F64).to_value(),
        Value::F64(6.5)
    );
}

#[test]
fn valtype_conversions_both_directions() {
    use spacewasm::ValType;

    let pairs = [
        (ValType::I32, spacewasm_valtype_t::SPACEWASM_I32),
        (ValType::I64, spacewasm_valtype_t::SPACEWASM_I64),
        (ValType::F32, spacewasm_valtype_t::SPACEWASM_F32),
        (ValType::F64, spacewasm_valtype_t::SPACEWASM_F64),
    ];
    for (vt, c) in pairs {
        assert_eq!(spacewasm_valtype_t::from(vt), c);
        assert_eq!(ValType::from(c), vt);
    }
}

// ---- runtime path tests -----------------------------------------------------

#[test]
fn trap_is_reported() {
    let _guard = ALLOC_LOCK.lock().unwrap();
    ensure_global_allocator();

    let store = new_store(1024, 1, 256);
    let alloc = new_guest_allocator();
    let idx = load_module_onto(alloc, store, c"main", TRAP_WASM, 0).expect("load");

    let mut func = 0u32;
    let st = unsafe { spacewasm_store_find_export_func(store, idx, c"boom".as_ptr(), &mut func) };
    assert_eq!(st, status::SPACEWASM_OK, "find");

    assert_eq!(
        unsafe { spacewasm_store_invoke(store, idx, func, core::ptr::null(), 0) },
        status::SPACEWASM_OK,
        "invoke"
    );
    let mut trap = spacewasm_trap_t::SPACEWASM_TRAP_NONE;
    assert_eq!(
        run_to_completion(store, &mut trap),
        spacewasm_run_status_t::SPACEWASM_RUN_TRAP,
        "should trap"
    );
    assert_eq!(trap, spacewasm_trap_t::SPACEWASM_TRAP_UNREACHABLE);

    unsafe {
        spacewasm_store_destroy(store);
        spacewasm_allocator_destroy(alloc);
    }
}

#[test]
fn module_with_start_runs() {
    let _guard = ALLOC_LOCK.lock().unwrap();
    ensure_global_allocator();

    let store = new_store(1024, 1, 256);
    let alloc = new_guest_allocator();

    // Stream in without auto-running the start function, so we can observe
    // `module_needs_start` and drive `run_start` explicitly.
    let mut cursor = Cursor {
        data: START_WASM,
        pos: 0,
        step: 0,
    };
    let mut idx = 0u32;
    assert_eq!(
        unsafe {
            spacewasm_store_load_module(
                store,
                c"main".as_ptr(),
                Some(cursor_read),
                &mut cursor as *mut Cursor as *mut c_void,
                alloc,
                &mut idx,
            )
        },
        status::SPACEWASM_OK,
        "load"
    );

    assert_eq!(
        unsafe { spacewasm_store_module_invoke_start(store, idx) },
        spacewasm_run_status_t::SPACEWASM_RUN_OUT_OF_FUEL
    );

    // Drive the start function in small fuel slices to also exercise the
    let mut trap = spacewasm_trap_t::SPACEWASM_TRAP_NONE;
    let start_status = run_to_completion(store, &mut trap);
    assert_eq!(
        start_status,
        spacewasm_run_status_t::SPACEWASM_RUN_FINISHED,
        "run_start (trap={trap:?})"
    );

    // The start function wrote 42 to linear memory; `get` reads it back.
    let mut func = 0u32;
    assert_eq!(
        unsafe { spacewasm_store_find_export_func(store, idx, c"get".as_ptr(), &mut func) },
        status::SPACEWASM_OK
    );
    assert_eq!(
        unsafe { spacewasm_store_invoke(store, idx, func, core::ptr::null(), 0) },
        status::SPACEWASM_OK
    );
    assert_eq!(
        run_to_completion(store, &mut trap),
        spacewasm_run_status_t::SPACEWASM_RUN_FINISHED
    );
    let mut out = i32_val(0);
    assert_eq!(
        unsafe { spacewasm_store_get_result(store, spacewasm_valtype_t::SPACEWASM_I32, &mut out) },
        status::SPACEWASM_OK
    );
    assert_eq!(unsafe { out.u.i32_ }, 42, "start wrote 42");

    // A second module with no start reports `needs_start == false`.
    unsafe {
        spacewasm_store_destroy(store);
        spacewasm_allocator_destroy(alloc);
    }
}

#[test]
fn no_start_module_reports_false() {
    let _guard = ALLOC_LOCK.lock().unwrap();
    ensure_global_allocator();

    let store = new_store(1024, 1, 256);
    let alloc = new_guest_allocator();
    let idx = load_module_onto(alloc, store, c"main", ADD_WASM, 0).expect("load");

    // No start function should return finished
    assert_eq!(
        unsafe { spacewasm_store_module_invoke_start(store, idx) },
        spacewasm_run_status_t::SPACEWASM_RUN_FINISHED
    );

    unsafe {
        spacewasm_store_destroy(store);
        spacewasm_allocator_destroy(alloc);
    }
}

#[test]
fn run_slices_out_of_fuel_then_resumes() {
    let _guard = ALLOC_LOCK.lock().unwrap();
    ensure_global_allocator();

    let store = new_store(1024, 1, 256);
    let alloc = new_guest_allocator();
    let idx = load_module_onto(alloc, store, c"main", LOOP_WASM, 0).expect("load");

    let mut func = 0u32;
    assert_eq!(
        unsafe { spacewasm_store_find_export_func(store, idx, c"spin".as_ptr(), &mut func) },
        status::SPACEWASM_OK
    );

    // Spin 5000 iterations; a small per-call fuel budget forces the run to
    // slice, so we observe OUT_OF_FUEL at least once before it finishes.
    let params = [i32_val(5000)];
    assert_eq!(
        unsafe { spacewasm_store_invoke(store, idx, func, params.as_ptr(), params.len()) },
        status::SPACEWASM_OK
    );

    let mut saw_out_of_fuel = false;
    let mut trap = spacewasm_trap_t::SPACEWASM_TRAP_NONE;
    let final_status = loop {
        let rs = unsafe { spacewasm_store_run(store, 64, &mut trap) };
        match rs {
            spacewasm_run_status_t::SPACEWASM_RUN_OUT_OF_FUEL => saw_out_of_fuel = true,
            other => break other,
        }
    };
    assert!(saw_out_of_fuel, "expected at least one out-of-fuel slice");
    assert_eq!(final_status, spacewasm_run_status_t::SPACEWASM_RUN_FINISHED);

    let mut out = i32_val(0);
    assert_eq!(
        unsafe { spacewasm_store_get_result(store, spacewasm_valtype_t::SPACEWASM_I32, &mut out) },
        status::SPACEWASM_OK
    );
    assert_eq!(unsafe { out.u.i32_ }, 5000, "spin(5000)");

    unsafe {
        spacewasm_store_destroy(store);
        spacewasm_allocator_destroy(alloc);
    }
}

/// Host callback that exercises `spacewasm_mem_read`/`write`/`size` against the
/// caller's guest memory, then returns `param + 1` so the `HOST_WASM` guest flow
/// still produces its expected result.
unsafe extern "C" fn mem_probe(
    caller: *mut SpacewasmCaller,
    _userdata: *mut c_void,
    params: *const spacewasm_value_t,
    n: usize,
    out: *mut spacewasm_value_t,
) -> spacewasm_hostcall_result_t {
    if n != 1 {
        return spacewasm_hostcall_result_t::SPACEWASM_TRAP;
    }

    // Memory size is at least one page.
    let mut pages = 0u32;
    assert_eq!(
        unsafe { spacewasm_mem_size(caller, &mut pages) },
        status::SPACEWASM_OK
    );
    assert!(pages >= 1, "guest has memory");

    // Write four bytes high in the page, then read them back.
    let src = [0xDEu8, 0xAD, 0xBE, 0xEF];
    assert_eq!(
        unsafe { spacewasm_mem_write(caller, 1024, src.as_ptr(), src.len()) },
        status::SPACEWASM_OK
    );
    let mut dst = [0u8; 4];
    assert_eq!(
        unsafe { spacewasm_mem_read(caller, 1024, dst.as_mut_ptr(), dst.len()) },
        status::SPACEWASM_OK
    );
    assert_eq!(src, dst, "write/read round trip");

    // Reading past the end of memory is an out-of-bounds error, not a crash.
    let past_end = pages as usize * 65536;
    assert_ne!(
        unsafe { spacewasm_mem_read(caller, past_end as u32, dst.as_mut_ptr(), 4) },
        status::SPACEWASM_OK,
        "out-of-bounds read must fail"
    );

    // NULL caller and NULL buffers are rejected with NULL_ARG.
    assert_eq!(
        unsafe { spacewasm_mem_size(core::ptr::null_mut(), &mut pages) },
        status::SPACEWASM_ERR_NULL_ARG
    );
    assert_eq!(
        unsafe { spacewasm_mem_write(caller, 0, core::ptr::null(), 4) },
        status::SPACEWASM_ERR_NULL_ARG
    );
    assert_eq!(
        unsafe { spacewasm_mem_read(caller, 0, core::ptr::null_mut(), 4) },
        status::SPACEWASM_ERR_NULL_ARG
    );

    let arg = unsafe { (*params).u.i32_ };
    unsafe { *out = i32_val(arg + 1) };
    spacewasm_hostcall_result_t::SPACEWASM_CONTINUE
}

#[test]
fn host_memory_accessors() {
    let _guard = ALLOC_LOCK.lock().unwrap();
    ensure_global_allocator();

    let mut host = core::mem::MaybeUninit::<spacewasm_host_t>::uninit();
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
                c"add_one".as_ptr(),
                c"i".as_ptr(),
                c"i".as_ptr(),
                Some(mem_probe),
                core::ptr::null_mut(),
            ),
            status::SPACEWASM_OK
        );
    }

    let mut store: *mut SpacewasmStore = core::ptr::null_mut();
    assert_eq!(
        unsafe { spacewasm_store_new(host.as_mut_ptr(), 1024, 1, opts(256), &mut store) },
        status::SPACEWASM_OK
    );

    let alloc = new_guest_allocator();
    let idx = load_module_onto(alloc, store, c"main", HOST_WASM, 0).expect("load");

    let mut func = 0u32;
    assert_eq!(
        unsafe { spacewasm_store_find_export_func(store, idx, c"run".as_ptr(), &mut func) },
        status::SPACEWASM_OK
    );
    let params = [i32_val(41)];
    assert_eq!(
        unsafe { spacewasm_store_invoke(store, idx, func, params.as_ptr(), params.len()) },
        status::SPACEWASM_OK
    );
    let mut trap = spacewasm_trap_t::SPACEWASM_TRAP_NONE;
    assert_eq!(
        run_to_completion(store, &mut trap),
        spacewasm_run_status_t::SPACEWASM_RUN_FINISHED,
        "run (trap={trap:?})"
    );
    let mut out = i32_val(0);
    assert_eq!(
        unsafe { spacewasm_store_get_result(store, spacewasm_valtype_t::SPACEWASM_I32, &mut out) },
        status::SPACEWASM_OK
    );
    assert_eq!(unsafe { out.u.i32_ }, 42);

    unsafe {
        spacewasm_store_destroy(store);
        spacewasm_allocator_destroy(alloc);
    }
}

#[test]
fn store_with_null_host() {
    let _guard = ALLOC_LOCK.lock().unwrap();
    ensure_global_allocator();

    // A NULL host makes a store with no host modules.
    let mut store: *mut SpacewasmStore = core::ptr::null_mut();
    assert_eq!(
        unsafe { spacewasm_store_new(core::ptr::null_mut(), 1024, 1, opts(256), &mut store) },
        status::SPACEWASM_OK
    );
    assert!(!store.is_null());

    let alloc = new_guest_allocator();
    let idx = load_module_onto(alloc, store, c"main", ADD_WASM, 0).expect("load");
    let mut func = 0u32;
    assert_eq!(
        unsafe { spacewasm_store_find_export_func(store, idx, c"add".as_ptr(), &mut func) },
        status::SPACEWASM_OK
    );
    assert_eq!(invoke_add(store, idx, func, 2, 3).expect("invoke"), 5);

    unsafe {
        spacewasm_store_destroy(store);
        spacewasm_allocator_destroy(alloc);
    }
}

#[test]
fn invoke_and_result_error_paths() {
    let _guard = ALLOC_LOCK.lock().unwrap();
    ensure_global_allocator();

    let store = new_store(1024, 1, 256);
    let alloc = new_guest_allocator();
    let idx = load_module_onto(alloc, store, c"main", ADD_WASM, 0).expect("load");
    let mut func = 0u32;
    assert_eq!(
        unsafe { spacewasm_store_find_export_func(store, idx, c"add".as_ptr(), &mut func) },
        status::SPACEWASM_OK
    );

    // No invocation yet: get_result has nothing to return.
    let mut out = i32_val(0);
    assert_eq!(
        unsafe { spacewasm_store_get_result(store, spacewasm_valtype_t::SPACEWASM_I32, &mut out) },
        status::SPACEWASM_ERR_NOT_FOUND,
        "no result available"
    );

    // Running while idle (nothing invoked) reports a trap without panicking.
    let mut trap = spacewasm_trap_t::SPACEWASM_TRAP_NONE;
    assert_eq!(
        unsafe { spacewasm_store_run(store, 0, &mut trap) },
        spacewasm_run_status_t::SPACEWASM_RUN_TRAP,
        "run while idle"
    );

    // func_index that does not fit in a u16 is rejected as a bad argument.
    let params = [i32_val(1), i32_val(2)];
    assert_eq!(
        unsafe { spacewasm_store_invoke(store, idx, 0x1_0000, params.as_ptr(), params.len()) },
        status::SPACEWASM_ERR_BAD_ARG,
        "func_index overflow"
    );

    // A missing export is not found.
    let mut nope = 0u32;
    assert_eq!(
        unsafe { spacewasm_store_find_export_func(store, idx, c"missing".as_ptr(), &mut nope) },
        status::SPACEWASM_ERR_NOT_FOUND
    );

    // Invoking, then invoking again before running, is a state error.
    assert_eq!(
        unsafe { spacewasm_store_invoke(store, idx, func, params.as_ptr(), params.len()) },
        status::SPACEWASM_OK
    );
    assert_eq!(
        unsafe { spacewasm_store_invoke(store, idx, func, params.as_ptr(), params.len()) },
        status::SPACEWASM_ERR_WRONG_STATE,
        "double invoke"
    );

    unsafe {
        spacewasm_store_destroy(store);
        spacewasm_allocator_destroy(alloc);
    }
}

#[test]
fn store_new_null_out_and_host_destroy() {
    let _guard = ALLOC_LOCK.lock().unwrap();
    ensure_global_allocator();

    // NULL out_store pointer is rejected up front (does not consume the host).
    let mut host = core::mem::MaybeUninit::<spacewasm_host_t>::uninit();
    assert_eq!(
        unsafe { spacewasm_host_new(1, host.as_mut_ptr()) },
        status::SPACEWASM_OK
    );
    assert_eq!(
        unsafe {
            spacewasm_store_new(host.as_mut_ptr(), 1024, 1, opts(256), core::ptr::null_mut())
        },
        status::SPACEWASM_ERR_NULL_ARG,
        "null out_store"
    );
    // The host was not consumed, so it must still be destroyed by hand.
    unsafe { spacewasm_host_destroy(host.as_mut_ptr()) };

    // Destroying a NULL host is a harmless no-op.
    unsafe { spacewasm_host_destroy(core::ptr::null_mut()) };
}

#[test]
fn add_host_function_not_found_module() {
    let _guard = ALLOC_LOCK.lock().unwrap();
    ensure_global_allocator();

    let mut host = core::mem::MaybeUninit::<spacewasm_host_t>::uninit();
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
        // A NULL callback is rejected.
        assert_eq!(
            spacewasm_add_host_function(
                host.as_mut_ptr(),
                hmod,
                c"f".as_ptr(),
                c"i".as_ptr(),
                c"i".as_ptr(),
                None,
                core::ptr::null_mut(),
            ),
            status::SPACEWASM_ERR_NULL_ARG,
            "null callback"
        );
        // A module index that does not exist is not found.
        assert_eq!(
            spacewasm_add_host_function(
                host.as_mut_ptr(),
                99,
                c"f".as_ptr(),
                c"i".as_ptr(),
                c"i".as_ptr(),
                Some(add_one),
                core::ptr::null_mut(),
            ),
            status::SPACEWASM_ERR_NOT_FOUND,
            "bad module index"
        );
        spacewasm_host_destroy(host.as_mut_ptr());
    }
}

#[test]
fn allocator_new_rejects_null_callbacks() {
    // Any null callback yields a null handle (no allocation performed).
    assert!(
        spacewasm_allocator_new(
            None,
            Some(mem_realloc),
            Some(mem_dealloc),
            core::ptr::null_mut()
        )
        .is_null()
    );
    assert!(
        spacewasm_allocator_new(
            Some(mem_alloc),
            None,
            Some(mem_dealloc),
            core::ptr::null_mut()
        )
        .is_null()
    );
    assert!(
        spacewasm_allocator_new(
            Some(mem_alloc),
            Some(mem_realloc),
            None,
            core::ptr::null_mut()
        )
        .is_null()
    );

    // Destroying a null handle is a no-op.
    unsafe { spacewasm_allocator_destroy(core::ptr::null_mut()) };
}

#[test]
fn set_global_allocator_rejects_null() {
    let _guard = ALLOC_LOCK.lock().unwrap();
    // A null callback is rejected with a non-zero code, leaving any previously
    // installed allocator in place.
    assert_eq!(
        crate::spacewasm_set_global_allocator(None, Some(global_dealloc), core::ptr::null_mut()),
        1
    );
    assert_eq!(
        crate::spacewasm_set_global_allocator(Some(global_alloc), None, core::ptr::null_mut()),
        1
    );
    // Re-establish the valid allocator for any subsequent tests.
    ensure_global_allocator();
}

#[test]
fn start_function_traps() {
    let _guard = ALLOC_LOCK.lock().unwrap();
    ensure_global_allocator();

    let store = new_store(1024, 1, 256);
    let alloc = new_guest_allocator();

    // Load without running the start (load_module_onto would surface the trap
    // as an error); drive run_start ourselves to observe the trap code.
    let mut cursor = Cursor {
        data: TRAP_START_WASM,
        pos: 0,
        step: 0,
    };
    let mut idx = 0u32;
    assert_eq!(
        unsafe {
            spacewasm_store_load_module(
                store,
                c"main".as_ptr(),
                Some(cursor_read),
                &mut cursor as *mut Cursor as *mut c_void,
                alloc,
                &mut idx,
            )
        },
        status::SPACEWASM_OK
    );

    assert_eq!(
        unsafe { spacewasm_store_module_invoke_start(store, idx) },
        spacewasm_run_status_t::SPACEWASM_RUN_OUT_OF_FUEL
    );

    let mut trap = spacewasm_trap_t::SPACEWASM_TRAP_NONE;
    let status = run_to_completion(store, &mut trap);
    assert_eq!(status, spacewasm_run_status_t::SPACEWASM_RUN_TRAP);
    assert_eq!(trap, spacewasm_trap_t::SPACEWASM_TRAP_UNREACHABLE);

    unsafe {
        spacewasm_store_destroy(store);
        spacewasm_allocator_destroy(alloc);
    }
}

#[test]
fn engine_rejects_out_of_range_module() {
    let _guard = ALLOC_LOCK.lock().unwrap();
    ensure_global_allocator();

    let store = new_store(1024, 1, 256);
    let alloc = new_guest_allocator();
    let _ = load_module_onto(alloc, store, c"main", ADD_WASM, 0).expect("load");

    // module_needs_start on an out-of-range module is trap.
    assert_eq!(
        unsafe { spacewasm_store_module_invoke_start(store, 99) },
        spacewasm_run_status_t::SPACEWASM_RUN_TRAP
    );

    // run_start on an out-of-range module traps (no such module to seed).
    let mut trap = spacewasm_trap_t::SPACEWASM_TRAP_NONE;
    assert_eq!(
        unsafe { spacewasm_store_run(store, 0, &mut trap) },
        spacewasm_run_status_t::SPACEWASM_RUN_TRAP
    );

    // invoke on an out-of-range module is NOT_FOUND.
    let params = [i32_val(1), i32_val(2)];
    assert_eq!(
        unsafe { spacewasm_store_invoke(store, 99, 0, params.as_ptr(), params.len()) },
        status::SPACEWASM_ERR_NOT_FOUND
    );

    unsafe {
        spacewasm_store_destroy(store);
        spacewasm_allocator_destroy(alloc);
    }
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
