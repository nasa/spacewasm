//! End-to-end tests driving the generated `spacewasm_*` C entry points directly from
//! Rust (no C toolchain required). Exercises loading, invocation, host
//! functions with guest-memory access, and error paths.

use core::ffi::c_void;
use std::ptr;
use std::sync::Mutex;

/// Serializes tests that allocate engines. The reference build's allocator
/// tracks a single global byte counter; without serialization the leak test's
/// before/after readings would race with other tests' allocations.
static TEST_LOCK: Mutex<()> = Mutex::new(());

use spacewasm_c_example::{
    spacewasm_allocator_destroy, spacewasm_allocator_new, spacewasm_builder_add_host_function,
    spacewasm_builder_add_host_module, spacewasm_builder_destroy, spacewasm_builder_finish,
    spacewasm_builder_new, spacewasm_store_destroy, spacewasm_store_find_export_func,
    spacewasm_store_get_result, spacewasm_store_invoke, spacewasm_store_load_module,
    spacewasm_store_module_needs_start, spacewasm_store_run_start, spacewasm_store_run_to_completion,
};
use spacewasm_ffi::engine::{SpacewasmCaller, spacewasm_hostcall_result_t};
use spacewasm_ffi::spacewasm_memory_statistics;
use spacewasm_ffi::status;
use spacewasm_ffi::stream::spacewasm_read_result_t;
use spacewasm_ffi::value::{spacewasm_valtype_t, spacewasm_value_payload_t, spacewasm_value_t};
use spacewasm_ffi::{Builder, SpacewasmStore, spacewasm_status_t};

// (module (func $add (export "add") (param i32 i32) (result i32)
//    local.get 0 local.get 1 i32.add))
const ADD_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x07, 0x01, 0x60, 0x02, 0x7f, 0x7f, 0x01,
    0x7f, 0x03, 0x02, 0x01, 0x00, 0x07, 0x07, 0x01, 0x03, 0x61, 0x64, 0x64, 0x00, 0x00, 0x0a, 0x09,
    0x01, 0x07, 0x00, 0x20, 0x00, 0x20, 0x01, 0x6a, 0x0b,
];

// (module
//   (import "env" "add_one" (func $add_one (param i32) (result i32)))
//   (memory (export "memory") 1)
//   (func $run (export "run") (param i32) (result i32) (local $r i32)
//     local.get 0 call $add_one local.set $r
//     i32.const 0 local.get $r i32.store
//     local.get $r))
const HOST_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x06, 0x01, 0x60, 0x01, 0x7f, 0x01, 0x7f,
    0x02, 0x0f, 0x01, 0x03, 0x65, 0x6e, 0x76, 0x07, 0x61, 0x64, 0x64, 0x5f, 0x6f, 0x6e, 0x65, 0x00,
    0x00, 0x03, 0x02, 0x01, 0x00, 0x05, 0x03, 0x01, 0x00, 0x01, 0x07, 0x10, 0x02, 0x06, 0x6d, 0x65,
    0x6d, 0x6f, 0x72, 0x79, 0x02, 0x00, 0x03, 0x72, 0x75, 0x6e, 0x00, 0x01, 0x0a, 0x15, 0x01, 0x13,
    0x01, 0x01, 0x7f, 0x20, 0x00, 0x10, 0x00, 0x21, 0x01, 0x41, 0x00, 0x20, 0x01, 0x36, 0x02, 0x00,
    0x20, 0x01, 0x0b,
];

fn i32_val(x: i32) -> spacewasm_value_t {
    spacewasm_value_t {
        tag: spacewasm_valtype_t::SPACEWASM_I32,
        u: spacewasm_value_payload_t { i32_: x },
    }
}

