//! End-to-end tests for the C ABI, driven from Rust.
#![cfg(test)]

extern crate std;

use core::ffi::c_void;
use std::alloc::{Layout, alloc, dealloc, realloc};
use std::sync::Mutex;

use crate::CAllocator;
use crate::capi::*;
use crate::host::{SpacewasmCaller, spacewasm_hostcall_result_t};
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
    assert_eq!(
        rc,
        crate::spacewasm_status_t::SPACEWASM_OK,
        "set_global_allocator failed"
    );
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

fn new_guest_allocator() -> *mut CAllocator {
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

fn i64_val(x: i64) -> spacewasm_value_t {
    spacewasm_value_t {
        tag: spacewasm_valtype_t::SPACEWASM_I64,
        u: spacewasm_value_payload_t { i64_: x },
    }
}

fn f32_val(x: f32) -> spacewasm_value_t {
    spacewasm_value_t {
        tag: spacewasm_valtype_t::SPACEWASM_F32,
        u: spacewasm_value_payload_t { f32_: x },
    }
}

fn f64_val(x: f64) -> spacewasm_value_t {
    spacewasm_value_t {
        tag: spacewasm_valtype_t::SPACEWASM_F64,
        u: spacewasm_value_payload_t { f64_: x },
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

/// A module importing a void `env.sink` (`(param i32)`, no result) and a `run`
/// function that keeps a sentinel on the stack across the call, then returns
/// `param + 1`. If the host call spuriously pushed a value, the sentinel would
/// be corrupted and the result would be wrong.
///
/// ```wat
/// (module
///   (import "env" "sink" (func $sink (param i32)))
///   (func (export "run") (param i32) (result i32)
///     local.get 0 local.get 0 call $sink i32.const 1 i32.add))
/// ```
#[rustfmt::skip]
static VOID_HOST_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x0a, 0x02, 0x60,
    0x01, 0x7f, 0x00, 0x60, 0x01, 0x7f, 0x01, 0x7f, 0x02, 0x0c, 0x01, 0x03,
    0x65, 0x6e, 0x76, 0x04, 0x73, 0x69, 0x6e, 0x6b, 0x00, 0x00, 0x03, 0x02,
    0x01, 0x01, 0x07, 0x07, 0x01, 0x03, 0x72, 0x75, 0x6e, 0x00, 0x01, 0x0a,
    0x0d, 0x01, 0x0b, 0x00, 0x20, 0x00, 0x20, 0x00, 0x10, 0x00, 0x41, 0x01,
    0x6a, 0x0b,
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

/// A module exporting a mutable global `g` (i32, init 7) and a const global `c`
/// (i32, init 42), plus `get_g` / `set_g` accessors that read and write `g`
/// through `global.get` / `global.set`. Used to drive the global C API and to
/// prove `set_global` mutates the same storage the interpreter observes.
///
/// ```wat
/// (module
///   (global $g (export "g") (mut i32) (i32.const 7))
///   (global $c (export "c") i32 (i32.const 42))
///   (func (export "get_g") (result i32) global.get $g)
///   (func (export "set_g") (param i32) local.get 0 global.set $g))
/// ```
#[rustfmt::skip]
static GLOBALS_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x09, 0x02, 0x60,
    0x00, 0x01, 0x7f, 0x60, 0x01, 0x7f, 0x00, 0x03, 0x03, 0x02, 0x00, 0x01,
    0x06, 0x0b, 0x02, 0x7f, 0x01, 0x41, 0x07, 0x0b, 0x7f, 0x00, 0x41, 0x2a,
    0x0b, 0x07, 0x19, 0x04, 0x01, 0x67, 0x03, 0x00, 0x01, 0x63, 0x03, 0x01,
    0x05, 0x67, 0x65, 0x74, 0x5f, 0x67, 0x00, 0x00, 0x05, 0x73, 0x65, 0x74,
    0x5f, 0x67, 0x00, 0x01, 0x0a, 0x0d, 0x02, 0x04, 0x00, 0x23, 0x00, 0x0b,
    0x06, 0x00, 0x20, 0x00, 0x24, 0x00, 0x0b,
];

/// A module exporting one mutable global of each value type, plus a `get_gI`
/// accessor that reads the i64 global. Exercises the get/set C API across all
/// four `RawValue` representations, not just i32.
///
/// ```wat
/// (module
///   (global $gi (export "gi") (mut i32) (i32.const 10))
///   (global $gI (export "gI") (mut i64) (i64.const 20))
///   (global $gf (export "gf") (mut f32) (f32.const 1.5))
///   (global $gd (export "gd") (mut f64) (f64.const 2.5))
///   (func (export "get_gI") (result i64) global.get $gI))
/// ```
#[rustfmt::skip]
static GLOBALS_MULTI_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x60,
    0x00, 0x01, 0x7e, 0x03, 0x02, 0x01, 0x00, 0x06, 0x1f, 0x04, 0x7f, 0x01,
    0x41, 0x0a, 0x0b, 0x7e, 0x01, 0x42, 0x14, 0x0b, 0x7d, 0x01, 0x43, 0x00,
    0x00, 0xc0, 0x3f, 0x0b, 0x7c, 0x01, 0x44, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x04, 0x40, 0x0b, 0x07, 0x1e, 0x05, 0x02, 0x67, 0x69, 0x03, 0x00,
    0x02, 0x67, 0x49, 0x03, 0x01, 0x02, 0x67, 0x66, 0x03, 0x02, 0x02, 0x67,
    0x64, 0x03, 0x03, 0x06, 0x67, 0x65, 0x74, 0x5f, 0x67, 0x49, 0x00, 0x00,
    0x0a, 0x06, 0x01, 0x04, 0x00, 0x23, 0x01, 0x0b,
];

/// Exporter half of the imported-global pair: a module named `b` exporting a
/// single mutable i32 global `bg` (init 55).
///
/// ```wat
/// (module (global (export "bg") (mut i32) (i32.const 55)))
/// ```
#[rustfmt::skip]
static GLOBAL_EXPORTER_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x06, 0x06, 0x01, 0x7f,
    0x01, 0x41, 0x37, 0x0b, 0x07, 0x06, 0x01, 0x02, 0x62, 0x67, 0x03, 0x00,
];

/// Importer half of the imported-global pair: a module importing `b.bg` (global
/// index 0 in its own index space), then defining and exporting its own mutable
/// i32 global `ag` (init 11, module-local index 0), and re-exporting the import
/// under the name `reexport`. Loading it requires the exporter above be present.
///
/// This distinguishes the WebAssembly global *index space* (where `ag` is index
/// 1, after the one imported global) from the module-local `globals` index
/// (where `ag` is 0). `find_global("ag")` must return the module-local `0`, and
/// `find_global("reexport")` must miss, since the target is an imported global.
///
/// ```wat
/// (module
///   (import "b" "bg" (global $ig (mut i32)))
///   (global $ag (export "ag") (mut i32) (i32.const 11))
///   (export "reexport" (global $ig)))
/// ```
#[rustfmt::skip]
static GLOBAL_IMPORTER_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x02, 0x09, 0x01, 0x01,
    0x62, 0x02, 0x62, 0x67, 0x03, 0x7f, 0x01, 0x06, 0x06, 0x01, 0x7f, 0x01,
    0x41, 0x0b, 0x0b, 0x07, 0x11, 0x02, 0x02, 0x61, 0x67, 0x03, 0x01, 0x08,
    0x72, 0x65, 0x65, 0x78, 0x70, 0x6f, 0x72, 0x74, 0x03, 0x00,
];

// ---- shared driving helpers -------------------------------------------------

/// Default compiler options bounding a test store to `max_code_pages` pages.
fn opts(max_code_pages: usize) -> spacewasm_compiler_options_t {
    spacewasm_compiler_options_t {
        allow_memory_grow: false,
        max_backpatch_iterations: 0,
        max_code_pages,
    }
}