#[test]
fn add_module_invoke() {
    let _guard = TEST_LOCK.lock().unwrap();
    unsafe {
        let builder = spacewasm_builder_new(1, 0);
        assert!(!builder.is_null());

        let mut store = ptr::null_mut();
        let name = c"main";
        let status = load_module_buffer(builder, name.as_ptr(), ADD_WASM, 1024, &mut store);
        assert_eq!(
            status,
            status::SPACEWASM_OK,
            "load_module failed: {status:?}"
        );
        assert!(!store.is_null());

        let mut idx = u32::MAX;
        let export = c"add";
        let status = spacewasm_store_find_export_func(store, 0, export.as_ptr(), &mut idx);
        assert_eq!(status, status::SPACEWASM_OK);

        let params = [i32_val(20), i32_val(22)];
        let status = spacewasm_store_invoke(store, 0, idx, params.as_ptr(), params.len());
        assert_eq!(status, status::SPACEWASM_OK);

        let mut trap = status::SPACEWASM_TRAP_NONE;
        let rs = spacewasm_store_run_to_completion(store, 0, &mut trap);
        assert_eq!(
            rs,
            spacewasm_ffi::spacewasm_run_status_t::SPACEWASM_RUN_FINISHED
        );

        let mut out = i32_val(0);
        let status =
            spacewasm_store_get_result(store, spacewasm_valtype_t::SPACEWASM_I32, &mut out);
        assert_eq!(status, status::SPACEWASM_OK);
        assert_eq!(out.u.i32_, 42);

        spacewasm_store_destroy(store);
    }
}

/// Load two guest modules onto one store and invoke each independently by its
/// module index — the capability the store-oriented API exists to provide.
#[test]
fn two_modules_on_one_store() {
    let _guard = TEST_LOCK.lock().unwrap();
    unsafe {
        let builder = spacewasm_builder_new(2, 0);
        assert!(!builder.is_null());

        let mut store = ptr::null_mut();
        assert_eq!(
            spacewasm_builder_finish(builder, 1024, 256, &mut store),
            status::SPACEWASM_OK
        );
        assert!(!store.is_null());

        // Two independent modules, each exporting `add`.
        let mut idx_a = u32::MAX;
        assert_eq!(
            load_module_onto(store, c"a".as_ptr(), ADD_WASM),
            status::SPACEWASM_OK
        );
        // The first module lands at index 0, the second at index 1.
        let mut idx_b = u32::MAX;
        assert_eq!(
            load_module_onto(store, c"b".as_ptr(), ADD_WASM),
            status::SPACEWASM_OK
        );

        let export = c"add";
        assert_eq!(
            spacewasm_store_find_export_func(store, 0, export.as_ptr(), &mut idx_a),
            status::SPACEWASM_OK
        );
        assert_eq!(
            spacewasm_store_find_export_func(store, 1, export.as_ptr(), &mut idx_b),
            status::SPACEWASM_OK
        );

        // Invoke module 1 first, then module 0, to prove the index selects the
        // target rather than "last loaded".
        let run = |module_idx: u32, func: u32, a: i32, b: i32| -> i32 {
            let params = [i32_val(a), i32_val(b)];
            assert_eq!(
                spacewasm_store_invoke(store, module_idx, func, params.as_ptr(), params.len()),
                status::SPACEWASM_OK
            );
            let mut trap = status::SPACEWASM_TRAP_NONE;
            assert_eq!(
                spacewasm_store_run_to_completion(store, 0, &mut trap),
                spacewasm_ffi::spacewasm_run_status_t::SPACEWASM_RUN_FINISHED
            );
            let mut out = i32_val(0);
            assert_eq!(
                spacewasm_store_get_result(store, spacewasm_valtype_t::SPACEWASM_I32, &mut out),
                status::SPACEWASM_OK
            );
            out.u.i32_
        };

        assert_eq!(run(1, idx_b, 100, 1), 101);
        assert_eq!(run(0, idx_a, 20, 22), 42);

        spacewasm_store_destroy(store);
    }
}

/// Cursor over a byte slice, used as the `read_userdata` for a streaming load.
struct ReadCursor<'a> {
    data: &'a [u8],
    pos: usize,
    /// Bytes handed out per call, to exercise multi-chunk streaming.
    step: usize,
}

/// C read callback: copies up to `min(step, cap)` bytes per call from a cursor.
unsafe extern "C" fn cursor_read(
    userdata: *mut c_void,
    buf: *mut u8,
    cap: usize,
    out_len: *mut usize,
) -> spacewasm_read_result_t {
    let cursor = unsafe { &mut *(userdata as *mut ReadCursor<'_>) };
    let remaining = cursor.data.len() - cursor.pos;
    if remaining == 0 {
        unsafe { *out_len = 0 };
        return spacewasm_read_result_t::SPACEWASM_READ_EOF;
    }
    let n = remaining.min(cap).min(cursor.step);
    unsafe {
        core::ptr::copy_nonoverlapping(cursor.data.as_ptr().add(cursor.pos), buf, n);
        *out_len = n;
    }
    cursor.pos += n;
    spacewasm_read_result_t::SPACEWASM_READ_OK
}

/// Finish a builder into a store, then load a single guest module from a
/// contiguous in-memory buffer by streaming it in one chunk. Replaces the
/// removed one-shot `spacewasm_builder_load_module` convenience for these tests.
///
/// # Safety
/// `builder` (consumed) and `out_store` valid; `name` null or a valid C string.
unsafe fn load_module_buffer(
    builder: *mut Builder,
    name: *const core::ffi::c_char,
    data: &[u8],
    stack_size: usize,
    out_store: *mut *mut SpacewasmStore,
) -> spacewasm_status_t {
    unsafe {
        let status = spacewasm_builder_finish(builder, stack_size, 256, out_store);
        if status != status::SPACEWASM_OK {
            return status;
        }
        load_module_onto(*out_store, name, data)
    }
}

/// Stream a single guest module from a contiguous in-memory buffer onto an
/// existing store, handing out the whole buffer in one chunk.
///
/// # Safety
/// `store` must be live; `name` null or a valid C string.
unsafe fn load_module_onto(
    store: *mut SpacewasmStore,
    name: *const core::ffi::c_char,
    data: &[u8],
) -> spacewasm_status_t {
    // Hand out the whole buffer at once; size the scratch buffer to match.
    let chunk = data.len().max(1);
    let mut cursor = ReadCursor {
        data,
        pos: 0,
        step: chunk,
    };
    unsafe {
        let alloc = spacewasm_allocator_new(
            Some(mem_alloc),
            Some(mem_realloc),
            Some(mem_dealloc),
            ptr::null_mut(),
        );
        assert!(!alloc.is_null());
        let mut mod_idx = u32::MAX;
        let status = spacewasm_store_load_module(
            store,
            name,
            Some(cursor_read),
            &mut cursor as *mut ReadCursor<'_> as *mut c_void,
            chunk,
            alloc,
            &mut mod_idx,
        );
        // The loaded module holds its own reference; release the handle now.
        spacewasm_allocator_destroy(alloc);
        if status != status::SPACEWASM_OK {
            return status;
        }
        // Run the module's start function if it declares one (these test
        // modules do not, but this exercises the explicit-start flow).
        run_start_if_needed(store, mod_idx);
        status
    }
}

/// Run a module's start function if it declares one, asserting it finishes.
///
/// # Safety
/// `store` must be live and `module_idx` a loaded module.
unsafe fn run_start_if_needed(store: *mut SpacewasmStore, module_idx: u32) {
    unsafe {
        let mut needs_start = false;
        assert_eq!(
            spacewasm_store_module_needs_start(store, module_idx, &mut needs_start),
            status::SPACEWASM_OK
        );
        if needs_start {
            let mut trap = status::SPACEWASM_TRAP_NONE;
            assert_eq!(
                spacewasm_store_run_start(store, module_idx, 0, &mut trap),
                spacewasm_ffi::spacewasm_run_status_t::SPACEWASM_RUN_FINISHED,
                "start trapped: {trap:?}"
            );
        }
    }
}

/// Guest linear-memory allocator callbacks over `std::alloc`, honoring the
/// requested alignment (mirrors `spacewasm_util::RustSystemAllocator`).
unsafe extern "C" fn mem_alloc(_userdata: *mut c_void, size: usize, align: usize) -> *mut u8 {
    let layout = std::alloc::Layout::from_size_align(size, align).unwrap();
    unsafe { std::alloc::alloc(layout) }
}

unsafe extern "C" fn mem_realloc(
    _userdata: *mut c_void,
    ptr: *mut u8,
    old_size: usize,
    new_size: usize,
    align: usize,
) -> *mut u8 {
    let old_layout = std::alloc::Layout::from_size_align(old_size, align).unwrap();
    unsafe { std::alloc::realloc(ptr, old_layout, new_size) }
}

unsafe extern "C" fn mem_dealloc(_userdata: *mut c_void, ptr: *mut u8, size: usize, align: usize) {
    let layout = std::alloc::Layout::from_size_align(size, align).unwrap();
    unsafe { std::alloc::dealloc(ptr, layout) }
}

/// C read callback that always reports an I/O error.
unsafe extern "C" fn failing_read(
    _userdata: *mut c_void,
    _buf: *mut u8,
    _cap: usize,
    out_len: *mut usize,
) -> spacewasm_read_result_t {
    unsafe { *out_len = 0 };
    spacewasm_read_result_t::SPACEWASM_READ_ERROR
}

#[test]
fn streaming_load() {
    let _guard = TEST_LOCK.lock().unwrap();
    unsafe {
        let builder = spacewasm_builder_new(1, 0);
        assert!(!builder.is_null());

        let mut store = ptr::null_mut();
        assert_eq!(
            spacewasm_builder_finish(builder, 1024, 256, &mut store),
            status::SPACEWASM_OK
        );
        assert!(!store.is_null());

        // Force many small chunks (7 bytes) through a tiny scratch buffer (5)
        // so the module spans multiple reads and buffer refills.
        let mut cursor = ReadCursor {
            data: ADD_WASM,
            pos: 0,
            step: 7,
        };
        let name = c"main";
        let mut mod_idx = u32::MAX;
        let alloc = spacewasm_allocator_new(
            Some(mem_alloc),
            Some(mem_realloc),
            Some(mem_dealloc),
            ptr::null_mut(),
        );
        assert!(!alloc.is_null());
        let status = spacewasm_store_load_module(
            store,
            name.as_ptr(),
            Some(cursor_read),
            &mut cursor as *mut ReadCursor as *mut c_void,
            5,
            alloc,
            &mut mod_idx,
        );
        spacewasm_allocator_destroy(alloc);
        assert_eq!(
            status,
            status::SPACEWASM_OK,
            "streaming load failed: {status:?}"
        );
        assert_eq!(mod_idx, 0);

        run_start_if_needed(store, mod_idx);

        let mut idx = u32::MAX;
        let export = c"add";
        assert_eq!(
            spacewasm_store_find_export_func(store, mod_idx, export.as_ptr(), &mut idx),
            status::SPACEWASM_OK
        );
        let params = [i32_val(30), i32_val(12)];
        assert_eq!(
            spacewasm_store_invoke(store, mod_idx, idx, params.as_ptr(), params.len()),
            status::SPACEWASM_OK
        );
        let mut trap = status::SPACEWASM_TRAP_NONE;
        assert_eq!(
            spacewasm_store_run_to_completion(store, 0, &mut trap),
            spacewasm_ffi::spacewasm_run_status_t::SPACEWASM_RUN_FINISHED
        );
        let mut out = i32_val(0);
        assert_eq!(
            spacewasm_store_get_result(store, spacewasm_valtype_t::SPACEWASM_I32, &mut out),
            status::SPACEWASM_OK
        );
        assert_eq!(out.u.i32_, 42);
        spacewasm_store_destroy(store);
    }
}

#[test]
fn streaming_read_error() {
    let _guard = TEST_LOCK.lock().unwrap();
    unsafe {
        let builder = spacewasm_builder_new(1, 0);
        let mut store = ptr::null_mut();
        assert_eq!(
            spacewasm_builder_finish(builder, 1024, 256, &mut store),
            status::SPACEWASM_OK
        );
        assert!(!store.is_null());

        let name = c"main";
        let mut mod_idx = u32::MAX;
        let alloc = spacewasm_allocator_new(
            Some(mem_alloc),
            Some(mem_realloc),
            Some(mem_dealloc),
            ptr::null_mut(),
        );
        assert!(!alloc.is_null());
        let status = spacewasm_store_load_module(
            store,
            name.as_ptr(),
            Some(failing_read),
            ptr::null_mut(),
            0,
            alloc,
            &mut mod_idx,
        );
        spacewasm_allocator_destroy(alloc);
        assert_eq!(status, status::SPACEWASM_ERR_STREAM);

        spacewasm_store_destroy(store);
    }
}

/// Host callback: returns param+1 and (implicitly) lets the guest store it.
unsafe extern "C" fn add_one(
    _caller: *mut SpacewasmCaller,
    _userdata: *mut c_void,
    params: *const spacewasm_value_t,
    n: usize,
    out: *mut spacewasm_value_t,
) -> spacewasm_hostcall_result_t {
    assert_eq!(n, 1);
    let x = unsafe { (*params).u.i32_ };
    unsafe { *out = i32_val(x + 1) };
    spacewasm_hostcall_result_t::SPACEWASM_CONTINUE
}

#[test]
fn host_function_and_memory() {
    let _guard = TEST_LOCK.lock().unwrap();
    unsafe {
        let builder = spacewasm_builder_new(1, 1);
        assert!(!builder.is_null());

        let mut mod_idx = u32::MAX;
        let env = c"env";
        assert_eq!(
            spacewasm_builder_add_host_module(builder, env.as_ptr(), 1, &mut mod_idx),
            status::SPACEWASM_OK
        );

        let fname = c"add_one";
        let params_sig = c"i";
        let returns_sig = c"i";
        assert_eq!(
            spacewasm_builder_add_host_function(
                builder,
                mod_idx,
                fname.as_ptr(),
                params_sig.as_ptr(),
                returns_sig.as_ptr(),
                Some(add_one),
                ptr::null_mut(),
            ),
            status::SPACEWASM_OK
        );

        let mut store = ptr::null_mut();
        let name = c"main";
        let status = load_module_buffer(builder, name.as_ptr(), HOST_WASM, 1024, &mut store);
        assert_eq!(
            status,
            status::SPACEWASM_OK,
            "load_module failed: {status:?}"
        );

        // The guest module is the first (and only) module in the store.
        let guest_idx = 0;
        let mut idx = u32::MAX;
        let run = c"run";
        assert_eq!(
            spacewasm_store_find_export_func(store, guest_idx, run.as_ptr(), &mut idx),
            status::SPACEWASM_OK
        );

        let params = [i32_val(41)];
        assert_eq!(
            spacewasm_store_invoke(store, guest_idx, idx, params.as_ptr(), params.len()),
            status::SPACEWASM_OK
        );

        let mut trap = status::SPACEWASM_TRAP_NONE;
        let rs = spacewasm_store_run_to_completion(store, 0, &mut trap);
        assert_eq!(
            rs,
            spacewasm_ffi::spacewasm_run_status_t::SPACEWASM_RUN_FINISHED,
            "trap={trap:?}"
        );

        let mut out = i32_val(0);
        assert_eq!(
            spacewasm_store_get_result(store, spacewasm_valtype_t::SPACEWASM_I32, &mut out),
            status::SPACEWASM_OK
        );
        assert_eq!(out.u.i32_, 42, "host add_one(41) should be 42");

        spacewasm_store_destroy(store);
    }
}

#[test]
fn error_paths() {
    let _guard = TEST_LOCK.lock().unwrap();
    unsafe {
        // max_modules > 256 -> builder_new returns NULL.
        assert!(spacewasm_builder_new(257, 0).is_null());

        // Bad signature char -> SPACEWASM_ERR_BAD_SIGNATURE, no panic.
        let builder = spacewasm_builder_new(1, 1);
        let mut mod_idx = u32::MAX;
        let env = c"env";
        assert_eq!(
            spacewasm_builder_add_host_module(builder, env.as_ptr(), 1, &mut mod_idx),
            status::SPACEWASM_OK
        );
        let fname = c"bad";
        let bad_sig = c"x"; // 'x' is not a valid signature char
        let empty = c"";
        assert_eq!(
            spacewasm_builder_add_host_function(
                builder,
                mod_idx,
                fname.as_ptr(),
                bad_sig.as_ptr(),
                empty.as_ptr(),
                Some(add_one),
                ptr::null_mut(),
            ),
            status::SPACEWASM_ERR_BAD_SIGNATURE
        );
        spacewasm_builder_destroy(builder);

        // Malformed wasm -> parse error. The builder finishes into a store
        // fine; only the module load fails.
        let builder = spacewasm_builder_new(1, 0);
        let mut store = ptr::null_mut();
        let name = c"main";
        let junk = [0u8, 1, 2, 3, 4, 5, 6, 7];
        let status = load_module_buffer(builder, name.as_ptr(), &junk, 1024, &mut store);
        assert_eq!(status, status::SPACEWASM_ERR_PARSE);
        assert!(!store.is_null());
        spacewasm_store_destroy(store);
    }
}

#[test]
fn null_arg_handling() {
    let _guard = TEST_LOCK.lock().unwrap();
    unsafe {
        // NULL name to load_module.
        let builder = spacewasm_builder_new(1, 0);
        let mut store = ptr::null_mut();
        let status = load_module_buffer(builder, ptr::null(), ADD_WASM, 1024, &mut store);
        assert_eq!(status, status::SPACEWASM_ERR_NULL_ARG);
        assert!(!store.is_null());
        spacewasm_store_destroy(store);

        // A NULL store to find_export_func.
        let name = c"add";
        assert_eq!(
            spacewasm_store_find_export_func(ptr::null_mut(), 0, name.as_ptr(), &mut 0),
            status::SPACEWASM_ERR_NULL_ARG
        );
    }
}

#[test]
fn statistics_available() {
    let stats = spacewasm_memory_statistics();
    // total_bytes is a running counter; just confirm the call is wired up.
    let _ = stats.total_bytes;
    let _ = stats.pad_bytes;
}

/// Create and destroy many stores; the tracked byte total must return to its
/// starting point, validating drop order and that names/closures are freed.
#[test]
fn no_leak_across_engine_lifecycle() {
    let _guard = TEST_LOCK.lock().unwrap();
    unsafe {
        // Warm up once so any one-time allocations are already counted.
        run_add_once();
        let baseline = spacewasm_memory_statistics().total_bytes;

        for _ in 0..50 {
            run_add_once();
        }

        let after = spacewasm_memory_statistics().total_bytes;
        assert_eq!(
            after, baseline,
            "memory total drifted: baseline={baseline} after={after}"
        );
    }
}

unsafe fn run_add_once() {
    unsafe {
        let builder = spacewasm_builder_new(1, 0);
        assert!(!builder.is_null());
        let mut store = ptr::null_mut();
        let name = c"main";
        let status = load_module_buffer(builder, name.as_ptr(), ADD_WASM, 1024, &mut store);
        assert_eq!(status, status::SPACEWASM_OK);
        let mut idx = 0u32;
        let export = c"add";
        assert_eq!(
            spacewasm_store_find_export_func(store, 0, export.as_ptr(), &mut idx),
            status::SPACEWASM_OK
        );
        let params = [i32_val(1), i32_val(2)];
        assert_eq!(
            spacewasm_store_invoke(store, 0, idx, params.as_ptr(), params.len()),
            status::SPACEWASM_OK
        );
        let mut trap = status::SPACEWASM_TRAP_NONE;
        let _ = spacewasm_store_run_to_completion(store, 0, &mut trap);
        spacewasm_store_destroy(store);
    }
}