/// Create an empty (no host modules) store with the given capacities.
fn new_store(stack_size: usize, max_modules: usize, max_code_pages: usize) -> *mut CEngine {
    let mut host = core::mem::MaybeUninit::<spacewasm_host_t>::uninit();
    let st = unsafe { spacewasm_host_new(0, host.as_mut_ptr()) };
    assert_eq!(st, status::SPACEWASM_OK, "host_new");

    let mut store: *mut CEngine = core::ptr::null_mut();
    let st = unsafe {
        spacewasm_new(
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
    alloc: *mut CAllocator,
    store: *mut CEngine,
    name: &core::ffi::CStr,
    data: &'static [u8],
    step: usize,
) -> Result<u32, spacewasm_status_t> {
    let mut cursor = Cursor { data, pos: 0, step };
    let mut idx = 0u32;
    let st = unsafe {
        spacewasm_load_module(
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

    // Resolve the start function (if any) and drive it to completion. A module
    // without a start function reports NOT_FOUND and needs no initialization.
    let mut start_mod = 0u32;
    let mut start_func = 0u32;
    match unsafe { spacewasm_module_start(store, idx, &mut start_mod, &mut start_func) } {
        status::SPACEWASM_OK => {}
        status::SPACEWASM_ERR_NOT_FOUND => return Ok(idx),
        e => return Err(e),
    }

    let st = unsafe { spacewasm_invoke(store, start_mod, start_func, core::ptr::null(), 0) };
    if st != status::SPACEWASM_OK {
        return Err(st);
    }

    // Spin the start function to completion.
    loop {
        let mut trap = spacewasm_trap_t::SPACEWASM_TRAP_NONE;
        let run = unsafe { spacewasm_run(store, 10000, &mut trap) };
        if run == spacewasm_run_status_t::SPACEWASM_RUN_FINISHED {
            break Ok(idx);
        } else if run != spacewasm_run_status_t::SPACEWASM_RUN_OUT_OF_FUEL {
            break Err(status::SPACEWASM_ERR_WRONG_STATE);
        }
    }
}

fn run_to_completion(store: *mut CEngine, trap: &mut spacewasm_trap_t) -> spacewasm_run_status_t {
    loop {
        let run = unsafe { spacewasm_run(store, 10000, trap) };
        if run != spacewasm_run_status_t::SPACEWASM_RUN_OUT_OF_FUEL {
            break run;
        }
    }
}

/// Invoke a 2-arg i32 function and run it to completion, returning its result.
fn invoke_add(
    store: *mut CEngine,
    module: u32,
    func: u32,
    a: i32,
    b: i32,
) -> Result<i32, spacewasm_status_t> {
    let params = [i32_val(a), i32_val(b)];
    let st = unsafe { spacewasm_invoke(store, module, func, params.as_ptr(), params.len()) };
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
    let st = unsafe { spacewasm_get_result(store, spacewasm_valtype_t::SPACEWASM_I32, &mut out) };
    if st != status::SPACEWASM_OK {
        return Err(st);
    }
    Ok(unsafe { out.u.i32_ })
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
    spacewasm_hostcall_result_t::SPACEWASM_CONTINUE_SOME
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
    let st = unsafe { spacewasm_find_export_func(store, idx, c"add".as_ptr(), &mut func) };
    assert_eq!(st, status::SPACEWASM_OK, "find");

    assert_eq!(invoke_add(store, idx, func, 20, 22).expect("invoke"), 42);

    unsafe {
        spacewasm_destroy(store);
        spacewasm_allocator_destroy(alloc);
    }
}

#[test]
fn check_func_signature() {
    let _guard = ALLOC_LOCK.lock().unwrap();
    ensure_global_allocator();

    let store = new_store(1024, 1, 256);
    let alloc = new_guest_allocator();

    let idx = load_module_onto(alloc, store, c"main", ADD_WASM, 0).expect("load");
    let mut func = 0u32;
    let st = unsafe { spacewasm_find_export_func(store, idx, c"add".as_ptr(), &mut func) };
    assert_eq!(st, status::SPACEWASM_OK, "find");

    // `add` is `(i32, i32) -> i32`.
    unsafe {
        // Correct signature matches.
        assert_eq!(
            spacewasm_check_func_signature(store, idx, func, c"ii".as_ptr(), c"i".as_ptr()),
            status::SPACEWASM_OK,
            "matching signature"
        );

        // Wrong parameter count.
        assert_eq!(
            spacewasm_check_func_signature(store, idx, func, c"i".as_ptr(), c"i".as_ptr()),
            status::SPACEWASM_ERR_PARAM_LEN_MISMATCH,
            "too few params"
        );

        // Wrong return count.
        assert_eq!(
            spacewasm_check_func_signature(store, idx, func, c"ii".as_ptr(), c"".as_ptr()),
            status::SPACEWASM_ERR_PARAM_LEN_MISMATCH,
            "missing return"
        );

        // Right arity, wrong parameter type (i64 instead of i32).
        assert_eq!(
            spacewasm_check_func_signature(store, idx, func, c"iI".as_ptr(), c"i".as_ptr()),
            status::SPACEWASM_ERR_PARAM_TYPE_MISMATCH,
            "wrong param type"
        );

        // Right arity, wrong return type (f32 instead of i32).
        assert_eq!(
            spacewasm_check_func_signature(store, idx, func, c"ii".as_ptr(), c"f".as_ptr()),
            status::SPACEWASM_ERR_PARAM_TYPE_MISMATCH,
            "wrong return type"
        );

        // Malformed signature string: an invalid value-list character.
        assert_eq!(
            spacewasm_check_func_signature(store, idx, func, c"ix".as_ptr(), c"i".as_ptr()),
            status::SPACEWASM_ERR_BAD_SIGNATURE,
            "bad signature char"
        );

        // Out-of-range function index.
        assert_eq!(
            spacewasm_check_func_signature(store, idx, 999, c"ii".as_ptr(), c"i".as_ptr()),
            status::SPACEWASM_ERR_NOT_FOUND,
            "func out of range"
        );

        // Out-of-range module index.
        assert_eq!(
            spacewasm_check_func_signature(store, 999, func, c"ii".as_ptr(), c"i".as_ptr()),
            status::SPACEWASM_ERR_NOT_FOUND,
            "module out of range"
        );

        // NULL engine.
        assert_eq!(
            spacewasm_check_func_signature(
                core::ptr::null_mut(),
                idx,
                func,
                c"ii".as_ptr(),
                c"i".as_ptr()
            ),
            status::SPACEWASM_ERR_NULL_ARG,
            "null engine"
        );
    }

    unsafe {
        spacewasm_destroy(store);
        spacewasm_allocator_destroy(alloc);
    }
}

#[test]
fn global_get_set() {
    let _guard = ALLOC_LOCK.lock().unwrap();
    ensure_global_allocator();

    let store = new_store(1024, 1, 256);
    let alloc = new_guest_allocator();

    let idx = load_module_onto(alloc, store, c"main", GLOBALS_WASM, 0).expect("load");

    // Resolve the exported globals to their module-local indices. `g` is the
    // mutable global (defined first, index 0); `c` is the const global (index 1).
    let mut g = u32::MAX;
    let mut c = u32::MAX;
    unsafe {
        assert_eq!(
            spacewasm_find_global(store, idx, c"g".as_ptr(), &mut g),
            status::SPACEWASM_OK,
            "find g"
        );
        assert_eq!(
            spacewasm_find_global(store, idx, c"c".as_ptr(), &mut c),
            status::SPACEWASM_OK,
            "find c"
        );
    }
    assert_eq!((g, c), (0, 1), "global indices");

    // A missing export reports NOT_FOUND, as does looking up a function export
    // as a global (`get_g` is a function, not a global).
    unsafe {
        let mut sink = 0u32;
        assert_eq!(
            spacewasm_find_global(store, idx, c"nope".as_ptr(), &mut sink),
            status::SPACEWASM_ERR_NOT_FOUND,
            "missing global"
        );
        assert_eq!(
            spacewasm_find_global(store, idx, c"get_g".as_ptr(), &mut sink),
            status::SPACEWASM_ERR_NOT_FOUND,
            "function export is not a global"
        );
    }

    // Read the initial values.
    unsafe {
        let mut out = i32_val(0);
        assert_eq!(
            spacewasm_get_global(store, idx, g, &mut out),
            status::SPACEWASM_OK,
            "get g"
        );
        assert_eq!(out.tag, spacewasm_valtype_t::SPACEWASM_I32, "g tag");
        assert_eq!(out.u.i32_, 7, "g init");

        assert_eq!(
            spacewasm_get_global(store, idx, c, &mut out),
            status::SPACEWASM_OK,
            "get c"
        );
        assert_eq!(out.u.i32_, 42, "c init");
    }

    // Write the mutable global and read it back, both through the C API and by
    // invoking `get_g`, proving `set_global` mutates the storage the
    // interpreter reads.
    unsafe {
        assert_eq!(
            spacewasm_set_global(store, idx, g, i32_val(100)),
            status::SPACEWASM_OK,
            "set g"
        );
        let mut out = i32_val(0);
        assert_eq!(
            spacewasm_get_global(store, idx, g, &mut out),
            status::SPACEWASM_OK,
            "get g after set"
        );
        assert_eq!(out.u.i32_, 100, "g after set");
    }

    let mut get_g = 0u32;
    assert_eq!(
        unsafe { spacewasm_find_export_func(store, idx, c"get_g".as_ptr(), &mut get_g) },
        status::SPACEWASM_OK,
        "find get_g"
    );
    let params: [spacewasm_value_t; 0] = [];
    assert_eq!(
        unsafe { spacewasm_invoke(store, idx, get_g, params.as_ptr(), 0) },
        status::SPACEWASM_OK,
        "invoke get_g"
    );
    let mut trap = spacewasm_trap_t::SPACEWASM_TRAP_NONE;
    assert_eq!(
        run_to_completion(store, &mut trap),
        spacewasm_run_status_t::SPACEWASM_RUN_FINISHED,
        "run get_g"
    );
    let mut out = i32_val(0);
    assert_eq!(
        unsafe { spacewasm_get_result(store, spacewasm_valtype_t::SPACEWASM_I32, &mut out) },
        status::SPACEWASM_OK,
        "result get_g"
    );
    assert_eq!(unsafe { out.u.i32_ }, 100, "get_g observes set_global");

    // Conversely, `set_g` from Wasm is observable through `get_global`.
    let mut set_g = 0u32;
    assert_eq!(
        unsafe { spacewasm_find_export_func(store, idx, c"set_g".as_ptr(), &mut set_g) },
        status::SPACEWASM_OK,
        "find set_g"
    );
    let args = [i32_val(5)];
    assert_eq!(
        unsafe { spacewasm_invoke(store, idx, set_g, args.as_ptr(), 1) },
        status::SPACEWASM_OK,
        "invoke set_g"
    );
    assert_eq!(
        run_to_completion(store, &mut trap),
        spacewasm_run_status_t::SPACEWASM_RUN_FINISHED,
        "run set_g"
    );
    unsafe {
        let mut out = i32_val(0);
        assert_eq!(
            spacewasm_get_global(store, idx, g, &mut out),
            status::SPACEWASM_OK,
            "get g after wasm set"
        );
        assert_eq!(out.u.i32_, 5, "get_global observes set_g");
    }

    // Error cases.
    unsafe {
        // Writing a const global is rejected.
        assert_eq!(
            spacewasm_set_global(store, idx, c, i32_val(1)),
            status::SPACEWASM_ERR_GLOBAL_NOT_MUTABLE,
            "set const global"
        );
        // The const global keeps its value.
        let mut out = i32_val(0);
        assert_eq!(
            spacewasm_get_global(store, idx, c, &mut out),
            status::SPACEWASM_OK,
            "get c after rejected set"
        );
        assert_eq!(out.u.i32_, 42, "c unchanged");

        // A value whose type does not match the global is rejected (i64 into an
        // i32 global). The mutable global's value is unchanged.
        let i64v = spacewasm_value_t {
            tag: spacewasm_valtype_t::SPACEWASM_I64,
            u: spacewasm_value_payload_t { i64_: 9 },
        };
        assert_eq!(
            spacewasm_set_global(store, idx, g, i64v),
            status::SPACEWASM_ERR_GLOBAL_TYPE_MISMATCH,
            "type mismatch"
        );
        let mut out = i32_val(0);
        assert_eq!(
            spacewasm_get_global(store, idx, g, &mut out),
            status::SPACEWASM_OK,
            "get g after mismatch"
        );
        assert_eq!(out.u.i32_, 5, "g unchanged after mismatch");

        // Check ordering: a value that is BOTH the wrong type AND targets a
        // const global reports the type mismatch first (the type check runs
        // before the mutability check). `c` is const i32, so an i64 is doubly
        // invalid.
        let i64v = spacewasm_value_t {
            tag: spacewasm_valtype_t::SPACEWASM_I64,
            u: spacewasm_value_payload_t { i64_: 1 },
        };
        assert_eq!(
            spacewasm_set_global(store, idx, c, i64v),
            status::SPACEWASM_ERR_GLOBAL_TYPE_MISMATCH,
            "type check precedes mutability check"
        );

        // Out-of-range global and module indices.
        assert_eq!(
            spacewasm_get_global(store, idx, 999, &mut out),
            status::SPACEWASM_ERR_NOT_FOUND,
            "get out-of-range global"
        );
        assert_eq!(
            spacewasm_set_global(store, idx, 999, i32_val(0)),
            status::SPACEWASM_ERR_NOT_FOUND,
            "set out-of-range global"
        );
        assert_eq!(
            spacewasm_get_global(store, 999, g, &mut out),
            status::SPACEWASM_ERR_NOT_FOUND,
            "get out-of-range module"
        );
        assert_eq!(
            spacewasm_set_global(store, 999, g, i32_val(0)),
            status::SPACEWASM_ERR_NOT_FOUND,
            "set out-of-range module"
        );
        assert_eq!(
            spacewasm_find_global(store, 999, c"g".as_ptr(), &mut g),
            status::SPACEWASM_ERR_NOT_FOUND,
            "find in out-of-range module"
        );

        // NULL argument handling across all three entry points.
        assert_eq!(
            spacewasm_find_global(core::ptr::null_mut(), idx, c"g".as_ptr(), &mut g),
            status::SPACEWASM_ERR_NULL_ARG,
            "find null engine"
        );
        assert_eq!(
            spacewasm_find_global(store, idx, core::ptr::null(), &mut g),
            status::SPACEWASM_ERR_NULL_ARG,
            "find null name"
        );
        assert_eq!(
            spacewasm_find_global(store, idx, c"g".as_ptr(), core::ptr::null_mut()),
            status::SPACEWASM_ERR_NULL_ARG,
            "find null out_index"
        );
        assert_eq!(
            spacewasm_get_global(core::ptr::null_mut(), idx, g, &mut out),
            status::SPACEWASM_ERR_NULL_ARG,
            "get null engine"
        );
        assert_eq!(
            spacewasm_get_global(store, idx, g, core::ptr::null_mut()),
            status::SPACEWASM_ERR_NULL_ARG,
            "get null out"
        );
        assert_eq!(
            spacewasm_set_global(core::ptr::null_mut(), idx, g, i32_val(0)),
            status::SPACEWASM_ERR_NULL_ARG,
            "set null engine"
        );
    }

    unsafe {
        spacewasm_destroy(store);
        spacewasm_allocator_destroy(alloc);
    }
}

/// Round-trip each of the four value types through `set_global`/`get_global`,
/// including a negative i64 and float bit patterns (negative zero, NaN), to
/// prove the non-i32 `RawValue` conversions are wired through the API correctly.
#[test]
fn global_get_set_all_types() {
    let _guard = ALLOC_LOCK.lock().unwrap();
    ensure_global_allocator();

    let store = new_store(1024, 1, 256);
    let alloc = new_guest_allocator();

    let idx = load_module_onto(alloc, store, c"main", GLOBALS_MULTI_WASM, 0).expect("load");

    // Resolve the four exported globals.
    let mut gi = u32::MAX;
    let mut gi64 = u32::MAX;
    let mut gf = u32::MAX;
    let mut gd = u32::MAX;
    unsafe {
        assert_eq!(
            spacewasm_find_global(store, idx, c"gi".as_ptr(), &mut gi),
            status::SPACEWASM_OK
        );
        assert_eq!(
            spacewasm_find_global(store, idx, c"gI".as_ptr(), &mut gi64),
            status::SPACEWASM_OK
        );
        assert_eq!(
            spacewasm_find_global(store, idx, c"gf".as_ptr(), &mut gf),
            status::SPACEWASM_OK
        );
        assert_eq!(
            spacewasm_find_global(store, idx, c"gd".as_ptr(), &mut gd),
            status::SPACEWASM_OK
        );
    }

    // Initial values, one per type, confirming the tag and payload read back.
    unsafe {
        let mut out = i32_val(0);
        assert_eq!(
            spacewasm_get_global(store, idx, gi, &mut out),
            status::SPACEWASM_OK
        );
        assert_eq!(out.tag, spacewasm_valtype_t::SPACEWASM_I32);
        assert_eq!(out.u.i32_, 10, "gi init");

        assert_eq!(
            spacewasm_get_global(store, idx, gi64, &mut out),
            status::SPACEWASM_OK
        );
        assert_eq!(out.tag, spacewasm_valtype_t::SPACEWASM_I64);
        assert_eq!(out.u.i64_, 20, "gI init");

        assert_eq!(
            spacewasm_get_global(store, idx, gf, &mut out),
            status::SPACEWASM_OK
        );
        assert_eq!(out.tag, spacewasm_valtype_t::SPACEWASM_F32);
        assert_eq!(out.u.f32_, 1.5, "gf init");

        assert_eq!(
            spacewasm_get_global(store, idx, gd, &mut out),
            status::SPACEWASM_OK
        );
        assert_eq!(out.tag, spacewasm_valtype_t::SPACEWASM_F64);
        assert_eq!(out.u.f64_, 2.5, "gd init");
    }

    // Write a representative (and tricky) value of each type and read it back.
    unsafe {
        assert_eq!(
            spacewasm_set_global(store, idx, gi64, i64_val(-9_000_000_000)),
            status::SPACEWASM_OK
        );
        assert_eq!(
            spacewasm_set_global(store, idx, gf, f32_val(-0.0)),
            status::SPACEWASM_OK
        );
        assert_eq!(
            spacewasm_set_global(store, idx, gd, f64_val(f64::NAN)),
            status::SPACEWASM_OK
        );

        let mut out = i32_val(0);
        assert_eq!(
            spacewasm_get_global(store, idx, gi64, &mut out),
            status::SPACEWASM_OK
        );
        assert_eq!(out.u.i64_, -9_000_000_000, "i64 round-trip");

        assert_eq!(
            spacewasm_get_global(store, idx, gf, &mut out),
            status::SPACEWASM_OK
        );
        // -0.0 == 0.0 by value, so compare the bit pattern to prove the sign
        // bit survived the round trip through RawValue.
        assert_eq!(out.u.f32_.to_bits(), (-0.0f32).to_bits(), "f32 -0.0 bits");

        assert_eq!(
            spacewasm_get_global(store, idx, gd, &mut out),
            status::SPACEWASM_OK
        );
        assert!(out.u.f64_.is_nan(), "f64 NaN round-trip");
    }

    // The i64 write is observable from Wasm through `get_gI`, proving set_global
    // writes the same storage the interpreter reads for a 64-bit global.
    let mut get_gi64 = 0u32;
    assert_eq!(
        unsafe { spacewasm_find_export_func(store, idx, c"get_gI".as_ptr(), &mut get_gi64) },
        status::SPACEWASM_OK,
        "find get_gI"
    );
    assert_eq!(
        unsafe { spacewasm_invoke(store, idx, get_gi64, core::ptr::null(), 0) },
        status::SPACEWASM_OK,
        "invoke get_gI"
    );
    let mut trap = spacewasm_trap_t::SPACEWASM_TRAP_NONE;
    assert_eq!(
        run_to_completion(store, &mut trap),
        spacewasm_run_status_t::SPACEWASM_RUN_FINISHED,
        "run get_gI"
    );
    let mut out = i64_val(0);
    assert_eq!(
        unsafe { spacewasm_get_result(store, spacewasm_valtype_t::SPACEWASM_I64, &mut out) },
        status::SPACEWASM_OK,
        "result get_gI"
    );
    assert_eq!(
        unsafe { out.u.i64_ },
        -9_000_000_000,
        "get_gI observes set_global"
    );

    unsafe {
        spacewasm_destroy(store);
        spacewasm_allocator_destroy(alloc);
    }
}

/// A module whose exported global sits *after* an imported global in the
/// WebAssembly global index space. `find_global` must resolve to the
/// module-local `globals` index (0), not the index-space index (1), and a
/// re-exported *imported* global must report NOT_FOUND. This guards the
/// import-offset resolution in `find_global` that a naive `*out_index = gi.0`
/// would break.
#[test]
fn global_find_skips_imports() {
    let _guard = ALLOC_LOCK.lock().unwrap();
    ensure_global_allocator();

    let store = new_store(1024, 2, 256);
    let alloc = new_guest_allocator();

    // The exporter (`b`) must load first so the importer (`a`) can resolve it.
    let b = load_module_onto(alloc, store, c"b", GLOBAL_EXPORTER_WASM, 0).expect("load b");
    let a = load_module_onto(alloc, store, c"a", GLOBAL_IMPORTER_WASM, 0).expect("load a");
    assert_eq!((b, a), (0, 1), "module indices");

    // `ag` is at index-space slot 1 (after the one imported global) but is the
    // module's own global 0. find_global must return the module-local 0.
    let mut ag = u32::MAX;
    assert_eq!(
        unsafe { spacewasm_find_global(store, a, c"ag".as_ptr(), &mut ag) },
        status::SPACEWASM_OK,
        "find ag"
    );
    assert_eq!(
        ag, 0,
        "ag resolves to the module-local index, not index-space 1"
    );

    // The re-exported import resolves to a global owned by module `b`, so
    // finding it *through module a* misses — mirroring find_export_func.
    let mut sink = u32::MAX;
    assert_eq!(
        unsafe { spacewasm_find_global(store, a, c"reexport".as_ptr(), &mut sink) },
        status::SPACEWASM_ERR_NOT_FOUND,
        "re-exported import is not a's own global"
    );

    // get/set on module a's own global use that module-local index and see its
    // init value (11), independent of module b's global (55).
    unsafe {
        let mut out = i32_val(0);
        assert_eq!(
            spacewasm_get_global(store, a, ag, &mut out),
            status::SPACEWASM_OK
        );
        assert_eq!(out.u.i32_, 11, "ag init");

        assert_eq!(
            spacewasm_set_global(store, a, ag, i32_val(77)),
            status::SPACEWASM_OK
        );
        assert_eq!(
            spacewasm_get_global(store, a, ag, &mut out),
            status::SPACEWASM_OK
        );
        assert_eq!(out.u.i32_, 77, "ag after set");

        // Module b's own global is reachable directly and untouched by the above.
        let mut bg = u32::MAX;
        assert_eq!(
            spacewasm_find_global(store, b, c"bg".as_ptr(), &mut bg),
            status::SPACEWASM_OK,
            "find bg"
        );
        assert_eq!(bg, 0, "bg module-local index");
        assert_eq!(
            spacewasm_get_global(store, b, bg, &mut out),
            status::SPACEWASM_OK
        );
        assert_eq!(out.u.i32_, 55, "bg unchanged");
    }

    unsafe {
        spacewasm_destroy(store);
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

    let mut mod_a = 0u32;
    let mut mod_b = 0u32;
    let mut mod_c = 0u32;
    unsafe {
        assert_eq!(
            spacewasm_find_module(store, c"a".as_ptr(), &mut mod_a),
            status::SPACEWASM_OK,
        );
        assert_eq!(
            spacewasm_find_module(store, c"b".as_ptr(), &mut mod_b),
            status::SPACEWASM_OK,
        );
        assert_eq!(
            spacewasm_find_module(store, c"c".as_ptr(), &mut mod_c),
            status::SPACEWASM_ERR_NOT_FOUND,
        );
    }

    assert_eq!(mod_a, 0);
    assert_eq!(mod_b, 1);

    let mut func_a = 0u32;
    let mut func_b = 0u32;
    unsafe {
        assert_eq!(
            spacewasm_find_export_func(store, 0, c"add".as_ptr(), &mut func_a),
            status::SPACEWASM_OK
        );
        assert_eq!(
            spacewasm_find_export_func(store, 1, c"add".as_ptr(), &mut func_b),
            status::SPACEWASM_OK
        );
    }

    // Invoke module 1 first, then 0, to prove the index selects the target.
    assert_eq!(invoke_add(store, 1, func_b, 100, 1).expect("b"), 101);
    assert_eq!(invoke_add(store, 0, func_a, 20, 22).expect("a"), 42);

    unsafe {
        spacewasm_destroy(store);
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
    let st = unsafe { spacewasm_find_export_func(store, idx, c"add".as_ptr(), &mut func) };
    assert_eq!(st, status::SPACEWASM_OK, "find");

    assert_eq!(invoke_add(store, idx, func, 30, 12).expect("invoke"), 42);

    unsafe {
        spacewasm_destroy(store);
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
        spacewasm_load_module(
            store,
            c"main".as_ptr(),
            Some(failing_read),
            core::ptr::null_mut(),
            alloc,
            &mut idx,
        )
    };
    unsafe { spacewasm_allocator_destroy(alloc) };
    assert_eq!(
        st,
        status::SPACEWASM_ERR_READER_ERROR,
        "expected ERR_READER_ERROR"
    );

    unsafe { spacewasm_destroy(store) };
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

    let mut store: *mut CEngine = core::ptr::null_mut();
    assert_eq!(
        unsafe { spacewasm_new(host.as_mut_ptr(), 1024, 1, opts(256), &mut store) },
        status::SPACEWASM_OK,
        "store_new"
    );

    let alloc = new_guest_allocator();
    let idx = load_module_onto(alloc, store, c"main", HOST_WASM, 0).expect("load host module");

    let mut func = 0u32;
    let st = unsafe { spacewasm_find_export_func(store, idx, c"run".as_ptr(), &mut func) };
    assert_eq!(st, status::SPACEWASM_OK, "find run");

    let params = [i32_val(41)];
    assert_eq!(
        unsafe { spacewasm_invoke(store, idx, func, params.as_ptr(), params.len()) },
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
        unsafe { spacewasm_get_result(store, spacewasm_valtype_t::SPACEWASM_I32, &mut out,) },
        status::SPACEWASM_OK,
        "result"
    );
    assert_eq!(unsafe { out.u.i32_ }, 42, "add_one(41)");

    unsafe {
        spacewasm_destroy(store);
        spacewasm_allocator_destroy(alloc);
    }
}

#[test]
fn void_host_function_pushes_no_value() {
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
        // `sink` takes one i32 and returns nothing (empty result signature).
        assert_eq!(
            spacewasm_add_host_function(
                host.as_mut_ptr(),
                hmod,
                c"sink".as_ptr(),
                c"i".as_ptr(),
                c"".as_ptr(),
                Some(sink),
                core::ptr::null_mut(),
            ),
            status::SPACEWASM_OK,
            "add_host_function"
        );
    }

    let mut store: *mut CEngine = core::ptr::null_mut();
    assert_eq!(
        unsafe { spacewasm_new(host.as_mut_ptr(), 1024, 1, opts(256), &mut store) },
        status::SPACEWASM_OK,
        "store_new"
    );

    let alloc = new_guest_allocator();
    let idx = load_module_onto(alloc, store, c"main", VOID_HOST_WASM, 0).expect("load void module");

    let mut func = 0u32;
    assert_eq!(
        unsafe { spacewasm_find_export_func(store, idx, c"run".as_ptr(), &mut func) },
        status::SPACEWASM_OK,
        "find run"
    );

    // `run(41)` keeps 41 on the stack across the void `sink` call, then adds 1.
    // If the host call had spuriously pushed a value, the trailing `i32.add`
    // would consume it and produce a corrupt (or trapping) result.
    let params = [i32_val(41)];
    assert_eq!(
        unsafe { spacewasm_invoke(store, idx, func, params.as_ptr(), params.len()) },
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
        unsafe { spacewasm_get_result(store, spacewasm_valtype_t::SPACEWASM_I32, &mut out) },
        status::SPACEWASM_OK,
        "result"
    );
    assert_eq!(
        unsafe { out.u.i32_ },
        42,
        "void host call must not push a value"
    );

    unsafe {
        spacewasm_destroy(store);
        spacewasm_allocator_destroy(alloc);
    }
}

#[test]
fn error_paths() {
    let _guard = ALLOC_LOCK.lock().unwrap();
    ensure_global_allocator();

    // max_modules > 256 -> ERR_VEC_TOO_LONG. This check runs *before* the host
    // vector is read, so the caller still owns it and must destroy it — see the
    // "Ownership of `host`" section on `spacewasm_new`. Capture the status first
    // so the vector is released before `host` is reused below.
    let mut host = core::mem::MaybeUninit::<spacewasm_host_t>::uninit();
    assert_eq!(
        unsafe { spacewasm_host_new(0, host.as_mut_ptr()) },
        status::SPACEWASM_OK
    );
    let mut store: *mut CEngine = core::ptr::null_mut();
    let oversized_st =
        unsafe { spacewasm_new(host.as_mut_ptr(), 1024, 257, opts(256), &mut store) };
    if oversized_st != status::SPACEWASM_OK {
        unsafe { spacewasm_host_destroy(host.as_mut_ptr()) };
    }
    assert_eq!(
        oversized_st,
        status::SPACEWASM_ERR_VEC_TOO_LONG,
        "oversized max_modules"
    );

    // Bad signature char -> ERR_BAD_ARG, no panic.
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
        unsafe { spacewasm_new(host.as_mut_ptr(), 1024, 1, opts(256), &mut store) },
        status::SPACEWASM_OK,
        "store_new"
    );
    static JUNK: &[u8] = &[0, 1, 2, 3, 4, 5, 6, 7];
    let alloc = new_guest_allocator();
    assert!(!alloc.is_null(), "allocator_new");
    let st = load_module_onto(alloc, store, c"main", JUNK, 0);
    unsafe { spacewasm_allocator_destroy(alloc) };
    assert_eq!(
        st,
        Err(status::SPACEWASM_ERR_MALFORMED_MAGIC),
        "expected ERR_MALFORMED_MAGIC"
    );

    unsafe { spacewasm_destroy(store) };
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
        spacewasm_load_module(
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
    let st =
        unsafe { spacewasm_find_export_func(core::ptr::null_mut(), 0, c"add".as_ptr(), &mut func) };
    assert_eq!(st, status::SPACEWASM_ERR_NULL_ARG, "null store");

    unsafe { spacewasm_destroy(store) };
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
fn validation_error_codes_map() {
    use spacewasm::{AllocError, ConstantExprError, MemoryError, SectionKind, ValidationError::*};
    let cases = [
        // Basic parsing errors
        (Eof, status::SPACEWASM_ERR_EOF),
        (MalformedInteger, status::SPACEWASM_ERR_MALFORMED_INTEGER),
        (I33IsNegative, status::SPACEWASM_ERR_I33_IS_NEGATIVE),
        (MalformedMagic, status::SPACEWASM_ERR_MALFORMED_MAGIC),
        (MalformedVersion, status::SPACEWASM_ERR_MALFORMED_VERSION),
        (MalformedUtf8, status::SPACEWASM_ERR_MALFORMED_UTF8),
        (
            DuplicateModuleName,
            status::SPACEWASM_ERR_DUPLICATE_MODULE_NAME,
        ),
        (
            DuplicateExportName,
            status::SPACEWASM_ERR_DUPLICATE_EXPORT_NAME,
        ),
        (
            MalformedSectionId(0),
            status::SPACEWASM_ERR_MALFORMED_SECTION_ID,
        ),
        (
            MalformedValueType(0),
            status::SPACEWASM_ERR_MALFORMED_VALUE_TYPE,
        ),
        (
            MalformedFunction(0),
            status::SPACEWASM_ERR_MALFORMED_FUNCTION,
        ),
        (MalformedLimit(0), status::SPACEWASM_ERR_MALFORMED_LIMIT),
        (
            MalformedElemType(0),
            status::SPACEWASM_ERR_MALFORMED_ELEM_TYPE,
        ),
        (
            MalformedSectionSize,
            status::SPACEWASM_ERR_MALFORMED_SECTION_SIZE,
        ),
        (
            ExpectedConstOrVar(0),
            status::SPACEWASM_ERR_EXPECTED_CONST_OR_VAR,
        ),
        (
            MalformedImportExportDesc(0),
            status::SPACEWASM_ERR_MALFORMED_IMPORT_EXPORT_DESC,
        ),
        (
            MalformedMemType(0),
            status::SPACEWASM_ERR_MALFORMED_MEM_TYPE,
        ),
        (InvalidPageSize(0), status::SPACEWASM_ERR_INVALID_PAGE_SIZE),
        (
            InvalidSectionOrdering(SectionKind::Type, SectionKind::Import),
            status::SPACEWASM_ERR_INVALID_SECTION_ORDERING,
        ),
        (
            DuplicateSection(SectionKind::Type),
            status::SPACEWASM_ERR_DUPLICATE_SECTION,
        ),
        (InvalidMaxLimit, status::SPACEWASM_ERR_INVALID_MAX_LIMIT),
        (ExpectedTerminal(0), status::SPACEWASM_ERR_EXPECTED_TERMINAL),
        (InvalidOpcode(0), status::SPACEWASM_ERR_INVALID_OPCODE),
        (MalformedCodeSize, status::SPACEWASM_ERR_MALFORMED_CODE_SIZE),
        (
            InvalidCodeSectionFunctionCount,
            status::SPACEWASM_ERR_INVALID_CODE_SECTION_FUNCTION_COUNT,
        ),
        (VecTooLong, status::SPACEWASM_ERR_VEC_TOO_LONG),
        (IdxTooLarge, status::SPACEWASM_ERR_IDX_TOO_LARGE),
        (
            ModuleIdxTooLarge,
            status::SPACEWASM_ERR_MODULE_IDX_TOO_LARGE,
        ),
        (MemoryTooLarge, status::SPACEWASM_ERR_MEMORY_TOO_LARGE),
        (
            MemoryImportTooLarge,
            status::SPACEWASM_ERR_MEMORY_IMPORT_TOO_LARGE,
        ),
        (MemAlignTooLarge, status::SPACEWASM_ERR_MEM_ALIGN_TOO_LARGE),
        (TableTooLarge, status::SPACEWASM_ERR_TABLE_TOO_LARGE),
        // Control flow validation
        (
            ControlFlowTooDeep,
            status::SPACEWASM_ERR_CONTROL_FLOW_TOO_DEEP,
        ),
        (StackUnderflow, status::SPACEWASM_ERR_STACK_UNDERFLOW),
        (StackTooLarge, status::SPACEWASM_ERR_STACK_TOO_LARGE),
        (
            LabelStackJumpTooDeep,
            status::SPACEWASM_ERR_LABEL_STACK_JUMP_TOO_DEEP,
        ),
        (
            LabelJumpTooLarge,
            status::SPACEWASM_ERR_LABEL_JUMP_TOO_LARGE,
        ),
        (TypeMismatch, status::SPACEWASM_ERR_TYPE_MISMATCH),
        (
            BlockResultTypeMismatch,
            status::SPACEWASM_ERR_BLOCK_RESULT_TYPE_MISMATCH,
        ),
        (
            BrTableResultTypeMismatch,
            status::SPACEWASM_ERR_BR_TABLE_RESULT_TYPE_MISMATCH,
        ),
        (
            FunctionResultTypeMismatch,
            status::SPACEWASM_ERR_FUNCTION_RESULT_TYPE_MISMATCH,
        ),
        // Memory and table validation
        (IllegalMemoryGrow, status::SPACEWASM_ERR_ILLEGAL_MEMORY_GROW),
        (
            InvalidElementOffset,
            status::SPACEWASM_ERR_INVALID_ELEMENT_OFFSET,
        ),
        (
            InvalidElementOutOfBounds,
            status::SPACEWASM_ERR_INVALID_ELEMENT_OUT_OF_BOUNDS,
        ),
        (InvalidTableIndex, status::SPACEWASM_ERR_INVALID_TABLE_INDEX),
        (TableNotDefined, status::SPACEWASM_ERR_TABLE_NOT_DEFINED),
        (
            InvalidElementCount,
            status::SPACEWASM_ERR_INVALID_ELEMENT_COUNT,
        ),
        (InvalidMemIndex, status::SPACEWASM_ERR_INVALID_MEM_INDEX),
        (MemoryNotDefined, status::SPACEWASM_ERR_MEMORY_NOT_DEFINED),
        (
            InvalidMemOffsetType,
            status::SPACEWASM_ERR_INVALID_MEM_OFFSET_TYPE,
        ),
        (
            InvalidNegativeMemOffset,
            status::SPACEWASM_ERR_INVALID_NEGATIVE_MEM_OFFSET,
        ),
        (InvalidMemOffset, status::SPACEWASM_ERR_INVALID_MEM_OFFSET),
        (MultipleMemories, status::SPACEWASM_ERR_MULTIPLE_MEMORIES),
        (MultipleTables, status::SPACEWASM_ERR_MULTIPLE_TABLES),
        // Index validation
        (InvalidLabelIndex, status::SPACEWASM_ERR_INVALID_LABEL_INDEX),
        (InvalidElseBlock, status::SPACEWASM_ERR_INVALID_ELSE_BLOCK),
        (InvalidEndBlock, status::SPACEWASM_ERR_INVALID_END_BLOCK),
        (
            InstructionOutsideOfFunction,
            status::SPACEWASM_ERR_INSTRUCTION_OUTSIDE_OF_FUNCTION,
        ),
        (
            LocalIdxOutOfRange,
            status::SPACEWASM_ERR_LOCAL_IDX_OUT_OF_RANGE,
        ),
        (
            FunctionIdxOutOfRange,
            status::SPACEWASM_ERR_FUNCTION_IDX_OUT_OF_RANGE,
        ),
        (
            TypeIdxOutOfRange,
            status::SPACEWASM_ERR_TYPE_IDX_OUT_OF_RANGE,
        ),
        (
            FunctionTextOutOfRange,
            status::SPACEWASM_ERR_FUNCTION_TEXT_OUT_OF_RANGE,
        ),
        (
            GlobalIdxOutOfRange,
            status::SPACEWASM_ERR_GLOBAL_IDX_OUT_OF_RANGE,
        ),
        // Import validation
        (
            FunctionImportNotFound,
            status::SPACEWASM_ERR_FUNCTION_IMPORT_NOT_FOUND,
        ),
        (
            GlobalImportNotFound,
            status::SPACEWASM_ERR_GLOBAL_IMPORT_NOT_FOUND,
        ),
        (
            MemoryImportNotFound,
            status::SPACEWASM_ERR_MEMORY_IMPORT_NOT_FOUND,
        ),
        (
            TableImportNotFound,
            status::SPACEWASM_ERR_TABLE_IMPORT_NOT_FOUND,
        ),
        (
            FunctionImportOutOfRange,
            status::SPACEWASM_ERR_FUNCTION_IMPORT_OUT_OF_RANGE,
        ),
        (
            FunctionImportTypeMismatch,
            status::SPACEWASM_ERR_FUNCTION_IMPORT_TYPE_MISMATCH,
        ),
        (GlobalNotMutable, status::SPACEWASM_ERR_GLOBAL_NOT_MUTABLE),
        (
            GlobalImportTypeMismatch,
            status::SPACEWASM_ERR_GLOBAL_IMPORT_TYPE_MISMATCH,
        ),
        (
            MemoryImportTypeMismatch,
            status::SPACEWASM_ERR_MEMORY_IMPORT_TYPE_MISMATCH,
        ),
        (
            TableImportTypeMismatch,
            status::SPACEWASM_ERR_TABLE_IMPORT_TYPE_MISMATCH,
        ),
        (
            TableImportIncompatibleSize,
            status::SPACEWASM_ERR_TABLE_IMPORT_INCOMPATIBLE_SIZE,
        ),
        // Function and global validation
        (
            FunctionParametersTooLarge,
            status::SPACEWASM_ERR_FUNCTION_PARAMETERS_TOO_LARGE,
        ),
        (
            FunctionReturnsTooLarge,
            status::SPACEWASM_ERR_FUNCTION_RETURNS_TOO_LARGE,
        ),
        (TooManyLocals, status::SPACEWASM_ERR_TOO_MANY_LOCALS),
        (
            InvalidConstInstruction,
            status::SPACEWASM_ERR_INVALID_CONST_INSTRUCTION,
        ),
        (
            GlobalTypeMismatch,
            status::SPACEWASM_ERR_GLOBAL_TYPE_MISMATCH,
        ),
        (
            AlignmentLargerThanType,
            status::SPACEWASM_ERR_ALIGNMENT_LARGER_THAN_TYPE,
        ),
        (
            InvalidStartFunctionSignature,
            status::SPACEWASM_ERR_INVALID_START_FUNCTION_SIGNATURE,
        ),
        (
            InvalidHostStartFunction,
            status::SPACEWASM_ERR_INVALID_HOST_START_FUNCTION,
        ),
        // Constant expression validation
        (
            InvalidConstantExpr(ConstantExprError::InvalidConstantInstruction),
            status::SPACEWASM_ERR_INVALID_CONST_INSTRUCTION,
        ),
        (
            InvalidConstantExpr(ConstantExprError::AlreadyHasValue),
            status::SPACEWASM_ERR_CONST_ALREADY_HAS_VALUE,
        ),
        (
            InvalidConstantExpr(ConstantExprError::NoValue),
            status::SPACEWASM_ERR_CONST_NO_VALUE,
        ),
        (
            InvalidConstantExpr(ConstantExprError::InvalidGlobal),
            status::SPACEWASM_ERR_CONST_INVALID_GLOBAL,
        ),
        (
            GuestMemoryAllocationFailure,
            status::SPACEWASM_ERR_GUEST_MEMORY_ALLOC_FAILED,
        ),
        // Nested error types
        (
            AllocError(AllocError::AllocationFailed),
            status::SPACEWASM_ERR_ALLOC_FAILED,
        ),
        (
            AllocError(AllocError::OutOfMemory),
            status::SPACEWASM_ERR_OUT_OF_MEMORY,
        ),
        (
            AllocError(AllocError::PageTooSmall),
            status::SPACEWASM_ERR_PAGE_TOO_SMALL,
        ),
        (
            MemoryError(MemoryError::OutOfBounds),
            status::SPACEWASM_ERR_MEM_OUT_OF_BOUNDS,
        ),
        (
            MemoryError(MemoryError::OutOfMemory),
            status::SPACEWASM_ERR_OUT_OF_MEMORY,
        ),
        (
            MemoryError(MemoryError::AllocationFailed),
            status::SPACEWASM_ERR_ALLOC_FAILED,
        ),
        (
            MemoryError(MemoryError::PageTooSmall),
            status::SPACEWASM_ERR_PAGE_TOO_SMALL,
        ),
        // Miscellaneous
        (
            PossibleBackpatchCycle,
            status::SPACEWASM_ERR_POSSIBLE_BACKPATCH_CYCLE,
        ),
        (PageFault, status::SPACEWASM_ERR_PAGE_FAULT),
        (ReaderError(0), status::SPACEWASM_ERR_READER_ERROR),
    ];
    for (err, code) in cases {
        assert_eq!(status::validation_status(&err), code, "{err:?}");
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
    use spacewasm::{HostFunctionError, HostNameError, SectionDecodeError, ValidationError};

    let pe = spacewasm::ParseError::new(0, SectionDecodeError::new(ValidationError::Eof));
    assert_eq!(status::parse_status(&pe), status::SPACEWASM_ERR_EOF);

    assert_eq!(
        status::host_name_status(HostNameError),
        status::SPACEWASM_ERR_NAME_TOO_LONG
    );

    // Every HostFunctionError variant maps to a distinct, stable status code.
    assert_eq!(
        status::host_val_list_status(HostFunctionError::ValListInvalidItem),
        status::SPACEWASM_ERR_BAD_SIGNATURE,
        "invalid value-list character"
    );
    assert_eq!(
        status::host_val_list_status(HostFunctionError::ParameterListTooLong),
        status::SPACEWASM_ERR_FUNCTION_PARAMETERS_TOO_LARGE,
        "too many parameters"
    );
    assert_eq!(
        status::host_val_list_status(HostFunctionError::MultiReturnNotAllowed),
        status::SPACEWASM_ERR_FUNCTION_RETURNS_TOO_LARGE,
        "multiple return values"
    );
    // The AllocError variant forwards to the shared allocator mapping.
    for ae in [
        spacewasm::AllocError::AllocationFailed,
        spacewasm::AllocError::OutOfMemory,
        spacewasm::AllocError::PageTooSmall,
    ] {
        assert_eq!(
            status::host_val_list_status(HostFunctionError::AllocError(ae.clone())),
            status::alloc_status(ae),
            "alloc error forwards to alloc_status"
        );
    }
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
        assert_eq!(c.try_to_value().unwrap(), v, "round trip {v:?}");
    }
}

#[test]
fn value_from_raw_reinterprets_by_type() {
    use spacewasm::{RawValue, ValType, Value};

    assert_eq!(
        spacewasm_value_t::from_raw(RawValue::from_i32(-1), ValType::I32)
            .try_to_value()
            .unwrap(),
        Value::I32(-1)
    );
    assert_eq!(
        spacewasm_value_t::from_raw(RawValue::from_i64(9), ValType::I64)
            .try_to_value()
            .unwrap(),
        Value::I64(9)
    );
    assert_eq!(
        spacewasm_value_t::from_raw(RawValue::from_f32(1.5), ValType::F32)
            .try_to_value()
            .unwrap(),
        Value::F32(1.5)
    );
    assert_eq!(
        spacewasm_value_t::from_raw(RawValue::from_f64(6.5), ValType::F64)
            .try_to_value()
            .unwrap(),
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
        assert_eq!(ValType::try_from(&c).unwrap(), vt);
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
    let st = unsafe { spacewasm_find_export_func(store, idx, c"boom".as_ptr(), &mut func) };
    assert_eq!(st, status::SPACEWASM_OK, "find");

    assert_eq!(
        unsafe { spacewasm_invoke(store, idx, func, core::ptr::null(), 0) },
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
        spacewasm_destroy(store);
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
    // `spacewasm_module_start` and drive the start invocation explicitly.
    let mut cursor = Cursor {
        data: START_WASM,
        pos: 0,
        step: 0,
    };
    let mut idx = 0u32;
    assert_eq!(
        unsafe {
            spacewasm_load_module(
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

    // Resolve the start function location, then invoke it like any other
    // exported function.
    let mut start_mod = 0u32;
    let mut start_func = 0u32;
    assert_eq!(
        unsafe { spacewasm_module_start(store, idx, &mut start_mod, &mut start_func) },
        status::SPACEWASM_OK
    );
    assert_eq!(
        unsafe { spacewasm_invoke(store, start_mod, start_func, core::ptr::null(), 0) },
        status::SPACEWASM_OK
    );

    // Drive the start function to completion.
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
        unsafe { spacewasm_find_export_func(store, idx, c"get".as_ptr(), &mut func) },
        status::SPACEWASM_OK
    );
    assert_eq!(
        unsafe { spacewasm_invoke(store, idx, func, core::ptr::null(), 0) },
        status::SPACEWASM_OK
    );
    assert_eq!(
        run_to_completion(store, &mut trap),
        spacewasm_run_status_t::SPACEWASM_RUN_FINISHED
    );
    let mut out = i32_val(0);
    assert_eq!(
        unsafe { spacewasm_get_result(store, spacewasm_valtype_t::SPACEWASM_I32, &mut out) },
        status::SPACEWASM_OK
    );
    assert_eq!(unsafe { out.u.i32_ }, 42, "start wrote 42");

    unsafe {
        spacewasm_destroy(store);
        spacewasm_allocator_destroy(alloc);
    }
}

#[test]
fn no_start_module_reports_none() {
    let _guard = ALLOC_LOCK.lock().unwrap();
    ensure_global_allocator();

    let store = new_store(1024, 1, 256);
    let alloc = new_guest_allocator();
    let idx = load_module_onto(alloc, store, c"main", ADD_WASM, 0).expect("load");

    // A module with no start function has no start location to resolve.
    let mut start_mod = 0u32;
    let mut start_func = 0u32;
    assert_eq!(
        unsafe { spacewasm_module_start(store, idx, &mut start_mod, &mut start_func) },
        status::SPACEWASM_ERR_NOT_FOUND
    );

    unsafe {
        spacewasm_destroy(store);
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
        unsafe { spacewasm_find_export_func(store, idx, c"spin".as_ptr(), &mut func) },
        status::SPACEWASM_OK
    );

    // Spin 5000 iterations; a small per-call fuel budget forces the run to
    // slice, so we observe OUT_OF_FUEL at least once before it finishes.
    let params = [i32_val(5000)];
    assert_eq!(
        unsafe { spacewasm_invoke(store, idx, func, params.as_ptr(), params.len()) },
        status::SPACEWASM_OK
    );

    let mut saw_out_of_fuel = false;
    let mut trap = spacewasm_trap_t::SPACEWASM_TRAP_NONE;
    let final_status = loop {
        let rs = unsafe { spacewasm_run(store, 64, &mut trap) };
        match rs {
            spacewasm_run_status_t::SPACEWASM_RUN_OUT_OF_FUEL => saw_out_of_fuel = true,
            other => break other,
        }
    };
    assert!(saw_out_of_fuel, "expected at least one out-of-fuel slice");
    assert_eq!(final_status, spacewasm_run_status_t::SPACEWASM_RUN_FINISHED);

    let mut out = i32_val(0);
    assert_eq!(
        unsafe { spacewasm_get_result(store, spacewasm_valtype_t::SPACEWASM_I32, &mut out) },
        status::SPACEWASM_OK
    );
    assert_eq!(unsafe { out.u.i32_ }, 5000, "spin(5000)");

    unsafe {
        spacewasm_destroy(store);
        spacewasm_allocator_destroy(alloc);
    }
}

#[test]
fn reset_abandons_in_progress_call() {
    let _guard = ALLOC_LOCK.lock().unwrap();
    ensure_global_allocator();

    let store = new_store(1024, 1, 256);
    let alloc = new_guest_allocator();
    let idx = load_module_onto(alloc, store, c"main", LOOP_WASM, 0).expect("load");

    let mut func = 0u32;
    assert_eq!(
        unsafe { spacewasm_find_export_func(store, idx, c"spin".as_ptr(), &mut func) },
        status::SPACEWASM_OK
    );

    // Start a long-running call and run a single small slice so it pauses
    // mid-execution (out of fuel) without finishing.
    let params = [i32_val(5000)];
    assert_eq!(
        unsafe { spacewasm_invoke(store, idx, func, params.as_ptr(), params.len()) },
        status::SPACEWASM_OK
    );
    let mut trap = spacewasm_trap_t::SPACEWASM_TRAP_NONE;
    assert_eq!(
        unsafe { spacewasm_run(store, 64, &mut trap) },
        spacewasm_run_status_t::SPACEWASM_RUN_OUT_OF_FUEL
    );

    // While the call is in progress the engine is not idle, so a fresh invoke
    // is rejected.
    assert_eq!(
        unsafe { spacewasm_invoke(store, idx, func, params.as_ptr(), params.len()) },
        status::SPACEWASM_ERR_WRONG_STATE,
        "invoke while running"
    );

    // Reset returns the engine to idle, discarding the in-progress call.
    assert_eq!(unsafe { spacewasm_reset(store) }, status::SPACEWASM_OK);

    // A fresh invocation now succeeds and runs to completion from a clean slate.
    let params = [i32_val(10)];
    assert_eq!(
        unsafe { spacewasm_invoke(store, idx, func, params.as_ptr(), params.len()) },
        status::SPACEWASM_OK,
        "invoke after reset"
    );
    assert_eq!(
        run_to_completion(store, &mut trap),
        spacewasm_run_status_t::SPACEWASM_RUN_FINISHED
    );
    let mut out = i32_val(0);
    assert_eq!(
        unsafe { spacewasm_get_result(store, spacewasm_valtype_t::SPACEWASM_I32, &mut out) },
        status::SPACEWASM_OK
    );
    assert_eq!(unsafe { out.u.i32_ }, 10, "spin(10) after reset");

    // Reset on a null engine is rejected.
    assert_eq!(
        unsafe { spacewasm_reset(core::ptr::null_mut()) },
        status::SPACEWASM_ERR_NULL_ARG,
        "null engine"
    );

    unsafe {
        spacewasm_destroy(store);
        spacewasm_allocator_destroy(alloc);
    }
}

/// Resuming an engine that is not paused (nothing is awaiting a host result) is
/// a state error, and a null engine is rejected up front, for both resume
/// entry points.
#[test]
fn resume_without_pause_is_wrong_state() {
    let _guard = ALLOC_LOCK.lock().unwrap();
    ensure_global_allocator();

    let store = new_store(1024, 1, 256);
    let alloc = new_guest_allocator();
    let _idx = load_module_onto(alloc, store, c"main", ADD_WASM, 0).expect("load");

    // The engine is idle (nothing paused), so both resume paths reject with
    // WRONG_STATE rather than corrupting the operand stack.
    assert_eq!(
        unsafe { spacewasm_resume(store) },
        status::SPACEWASM_ERR_WRONG_STATE,
        "resume while not paused"
    );
    assert_eq!(
        unsafe { spacewasm_resume_value(store, i32_val(0)) },
        status::SPACEWASM_ERR_WRONG_STATE,
        "resume_value while not paused"
    );

    // A null engine is rejected before any state inspection.
    assert_eq!(
        unsafe { spacewasm_resume(core::ptr::null_mut()) },
        status::SPACEWASM_ERR_NULL_ARG,
        "resume null engine"
    );
    assert_eq!(
        unsafe { spacewasm_resume_value(core::ptr::null_mut(), i32_val(0)) },
        status::SPACEWASM_ERR_NULL_ARG,
        "resume_value null engine"
    );

    unsafe {
        spacewasm_destroy(store);
        spacewasm_allocator_destroy(alloc);
    }
}

/// Exercise the `out_trap` output path of `spacewasm_run`: it is cleared up
/// front even on an early return, overwritten with the real trap reason on a
/// trap, and a null `out_trap` is accepted.
#[test]
fn run_out_trap_output_path() {
    let _guard = ALLOC_LOCK.lock().unwrap();
    ensure_global_allocator();

    let store = new_store(1024, 1, 256);
    let alloc = new_guest_allocator();
    let idx = load_module_onto(alloc, store, c"main", TRAP_WASM, 0).expect("load");
    let mut func = 0u32;
    assert_eq!(
        unsafe { spacewasm_find_export_func(store, idx, c"boom".as_ptr(), &mut func) },
        status::SPACEWASM_OK,
        "find boom"
    );

    // `run` clears `out_trap` before doing anything, even on the early return
    // taken when idle: a stale seed must be reset to NONE.
    let mut trap = spacewasm_trap_t::SPACEWASM_TRAP_UNREACHABLE; // stale seed
    assert_eq!(
        unsafe { spacewasm_run(store, 0, &mut trap) },
        spacewasm_run_status_t::SPACEWASM_RUN_TRAP,
        "run while idle traps"
    );
    assert_eq!(
        trap,
        spacewasm_trap_t::SPACEWASM_TRAP_NONE,
        "idle run clears out_trap to NONE"
    );

    // A real trap overwrites the seeded value with the actual trap reason.
    assert_eq!(
        unsafe { spacewasm_invoke(store, idx, func, core::ptr::null(), 0) },
        status::SPACEWASM_OK,
        "invoke boom"
    );
    trap = spacewasm_trap_t::SPACEWASM_TRAP_HOST; // stale seed
    assert_eq!(
        run_to_completion(store, &mut trap),
        spacewasm_run_status_t::SPACEWASM_RUN_TRAP,
        "boom traps"
    );
    assert_eq!(
        trap,
        spacewasm_trap_t::SPACEWASM_TRAP_UNREACHABLE,
        "out_trap carries the real trap reason"
    );

    // A null `out_trap` is accepted: after a reset the same call traps again
    // with nowhere to report the reason, and must not crash.
    assert_eq!(
        unsafe { spacewasm_reset(store) },
        status::SPACEWASM_OK,
        "reset"
    );
    assert_eq!(
        unsafe { spacewasm_invoke(store, idx, func, core::ptr::null(), 0) },
        status::SPACEWASM_OK,
        "re-invoke boom"
    );
    let mut rs = spacewasm_run_status_t::SPACEWASM_RUN_OUT_OF_FUEL;
    while rs == spacewasm_run_status_t::SPACEWASM_RUN_OUT_OF_FUEL {
        rs = unsafe { spacewasm_run(store, 10000, core::ptr::null_mut()) };
    }
    assert_eq!(
        rs,
        spacewasm_run_status_t::SPACEWASM_RUN_TRAP,
        "null out_trap still reports TRAP"
    );

    unsafe {
        spacewasm_destroy(store);
        spacewasm_allocator_destroy(alloc);
    }
}

/// Host implementation of `env.sink`: a void host function (no result type). It
/// asserts it received exactly one argument and returns `SPACEWASM_CONTINUE_NONE`
/// *while still writing to `out`*, to prove the interpreter honours the "no
/// value" outcome and does not push the stale `out` onto the Wasm stack.
unsafe extern "C" fn sink(
    _caller: *mut SpacewasmCaller,
    _userdata: *mut c_void,
    _params: *const spacewasm_value_t,
    n: usize,
    out: *mut spacewasm_value_t,
) -> spacewasm_hostcall_result_t {
    if n != 1 {
        return spacewasm_hostcall_result_t::SPACEWASM_TRAP;
    }
    // Deliberately scribble a bogus result; CONTINUE_NONE must ignore it.
    unsafe { *out = i32_val(0x7fff_ffff) };
    spacewasm_hostcall_result_t::SPACEWASM_CONTINUE_NONE
}

/// Host callback that pauses execution without returning a value.
unsafe extern "C" fn pause_host(
    _caller: *mut SpacewasmCaller,
    _userdata: *mut c_void,
    _params: *const spacewasm_value_t,
    _n: usize,
    _out: *mut spacewasm_value_t,
) -> spacewasm_hostcall_result_t {
    spacewasm_hostcall_result_t::SPACEWASM_PAUSE
}

/// Host callback that pauses execution and expects to return an i32.
unsafe extern "C" fn pause_i32_host(
    _caller: *mut SpacewasmCaller,
    _userdata: *mut c_void,
    _params: *const spacewasm_value_t,
    _n: usize,
    _out: *mut spacewasm_value_t,
) -> spacewasm_hostcall_result_t {
    spacewasm_hostcall_result_t::SPACEWASM_PAUSE
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
    spacewasm_hostcall_result_t::SPACEWASM_CONTINUE_SOME
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

    let mut store: *mut CEngine = core::ptr::null_mut();
    assert_eq!(
        unsafe { spacewasm_new(host.as_mut_ptr(), 1024, 1, opts(256), &mut store) },
        status::SPACEWASM_OK
    );

    let alloc = new_guest_allocator();
    let idx = load_module_onto(alloc, store, c"main", HOST_WASM, 0).expect("load");

    let mut func = 0u32;
    assert_eq!(
        unsafe { spacewasm_find_export_func(store, idx, c"run".as_ptr(), &mut func) },
        status::SPACEWASM_OK
    );
    let params = [i32_val(41)];
    assert_eq!(
        unsafe { spacewasm_invoke(store, idx, func, params.as_ptr(), params.len()) },
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
        unsafe { spacewasm_get_result(store, spacewasm_valtype_t::SPACEWASM_I32, &mut out) },
        status::SPACEWASM_OK
    );
    assert_eq!(unsafe { out.u.i32_ }, 42);

    unsafe {
        spacewasm_destroy(store);
        spacewasm_allocator_destroy(alloc);
    }
}

#[test]
fn store_with_null_host() {
    let _guard = ALLOC_LOCK.lock().unwrap();
    ensure_global_allocator();

    // A NULL host makes a store with no host modules.
    let mut store: *mut CEngine = core::ptr::null_mut();
    assert_eq!(
        unsafe { spacewasm_new(core::ptr::null_mut(), 1024, 1, opts(256), &mut store) },
        status::SPACEWASM_OK
    );
    assert!(!store.is_null());

    let alloc = new_guest_allocator();
    let idx = load_module_onto(alloc, store, c"main", ADD_WASM, 0).expect("load");
    let mut func = 0u32;
    assert_eq!(
        unsafe { spacewasm_find_export_func(store, idx, c"add".as_ptr(), &mut func) },
        status::SPACEWASM_OK
    );
    assert_eq!(invoke_add(store, idx, func, 2, 3).expect("invoke"), 5);

    unsafe {
        spacewasm_destroy(store);
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
        unsafe { spacewasm_find_export_func(store, idx, c"add".as_ptr(), &mut func) },
        status::SPACEWASM_OK
    );

    // No invocation yet: get_result has nothing to return.
    let mut out = i32_val(0);
    assert_eq!(
        unsafe { spacewasm_get_result(store, spacewasm_valtype_t::SPACEWASM_I32, &mut out) },
        status::SPACEWASM_ERR_NOT_FOUND,
        "no result available"
    );

    // Running while idle (nothing invoked) reports a trap without panicking.
    let mut trap = spacewasm_trap_t::SPACEWASM_TRAP_NONE;
    assert_eq!(
        unsafe { spacewasm_run(store, 0, &mut trap) },
        spacewasm_run_status_t::SPACEWASM_RUN_TRAP,
        "run while idle"
    );

    // func_index that does not fit in a u16 is rejected as a bad argument.
    let params = [i32_val(1), i32_val(2)];
    assert_eq!(
        unsafe { spacewasm_invoke(store, idx, 0x1_0000, params.as_ptr(), params.len()) },
        status::SPACEWASM_ERR_BAD_ARG,
        "func_index overflow"
    );

    // A missing export is not found.
    let mut nope = 0u32;
    assert_eq!(
        unsafe { spacewasm_find_export_func(store, idx, c"missing".as_ptr(), &mut nope) },
        status::SPACEWASM_ERR_NOT_FOUND
    );

    // Invoking, then invoking again before running, is a state error.
    assert_eq!(
        unsafe { spacewasm_invoke(store, idx, func, params.as_ptr(), params.len()) },
        status::SPACEWASM_OK
    );
    assert_eq!(
        unsafe { spacewasm_invoke(store, idx, func, params.as_ptr(), params.len()) },
        status::SPACEWASM_ERR_WRONG_STATE,
        "double invoke"
    );

    unsafe {
        spacewasm_destroy(store);
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
        unsafe { spacewasm_new(host.as_mut_ptr(), 1024, 1, opts(256), core::ptr::null_mut()) },
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
fn add_host_function_signature_errors() {
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
            spacewasm_add_host_module(host.as_mut_ptr(), c"env".as_ptr(), 4, 0, &mut hmod),
            status::SPACEWASM_OK
        );

        // Helper to register a function with the given signatures, mapping each
        // distinct HostFunctionError variant to its FFI status code.
        let mut add =
            |name: &core::ffi::CStr, params: &core::ffi::CStr, returns: &core::ffi::CStr| {
                spacewasm_add_host_function(
                    host.as_mut_ptr(),
                    hmod,
                    name.as_ptr(),
                    params.as_ptr(),
                    returns.as_ptr(),
                    Some(add_one),
                    core::ptr::null_mut(),
                )
            };

        // Invalid value-list character in the parameter signature.
        assert_eq!(
            add(c"bad_param", c"x", c""),
            status::SPACEWASM_ERR_BAD_SIGNATURE,
            "invalid param char -> ValListInvalidItem"
        );

        // Invalid value-list character in the return signature.
        assert_eq!(
            add(c"bad_ret", c"i", c"z"),
            status::SPACEWASM_ERR_BAD_SIGNATURE,
            "invalid return char -> ValListInvalidItem"
        );

        // More than MAX_HOST_FUNCTION_PARAMS (9) parameters.
        assert_eq!(
            add(c"too_many", c"iiiiiiiiii", c""),
            status::SPACEWASM_ERR_FUNCTION_PARAMETERS_TOO_LARGE,
            "10 params -> ParameterListTooLong"
        );

        // More than one return value is not supported.
        assert_eq!(
            add(c"multi_ret", c"i", c"ii"),
            status::SPACEWASM_ERR_FUNCTION_RETURNS_TOO_LARGE,
            "two returns -> MultiReturnNotAllowed"
        );

        // A valid signature still succeeds after the rejected attempts.
        assert_eq!(
            add(c"ok", c"i", c"i"),
            status::SPACEWASM_OK,
            "valid signature registers"
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
    // A null callback is rejected with the "null callback" code, leaving any
    // previously installed allocator in place.
    assert_eq!(
        crate::spacewasm_set_global_allocator(None, Some(global_dealloc), core::ptr::null_mut()),
        crate::spacewasm_status_t::SPACEWASM_ERR_BAD_ARG
    );
    assert_eq!(
        crate::spacewasm_set_global_allocator(Some(global_alloc), None, core::ptr::null_mut()),
        crate::spacewasm_status_t::SPACEWASM_ERR_BAD_ARG
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
            spacewasm_load_module(
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

    let mut start_mod = 0u32;
    let mut start_func = 0u32;
    assert_eq!(
        unsafe { spacewasm_module_start(store, idx, &mut start_mod, &mut start_func) },
        status::SPACEWASM_OK
    );
    assert_eq!(
        unsafe { spacewasm_invoke(store, start_mod, start_func, core::ptr::null(), 0) },
        status::SPACEWASM_OK
    );

    let mut trap = spacewasm_trap_t::SPACEWASM_TRAP_NONE;
    let status = run_to_completion(store, &mut trap);
    assert_eq!(status, spacewasm_run_status_t::SPACEWASM_RUN_TRAP);
    assert_eq!(trap, spacewasm_trap_t::SPACEWASM_TRAP_UNREACHABLE);

    unsafe {
        spacewasm_destroy(store);
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

    // spacewasm_module_start on an out-of-range module is NOT_FOUND.
    let mut start_mod = 0u32;
    let mut start_func = 0u32;
    assert_eq!(
        unsafe { spacewasm_module_start(store, 99, &mut start_mod, &mut start_func) },
        status::SPACEWASM_ERR_NOT_FOUND
    );

    // run on an out-of-range module traps (no such module to seed).
    let mut trap = spacewasm_trap_t::SPACEWASM_TRAP_NONE;
    assert_eq!(
        unsafe { spacewasm_run(store, 0, &mut trap) },
        spacewasm_run_status_t::SPACEWASM_RUN_TRAP
    );

    // invoke on an out-of-range module is NOT_FOUND.
    let params = [i32_val(1), i32_val(2)];
    assert_eq!(
        unsafe { spacewasm_invoke(store, 99, 0, params.as_ptr(), params.len()) },
        status::SPACEWASM_ERR_NOT_FOUND
    );

    unsafe {
        spacewasm_destroy(store);
        spacewasm_allocator_destroy(alloc);
    }
}

/// `(module (import "env" "pause") (func (export "test_pause") (result i32)
///    (call 0) (i32.const 42)))` — calls pause, then returns 42 after resume.
#[rustfmt::skip]
static PAUSE_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x60,
    0x00, 0x00, 0x60, 0x00, 0x01, 0x7f, 0x02, 0x0d, 0x01, 0x03, 0x65, 0x6e,
    0x76, 0x05, 0x70, 0x61, 0x75, 0x73, 0x65, 0x00, 0x00, 0x03, 0x02, 0x01,
    0x01, 0x07, 0x0e, 0x01, 0x0a, 0x74, 0x65, 0x73, 0x74, 0x5f, 0x70, 0x61,
    0x75, 0x73, 0x65, 0x00, 0x01, 0x0a, 0x08, 0x01, 0x06, 0x00, 0x10, 0x00,
    0x41, 0x2a, 0x0b,
];

/// `(module (import "env" "pause_i32") (func (export "test_pause_i32") (result i32)
///    (call 0)))` — calls pause_i32, returns whatever value is resumed with.
#[rustfmt::skip]
static PAUSE_I32_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x60,
    0x00, 0x01, 0x7f, 0x02, 0x11, 0x01, 0x03, 0x65, 0x6e, 0x76, 0x09, 0x70,
    0x61, 0x75, 0x73, 0x65, 0x5f, 0x69, 0x33, 0x32, 0x00, 0x00, 0x03, 0x02,
    0x01, 0x00, 0x07, 0x12, 0x01, 0x0e, 0x74, 0x65, 0x73, 0x74, 0x5f, 0x70,
    0x61, 0x75, 0x73, 0x65, 0x5f, 0x69, 0x33, 0x32, 0x00, 0x01, 0x0a, 0x06,
    0x01, 0x04, 0x00, 0x10, 0x00, 0x0b,
];

#[test]
fn pause_and_resume_no_value() {
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
                c"pause".as_ptr(),
                c"".as_ptr(),
                c"".as_ptr(),
                Some(pause_host),
                core::ptr::null_mut(),
            ),
            status::SPACEWASM_OK
        );
    }

    let mut store: *mut CEngine = core::ptr::null_mut();
    assert_eq!(
        unsafe { spacewasm_new(host.as_mut_ptr(), 1024, 1, opts(256), &mut store) },
        status::SPACEWASM_OK
    );

    let alloc = new_guest_allocator();
    let idx = load_module_onto(alloc, store, c"main", PAUSE_WASM, 0).expect("load");

    let mut func = 0u32;
    assert_eq!(
        unsafe { spacewasm_find_export_func(store, idx, c"test_pause".as_ptr(), &mut func) },
        status::SPACEWASM_OK
    );

    // Invoke the function
    assert_eq!(
        unsafe { spacewasm_invoke(store, idx, func, core::ptr::null(), 0) },
        status::SPACEWASM_OK
    );

    // Run until pause
    let mut trap = spacewasm_trap_t::SPACEWASM_TRAP_NONE;
    assert_eq!(
        unsafe { spacewasm_run(store, 10000, &mut trap) },
        spacewasm_run_status_t::SPACEWASM_RUN_PAUSE,
        "should pause"
    );

    // Resume without value
    assert_eq!(unsafe { spacewasm_resume(store) }, status::SPACEWASM_OK);

    // Continue running to completion
    assert_eq!(
        run_to_completion(store, &mut trap),
        spacewasm_run_status_t::SPACEWASM_RUN_FINISHED
    );

    // Check result
    let mut out = i32_val(0);
    assert_eq!(
        unsafe { spacewasm_get_result(store, spacewasm_valtype_t::SPACEWASM_I32, &mut out) },
        status::SPACEWASM_OK
    );
    assert_eq!(unsafe { out.u.i32_ }, 42);

    unsafe {
        spacewasm_destroy(store);
        spacewasm_allocator_destroy(alloc);
    }
}

#[test]
fn pause_and_resume_with_value() {
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
                c"pause_i32".as_ptr(),
                c"".as_ptr(),
                c"i".as_ptr(),
                Some(pause_i32_host),
                core::ptr::null_mut(),
            ),
            status::SPACEWASM_OK
        );
    }

    let mut store: *mut CEngine = core::ptr::null_mut();
    assert_eq!(
        unsafe { spacewasm_new(host.as_mut_ptr(), 1024, 1, opts(256), &mut store) },
        status::SPACEWASM_OK
    );

    let alloc = new_guest_allocator();
    let idx = load_module_onto(alloc, store, c"main", PAUSE_I32_WASM, 0).expect("load");

    let mut func = 0u32;
    assert_eq!(
        unsafe { spacewasm_find_export_func(store, idx, c"test_pause_i32".as_ptr(), &mut func) },
        status::SPACEWASM_OK
    );

    // Invoke the function
    assert_eq!(
        unsafe { spacewasm_invoke(store, idx, func, core::ptr::null(), 0) },
        status::SPACEWASM_OK
    );

    // Run until pause
    let mut trap = spacewasm_trap_t::SPACEWASM_TRAP_NONE;
    assert_eq!(
        unsafe { spacewasm_run(store, 10000, &mut trap) },
        spacewasm_run_status_t::SPACEWASM_RUN_PAUSE,
        "should pause"
    );

    // Resume with value 99
    assert_eq!(
        unsafe { spacewasm_resume_value(store, i32_val(99)) },
        status::SPACEWASM_OK
    );

    // Continue running to completion
    assert_eq!(
        run_to_completion(store, &mut trap),
        spacewasm_run_status_t::SPACEWASM_RUN_FINISHED
    );

    // Check result - should be the resumed value (99)
    let mut out = i32_val(0);
    assert_eq!(
        unsafe { spacewasm_get_result(store, spacewasm_valtype_t::SPACEWASM_I32, &mut out) },
        status::SPACEWASM_OK
    );
    assert_eq!(unsafe { out.u.i32_ }, 99);

    unsafe {
        spacewasm_destroy(store);
        spacewasm_allocator_destroy(alloc);
    }
}

/// A mistyped resume value is reported distinctly from "not paused", and leaves
/// the pause intact so the caller can retry. Collapsing both onto `WRONG_STATE`
/// would make that retry unreachable from C.
#[test]
fn resume_with_wrong_type_is_retryable() {
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
                c"pause_i32".as_ptr(),
                c"".as_ptr(),
                c"i".as_ptr(),
                Some(pause_i32_host),
                core::ptr::null_mut(),
            ),
            status::SPACEWASM_OK
        );
    }

    let mut store: *mut CEngine = core::ptr::null_mut();
    assert_eq!(
        unsafe { spacewasm_new(host.as_mut_ptr(), 1024, 1, opts(256), &mut store) },
        status::SPACEWASM_OK
    );

    let alloc = new_guest_allocator();
    let idx = load_module_onto(alloc, store, c"main", PAUSE_I32_WASM, 0).expect("load");

    let mut func = 0u32;
    assert_eq!(
        unsafe { spacewasm_find_export_func(store, idx, c"test_pause_i32".as_ptr(), &mut func) },
        status::SPACEWASM_OK
    );

    assert_eq!(
        unsafe { spacewasm_invoke(store, idx, func, core::ptr::null(), 0) },
        status::SPACEWASM_OK
    );

    let mut trap = spacewasm_trap_t::SPACEWASM_TRAP_NONE;
    assert_eq!(
        unsafe { spacewasm_run(store, 10000, &mut trap) },
        spacewasm_run_status_t::SPACEWASM_RUN_PAUSE,
        "should pause"
    );

    // The host function declared an i32 result. Resuming with an f64 is a type
    // error, not a state error -- and must be distinguishable from WRONG_STATE.
    let f64_val = spacewasm_value_t {
        tag: spacewasm_valtype_t::SPACEWASM_F64,
        u: spacewasm_value_payload_t { f64_: 1.5 },
    };
    assert_eq!(
        unsafe { spacewasm_resume_value(store, f64_val) },
        status::SPACEWASM_ERR_PARAM_TYPE_MISMATCH,
        "mistyped resume value"
    );

    // Resuming with no value at all is the same class of error.
    assert_eq!(
        unsafe { spacewasm_resume(store) },
        status::SPACEWASM_ERR_PARAM_TYPE_MISMATCH,
        "missing resume value for a value-returning host function"
    );

    // The pause survived both rejections, so the correct value still works.
    assert_eq!(
        unsafe { spacewasm_resume_value(store, i32_val(7)) },
        status::SPACEWASM_OK,
        "retry after a rejected resume"
    );
    assert_eq!(
        run_to_completion(store, &mut trap),
        spacewasm_run_status_t::SPACEWASM_RUN_FINISHED
    );

    let mut out = i32_val(0);
    assert_eq!(
        unsafe { spacewasm_get_result(store, spacewasm_valtype_t::SPACEWASM_I32, &mut out) },
        status::SPACEWASM_OK
    );
    assert_eq!(unsafe { out.u.i32_ }, 7, "retried value took effect");

    unsafe {
        spacewasm_destroy(store);
        spacewasm_allocator_destroy(alloc);
    }
}
