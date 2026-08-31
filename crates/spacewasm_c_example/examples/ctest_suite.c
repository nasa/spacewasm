/*
 * ctest_suite.c — the spacewasm C API exercised end-to-end from C.
 * Built and run by tests/c_abi.rs. Returns 0 iff every case passes.
 */
#include "spacewasm.h"

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* ---- integrator-supplied hooks (see ctest.c for commentary) -------------- */

void spacewasm_panic(const uint8_t* filename, size_t filename_len, uint32_t line,
                     const uint8_t* msg, size_t len) {
    fprintf(stderr, "spacewasm panic at %.*s:%u: %.*s\n", (int)filename_len, (const char*)filename,
            line, (int)len, (const char*)msg);
    abort();
}

static uint8_t* heap_alloc(void* userdata, size_t size, size_t align) {
    (void)userdata;
    if (size == 0) {
        return NULL;
    }
    if (align < sizeof(void*)) {
        align = sizeof(void*);
    }
    size_t rounded = (size + align - 1) & ~(align - 1);
    return (uint8_t*)aligned_alloc(align, rounded);
}

static void heap_dealloc(void* userdata, uint8_t* ptr, size_t size, size_t align) {
    (void)userdata;
    (void)size;
    (void)align;
    free(ptr);
}

/* Guest linear-memory allocator callbacks (malloc-backed, alignment honored).
 */
static uint8_t* mem_alloc(void* userdata, size_t size, size_t align) {
    (void)userdata;
    if (size == 0) {
        return NULL;
    }
    if (align < sizeof(void*)) {
        align = sizeof(void*);
    }
    size_t rounded = (size + align - 1) & ~(align - 1);
    return (uint8_t*)aligned_alloc(align, rounded);
}

static uint8_t* mem_realloc(void* userdata, uint8_t* ptr, size_t old_size, size_t new_size,
                            size_t align) {
    (void)userdata;
    (void)align;
    uint8_t* out = mem_alloc(NULL, new_size, align);
    if (out && ptr) {
        size_t n = old_size < new_size ? old_size : new_size;
        memcpy(out, ptr, n);
        free(ptr);
    }
    return out;
}

static void mem_dealloc(void* userdata, uint8_t* ptr, size_t size, size_t align) {
    (void)userdata;
    (void)size;
    (void)align;
    free(ptr);
}

/* ---- test wasm modules --------------------------------------------------- */

/* (module (func (export "add") (param i32 i32) (result i32)
 *    local.get 0 local.get 1 i32.add)) */
static const uint8_t ADD_WASM[] = {0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x07, 0x01,
                                   0x60, 0x02, 0x7f, 0x7f, 0x01, 0x7f, 0x03, 0x02, 0x01, 0x00, 0x07,
                                   0x07, 0x01, 0x03, 0x61, 0x64, 0x64, 0x00, 0x00, 0x0a, 0x09, 0x01,
                                   0x07, 0x00, 0x20, 0x00, 0x20, 0x01, 0x6a, 0x0b};

/* (module
 *   (import "env" "add_one" (func $add_one (param i32) (result i32)))
 *   (memory (export "memory") 1)
 *   (func $run (export "run") (param i32) (result i32) (local $r i32)
 *     local.get 0 call $add_one local.set $r
 *     i32.const 0 local.get $r i32.store
 *     local.get $r)) */
static const uint8_t HOST_WASM[] = {
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x06, 0x01, 0x60, 0x01, 0x7f,
    0x01, 0x7f, 0x02, 0x0f, 0x01, 0x03, 0x65, 0x6e, 0x76, 0x07, 0x61, 0x64, 0x64, 0x5f,
    0x6f, 0x6e, 0x65, 0x00, 0x00, 0x03, 0x02, 0x01, 0x00, 0x05, 0x03, 0x01, 0x00, 0x01,
    0x07, 0x10, 0x02, 0x06, 0x6d, 0x65, 0x6d, 0x6f, 0x72, 0x79, 0x02, 0x00, 0x03, 0x72,
    0x75, 0x6e, 0x00, 0x01, 0x0a, 0x15, 0x01, 0x13, 0x01, 0x01, 0x7f, 0x20, 0x00, 0x10,
    0x00, 0x21, 0x01, 0x41, 0x00, 0x20, 0x01, 0x36, 0x02, 0x00, 0x20, 0x01, 0x0b};

/* (module (import "env" "pause") (func (export "test_pause") (result i32)
 *   (call 0) (i32.const 42))) */
static const uint8_t PAUSE_WASM[] = {
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x60,
    0x00, 0x00, 0x60, 0x00, 0x01, 0x7f, 0x02, 0x0d, 0x01, 0x03, 0x65, 0x6e,
    0x76, 0x05, 0x70, 0x61, 0x75, 0x73, 0x65, 0x00, 0x00, 0x03, 0x02, 0x01,
    0x01, 0x07, 0x0e, 0x01, 0x0a, 0x74, 0x65, 0x73, 0x74, 0x5f, 0x70, 0x61,
    0x75, 0x73, 0x65, 0x00, 0x01, 0x0a, 0x08, 0x01, 0x06, 0x00, 0x10, 0x00,
    0x41, 0x2a, 0x0b};

/* (module (import "env" "pause_i32") (func (export "test_pause_i32") (result i32)
 *   (call 0))) */
static const uint8_t PAUSE_I32_WASM[] = {
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x60,
    0x00, 0x01, 0x7f, 0x02, 0x11, 0x01, 0x03, 0x65, 0x6e, 0x76, 0x09, 0x70,
    0x61, 0x75, 0x73, 0x65, 0x5f, 0x69, 0x33, 0x32, 0x00, 0x00, 0x03, 0x02,
    0x01, 0x00, 0x07, 0x12, 0x01, 0x0e, 0x74, 0x65, 0x73, 0x74, 0x5f, 0x70,
    0x61, 0x75, 0x73, 0x65, 0x5f, 0x69, 0x33, 0x32, 0x00, 0x01, 0x0a, 0x06,
    0x01, 0x04, 0x00, 0x10, 0x00, 0x0b};

/* (module
 *   (global $g (export "g") (mut i32) (i32.const 7))
 *   (global $c (export "c") i32 (i32.const 42))
 *   (func (export "get_g") (result i32) global.get $g)
 *   (func (export "set_g") (param i32) local.get 0 global.set $g)) */
static const uint8_t GLOBALS_WASM[] = {
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x09, 0x02, 0x60,
    0x00, 0x01, 0x7f, 0x60, 0x01, 0x7f, 0x00, 0x03, 0x03, 0x02, 0x00, 0x01,
    0x06, 0x0b, 0x02, 0x7f, 0x01, 0x41, 0x07, 0x0b, 0x7f, 0x00, 0x41, 0x2a,
    0x0b, 0x07, 0x19, 0x04, 0x01, 0x67, 0x03, 0x00, 0x01, 0x63, 0x03, 0x01,
    0x05, 0x67, 0x65, 0x74, 0x5f, 0x67, 0x00, 0x00, 0x05, 0x73, 0x65, 0x74,
    0x5f, 0x67, 0x00, 0x01, 0x0a, 0x0d, 0x02, 0x04, 0x00, 0x23, 0x00, 0x0b,
    0x06, 0x00, 0x20, 0x00, 0x24, 0x00, 0x0b};

/* (module
 *   (global $gi (export "gi") (mut i32) (i32.const 10))
 *   (global $gI (export "gI") (mut i64) (i64.const 20))
 *   (global $gf (export "gf") (mut f32) (f32.const 1.5))
 *   (global $gd (export "gd") (mut f64) (f64.const 2.5))
 *   (func (export "get_gI") (result i64) global.get $gI)) */
static const uint8_t GLOBALS_MULTI_WASM[] = {
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x60,
    0x00, 0x01, 0x7e, 0x03, 0x02, 0x01, 0x00, 0x06, 0x1f, 0x04, 0x7f, 0x01,
    0x41, 0x0a, 0x0b, 0x7e, 0x01, 0x42, 0x14, 0x0b, 0x7d, 0x01, 0x43, 0x00,
    0x00, 0xc0, 0x3f, 0x0b, 0x7c, 0x01, 0x44, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x04, 0x40, 0x0b, 0x07, 0x1e, 0x05, 0x02, 0x67, 0x69, 0x03, 0x00,
    0x02, 0x67, 0x49, 0x03, 0x01, 0x02, 0x67, 0x66, 0x03, 0x02, 0x02, 0x67,
    0x64, 0x03, 0x03, 0x06, 0x67, 0x65, 0x74, 0x5f, 0x67, 0x49, 0x00, 0x00,
    0x0a, 0x06, 0x01, 0x04, 0x00, 0x23, 0x01, 0x0b};

/* Exporter half of the imported-global pair, module "b":
 * (module (global (export "bg") (mut i32) (i32.const 55))) */
static const uint8_t GLOBAL_EXPORTER_WASM[] = {
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x06, 0x06, 0x01, 0x7f,
    0x01, 0x41, 0x37, 0x0b, 0x07, 0x06, 0x01, 0x02, 0x62, 0x67, 0x03, 0x00};

/* Importer half, module "a": imports b.bg (index-space global 0), then defines
 * and exports its own mutable i32 `ag` (module-local global 0, index-space 1),
 * and re-exports the import as `reexport`. Proves find_global returns the
 * module-local index (0 for `ag`), and that a re-exported import misses.
 * (module
 *   (import "b" "bg" (global $ig (mut i32)))
 *   (global $ag (export "ag") (mut i32) (i32.const 11))
 *   (export "reexport" (global $ig))) */
static const uint8_t GLOBAL_IMPORTER_WASM[] = {
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x02, 0x09, 0x01, 0x01,
    0x62, 0x02, 0x62, 0x67, 0x03, 0x7f, 0x01, 0x06, 0x06, 0x01, 0x7f, 0x01,
    0x41, 0x0b, 0x0b, 0x07, 0x11, 0x02, 0x02, 0x61, 0x67, 0x03, 0x01, 0x08,
    0x72, 0x65, 0x65, 0x78, 0x70, 0x6f, 0x72, 0x74, 0x03, 0x00};

/* ---- helpers ------------------------------------------------------------- */

#define CHECK(cond, ...)                                                                           \
    do {                                                                                           \
        if (!(cond)) {                                                                             \
            fprintf(stderr, "FAIL %s: ", __func__);                                                \
            fprintf(stderr, __VA_ARGS__);                                                          \
            fprintf(stderr, "\n");                                                                 \
            return 1;                                                                              \
        }                                                                                          \
    } while (0)

static spacewasm_value_t i32_val(int32_t x) {
    spacewasm_value_t v;
    v.tag = SPACEWASM_I32;
    v.u.i32_ = x;
    return v;
}

/* Default compiler options bounding a store to `max_code_pages` code pages. */
static spacewasm_compiler_options_t opts(uint32_t max_code_pages) {
    spacewasm_compiler_options_t o;
    o.allow_memory_grow = false;
    o.max_backpatch_iterations = 0;
    o.max_code_pages = max_code_pages;
    return o;
}

/* A cursor over a byte slice, handing out `step` bytes per read call. The
 * callback owns the buffer, so it points `out_buf` into the slice directly. */
typedef struct {
    const uint8_t* data;
    size_t len;
    size_t pos;
    size_t step;
} cursor_t;

static spacewasm_read_result_t cursor_read(void* userdata, const uint8_t** out_buf,
                                           size_t* out_len) {
    cursor_t* c = (cursor_t*)userdata;
    size_t remaining = c->len - c->pos;
    if (remaining == 0) {
        *out_len = 0;
        return SPACEWASM_READ_EOF;
    }
    size_t n = (c->step && remaining > c->step) ? c->step : remaining;
    *out_buf = c->data + c->pos;
    c->pos += n;
    *out_len = n;
    return SPACEWASM_READ_OK;
}

static spacewasm_read_result_t failing_read(void* userdata, const uint8_t** out_buf,
                                            size_t* out_len) {
    (void)userdata;
    (void)out_buf;
    *out_len = 0;
    return SPACEWASM_READ_ERROR;
}

/* Stream one module onto an existing store in `step`-byte chunks (0 => whole
 * buffer at once). Runs the start function if declared. */
static spacewasm_status_t load_module_onto(spacewasm_allocator_t* alloc, spacewasm_t* store,
                                           const char* name, const uint8_t* data, size_t len,
                                           size_t step, uint32_t* out_idx) {
    cursor_t cursor = {data, len, 0, step};
    spacewasm_status_t st =
        spacewasm_load_module(store, name, cursor_read, &cursor, alloc, out_idx);
    if (st != SPACEWASM_OK) {
        return st;
    }

    /* Resolve the start function (if any) and drive it to completion. A module
     * without a start function reports NOT_FOUND and needs no initialization. */
    uint32_t start_mod = 0;
    uint32_t start_func = 0;
    spacewasm_status_t start_st = spacewasm_module_start(store, *out_idx, &start_mod, &start_func);
    if (start_st == SPACEWASM_ERR_NOT_FOUND) {
        return SPACEWASM_OK;
    }
    if (start_st != SPACEWASM_OK) {
        return start_st;
    }

    st = spacewasm_invoke(store, start_mod, start_func, NULL, 0);
    if (st != SPACEWASM_OK) {
        return st;
    }

    spacewasm_trap_t trap = SPACEWASM_TRAP_NONE;
    spacewasm_run_status_t rs = SPACEWASM_RUN_OUT_OF_FUEL;
    while (rs == SPACEWASM_RUN_OUT_OF_FUEL) {
        rs = spacewasm_run(store, 1000, &trap);
    }

    return rs == SPACEWASM_RUN_FINISHED ? SPACEWASM_OK : SPACEWASM_ERR_WRONG_STATE;
}

static spacewasm_run_status_t run_to_completion(spacewasm_t* store,
                                                spacewasm_trap_t* out_trap) {
    spacewasm_run_status_t rs = SPACEWASM_RUN_OUT_OF_FUEL;
    while (rs == SPACEWASM_RUN_OUT_OF_FUEL) {
        rs = spacewasm_run(store, 1000, out_trap);
    }

    return rs;
}

/* ---- test cases ---------------------------------------------------------- */

static int test_add_module_invoke(void) {
    spacewasm_host_t host;
    CHECK(spacewasm_host_new(0, &host) == SPACEWASM_OK, "host_new");

    spacewasm_t* store = NULL;
    CHECK(spacewasm_new(&host, 1024, 1, opts(256), &store) == SPACEWASM_OK, "store_new");

    spacewasm_allocator_t* alloc =
        spacewasm_allocator_new(mem_alloc, mem_realloc, mem_dealloc, NULL);

    uint32_t mod_idx = 0;
    CHECK(load_module_onto(alloc, store, "main", ADD_WASM, sizeof(ADD_WASM), 0, &mod_idx) ==
              SPACEWASM_OK,
          "load");

    uint32_t idx = 0;
    CHECK(spacewasm_find_export_func(store, 0, "add", &idx) == SPACEWASM_OK, "find");

    spacewasm_value_t params[2] = {i32_val(20), i32_val(22)};
    CHECK(spacewasm_invoke(store, 0, idx, params, 2) == SPACEWASM_OK, "invoke");
    spacewasm_trap_t trap = SPACEWASM_TRAP_NONE;
    CHECK(run_to_completion(store, &trap) == SPACEWASM_RUN_FINISHED, "run");

    spacewasm_value_t out = i32_val(0);
    CHECK(spacewasm_get_result(store, SPACEWASM_I32, &out) == SPACEWASM_OK, "result");
    CHECK(out.u.i32_ == 42, "add(20,22)=%d", out.u.i32_);

    spacewasm_destroy(store);
    spacewasm_allocator_destroy(alloc);

    return 0;
}

static int invoke_add(spacewasm_t* store, uint32_t mod, uint32_t func, int32_t a, int32_t b,
                      int32_t* out_val) {
    spacewasm_value_t params[2] = {i32_val(a), i32_val(b)};
    if (spacewasm_invoke(store, mod, func, params, 2) != SPACEWASM_OK) {
        return 1;
    }
    spacewasm_trap_t trap = SPACEWASM_TRAP_NONE;
    if (run_to_completion(store, &trap) != SPACEWASM_RUN_FINISHED) {
        return 1;
    }
    spacewasm_value_t out = i32_val(0);
    if (spacewasm_get_result(store, SPACEWASM_I32, &out) != SPACEWASM_OK) {
        return 1;
    }
    *out_val = out.u.i32_;
    return 0;
}

static int test_two_modules_on_one_store(void) {
    spacewasm_host_t host;
    CHECK(spacewasm_host_new(0, &host) == SPACEWASM_OK, "host_new");
    spacewasm_t* store = NULL;
    CHECK(spacewasm_new(&host, 1024, 2, opts(256), &store) == SPACEWASM_OK, "store_new");

    spacewasm_allocator_t* alloc =
        spacewasm_allocator_new(mem_alloc, mem_realloc, mem_dealloc, NULL);

    uint32_t a = 0, b = 0;
    CHECK(load_module_onto(alloc, store, "a", ADD_WASM, sizeof(ADD_WASM), 0, &a) == SPACEWASM_OK,
          "load a");
    CHECK(load_module_onto(alloc, store, "b", ADD_WASM, sizeof(ADD_WASM), 0, &b) == SPACEWASM_OK,
          "load b");
    CHECK(a == 0 && b == 1, "indices a=%u b=%u", a, b);

    uint32_t idx_a = 0, idx_b = 0;
    CHECK(spacewasm_find_export_func(store, 0, "add", &idx_a) == SPACEWASM_OK, "find a");
    CHECK(spacewasm_find_export_func(store, 1, "add", &idx_b) == SPACEWASM_OK, "find b");

    /* Invoke module 1 first, then 0, to prove the index selects the target. */
    int32_t v = 0;
    CHECK(invoke_add(store, 1, idx_b, 100, 1, &v) == 0 && v == 101, "b=%d", v);
    CHECK(invoke_add(store, 0, idx_a, 20, 22, &v) == 0 && v == 42, "a=%d", v);

    spacewasm_destroy(store);
    spacewasm_allocator_destroy(alloc);
    return 0;
}

static int test_streaming_load(void) {
    spacewasm_host_t host;
    CHECK(spacewasm_host_new(0, &host) == SPACEWASM_OK, "host_new");
    spacewasm_t* store = NULL;
    CHECK(spacewasm_new(&host, 1024, 1, opts(256), &store) == SPACEWASM_OK, "store_new");

    spacewasm_allocator_t* alloc =
        spacewasm_allocator_new(mem_alloc, mem_realloc, mem_dealloc, NULL);

    /* Force many small 7-byte chunks through a tiny 5-byte scratch buffer. */
    uint32_t mod_idx = 0;
    CHECK(load_module_onto(alloc, store, "main", ADD_WASM, sizeof(ADD_WASM), 7, &mod_idx) ==
              SPACEWASM_OK,
          "streaming load");

    uint32_t idx = 0;
    CHECK(spacewasm_find_export_func(store, mod_idx, "add", &idx) == SPACEWASM_OK, "find");
    int32_t v = 0;
    CHECK(invoke_add(store, mod_idx, idx, 30, 12, &v) == 0 && v == 42, "=%d", v);

    spacewasm_destroy(store);
    spacewasm_allocator_destroy(alloc);
    return 0;
}

static int test_streaming_read_error(void) {
    spacewasm_host_t host;
    CHECK(spacewasm_host_new(0, &host) == SPACEWASM_OK, "host_new");
    spacewasm_t* store = NULL;
    CHECK(spacewasm_new(&host, 1024, 1, opts(256), &store) == SPACEWASM_OK, "store_new");

    spacewasm_allocator_t* alloc =
        spacewasm_allocator_new(mem_alloc, mem_realloc, mem_dealloc, NULL);
    CHECK(alloc, "allocator_new");
    uint32_t mod_idx = 0;
    spacewasm_status_t st =
        spacewasm_load_module(store, "main", failing_read, NULL, alloc, &mod_idx);
    spacewasm_allocator_destroy(alloc);
    CHECK(st == SPACEWASM_ERR_READER_ERROR, "expected ERR_READER_ERROR, got %d", (int)st);

    spacewasm_destroy(store);
    return 0;
}

/* Host callback: returns param + 1. */
static spacewasm_hostcall_result_t add_one(spacewasm_caller_t* caller, void* userdata,
                                           const spacewasm_value_t* params, size_t n,
                                           spacewasm_value_t* out) {
    (void)caller;
    (void)userdata;
    if (n != 1) {
        return SPACEWASM_TRAP;
    }
    *out = i32_val(params[0].u.i32_ + 1);
    return SPACEWASM_CONTINUE_SOME;
}

static int test_host_function_and_memory(void) {
    spacewasm_host_t host;
    CHECK(spacewasm_host_new(1, &host) == SPACEWASM_OK, "host_new");

    uint32_t hmod = 0;
    CHECK(spacewasm_add_host_module(&host, "env", 1, 0, &hmod) == SPACEWASM_OK, "add_host_module");
    CHECK(spacewasm_add_host_function(&host, hmod, "add_one", "i", "i", add_one, NULL) ==
              SPACEWASM_OK,
          "add_host_function");

    spacewasm_t* store = NULL;
    CHECK(spacewasm_new(&host, 1024, 1, opts(256), &store) == SPACEWASM_OK, "store_new");

    spacewasm_allocator_t* alloc =
        spacewasm_allocator_new(mem_alloc, mem_realloc, mem_dealloc, NULL);

    uint32_t mod_idx = 0;
    CHECK(load_module_onto(alloc, store, "main", HOST_WASM, sizeof(HOST_WASM), 0, &mod_idx) ==
              SPACEWASM_OK,
          "load host module");

    uint32_t idx = 0;
    CHECK(spacewasm_find_export_func(store, 0, "run", &idx) == SPACEWASM_OK, "find run");
    spacewasm_value_t params[1] = {i32_val(41)};
    CHECK(spacewasm_invoke(store, 0, idx, params, 1) == SPACEWASM_OK, "invoke");
    spacewasm_trap_t trap = SPACEWASM_TRAP_NONE;
    CHECK(run_to_completion(store, &trap) == SPACEWASM_RUN_FINISHED, "run (trap=%d)", (int)trap);
    spacewasm_value_t out = i32_val(0);
    CHECK(spacewasm_get_result(store, SPACEWASM_I32, &out) == SPACEWASM_OK, "result");
    CHECK(out.u.i32_ == 42, "add_one(41)=%d", out.u.i32_);

    spacewasm_destroy(store);
    spacewasm_allocator_destroy(alloc);
    return 0;
}

static int test_globals(void) {
    spacewasm_host_t host;
    CHECK(spacewasm_host_new(0, &host) == SPACEWASM_OK, "host_new");
    spacewasm_t* store = NULL;
    CHECK(spacewasm_new(&host, 1024, 1, opts(256), &store) == SPACEWASM_OK, "store_new");

    spacewasm_allocator_t* alloc =
        spacewasm_allocator_new(mem_alloc, mem_realloc, mem_dealloc, NULL);

    uint32_t mod_idx = 0;
    CHECK(load_module_onto(alloc, store, "main", GLOBALS_WASM, sizeof(GLOBALS_WASM), 0, &mod_idx) ==
              SPACEWASM_OK,
          "load");

    /* Resolve the exported mutable global `g` and const global `c`. */
    uint32_t g = 0, c = 0;
    CHECK(spacewasm_find_global(store, mod_idx, "g", &g) == SPACEWASM_OK, "find g");
    CHECK(spacewasm_find_global(store, mod_idx, "c", &c) == SPACEWASM_OK, "find c");

    /* A missing global, and a function export looked up as a global, both miss. */
    uint32_t sink = 0;
    CHECK(spacewasm_find_global(store, mod_idx, "nope", &sink) == SPACEWASM_ERR_NOT_FOUND,
          "missing global");
    CHECK(spacewasm_find_global(store, mod_idx, "get_g", &sink) == SPACEWASM_ERR_NOT_FOUND,
          "function is not a global");

    /* Initial values. */
    spacewasm_value_t out = i32_val(0);
    CHECK(spacewasm_get_global(store, mod_idx, g, &out) == SPACEWASM_OK, "get g");
    CHECK(out.tag == SPACEWASM_I32 && out.u.i32_ == 7, "g init = %d", out.u.i32_);
    CHECK(spacewasm_get_global(store, mod_idx, c, &out) == SPACEWASM_OK, "get c");
    CHECK(out.u.i32_ == 42, "c init = %d", out.u.i32_);

    /* Writing `g` is visible both through get_global and through executing get_g. */
    CHECK(spacewasm_set_global(store, mod_idx, g, i32_val(100)) == SPACEWASM_OK, "set g");
    CHECK(spacewasm_get_global(store, mod_idx, g, &out) == SPACEWASM_OK, "get g after set");
    CHECK(out.u.i32_ == 100, "g after set = %d", out.u.i32_);

    uint32_t get_g = 0;
    CHECK(spacewasm_find_export_func(store, mod_idx, "get_g", &get_g) == SPACEWASM_OK, "find get_g");
    CHECK(spacewasm_invoke(store, mod_idx, get_g, NULL, 0) == SPACEWASM_OK, "invoke get_g");
    spacewasm_trap_t trap = SPACEWASM_TRAP_NONE;
    CHECK(run_to_completion(store, &trap) == SPACEWASM_RUN_FINISHED, "run get_g");
    CHECK(spacewasm_get_result(store, SPACEWASM_I32, &out) == SPACEWASM_OK, "result get_g");
    CHECK(out.u.i32_ == 100, "get_g observes set_global = %d", out.u.i32_);

    /* Conversely, `set_g` from Wasm is observable through get_global. */
    uint32_t set_g = 0;
    CHECK(spacewasm_find_export_func(store, mod_idx, "set_g", &set_g) == SPACEWASM_OK, "find set_g");
    spacewasm_value_t arg = i32_val(5);
    CHECK(spacewasm_invoke(store, mod_idx, set_g, &arg, 1) == SPACEWASM_OK, "invoke set_g");
    CHECK(run_to_completion(store, &trap) == SPACEWASM_RUN_FINISHED, "run set_g");
    CHECK(spacewasm_get_global(store, mod_idx, g, &out) == SPACEWASM_OK, "get g after wasm set");
    CHECK(out.u.i32_ == 5, "get_global observes set_g = %d", out.u.i32_);

    /* Writing a const global is rejected and leaves it unchanged. */
    CHECK(spacewasm_set_global(store, mod_idx, c, i32_val(1)) == SPACEWASM_ERR_GLOBAL_IS_NOT_MUTABLE,
          "set const");
    CHECK(spacewasm_get_global(store, mod_idx, c, &out) == SPACEWASM_OK, "get c after reject");
    CHECK(out.u.i32_ == 42, "c unchanged = %d", out.u.i32_);

    /* A type-mismatched value is rejected and leaves the global unchanged. */
    spacewasm_value_t i64v = {SPACEWASM_I64, {.i64_ = 9}};
    CHECK(spacewasm_set_global(store, mod_idx, g, i64v) == SPACEWASM_ERR_GLOBAL_TYPE_MISMATCH,
          "type mismatch");
    CHECK(spacewasm_get_global(store, mod_idx, g, &out) == SPACEWASM_OK, "get g after mismatch");
    CHECK(out.u.i32_ == 5, "g unchanged after mismatch = %d", out.u.i32_);

    /* The type check runs before the mutability check: a doubly-invalid write
     * (wrong type into a const global) reports the type mismatch. */
    CHECK(spacewasm_set_global(store, mod_idx, c, i64v) == SPACEWASM_ERR_GLOBAL_TYPE_MISMATCH,
          "type check precedes mutability check");

    /* Out-of-range indices miss. */
    CHECK(spacewasm_get_global(store, mod_idx, 999, &out) == SPACEWASM_ERR_NOT_FOUND,
          "get oob global");
    CHECK(spacewasm_set_global(store, mod_idx, 999, i32_val(0)) == SPACEWASM_ERR_NOT_FOUND,
          "set oob global");
    CHECK(spacewasm_get_global(store, 999, g, &out) == SPACEWASM_ERR_NOT_FOUND, "get oob module");
    CHECK(spacewasm_set_global(store, 999, g, i32_val(0)) == SPACEWASM_ERR_NOT_FOUND,
          "set oob module");
    CHECK(spacewasm_find_global(store, 999, "g", &sink) == SPACEWASM_ERR_NOT_FOUND,
          "find oob module");

    /* NULL argument handling. */
    CHECK(spacewasm_find_global(NULL, mod_idx, "g", &sink) == SPACEWASM_ERR_NULL_ARG,
          "find null engine");
    CHECK(spacewasm_find_global(store, mod_idx, NULL, &sink) == SPACEWASM_ERR_NULL_ARG,
          "find null name");
    CHECK(spacewasm_find_global(store, mod_idx, "g", NULL) == SPACEWASM_ERR_NULL_ARG,
          "find null out_index");
    CHECK(spacewasm_get_global(NULL, mod_idx, g, &out) == SPACEWASM_ERR_NULL_ARG,
          "get null engine");
    CHECK(spacewasm_get_global(store, mod_idx, g, NULL) == SPACEWASM_ERR_NULL_ARG, "get null out");
    CHECK(spacewasm_set_global(NULL, mod_idx, g, i32_val(0)) == SPACEWASM_ERR_NULL_ARG,
          "set null engine");

    spacewasm_destroy(store);
    spacewasm_allocator_destroy(alloc);
    return 0;
}

/* Round-trips each value type through set_global/get_global and proves the i64
 * write is visible to Wasm. */
static int test_globals_all_types(void) {
    spacewasm_host_t host;
    CHECK(spacewasm_host_new(0, &host) == SPACEWASM_OK, "host_new");
    spacewasm_t* store = NULL;
    CHECK(spacewasm_new(&host, 1024, 1, opts(256), &store) == SPACEWASM_OK, "store_new");

    spacewasm_allocator_t* alloc =
        spacewasm_allocator_new(mem_alloc, mem_realloc, mem_dealloc, NULL);

    uint32_t mod_idx = 0;
    CHECK(load_module_onto(alloc, store, "main", GLOBALS_MULTI_WASM, sizeof(GLOBALS_MULTI_WASM), 0,
                           &mod_idx) == SPACEWASM_OK,
          "load");

    uint32_t gi = 0, gi64 = 0, gf = 0, gd = 0;
    CHECK(spacewasm_find_global(store, mod_idx, "gi", &gi) == SPACEWASM_OK, "find gi");
    CHECK(spacewasm_find_global(store, mod_idx, "gI", &gi64) == SPACEWASM_OK, "find gI");
    CHECK(spacewasm_find_global(store, mod_idx, "gf", &gf) == SPACEWASM_OK, "find gf");
    CHECK(spacewasm_find_global(store, mod_idx, "gd", &gd) == SPACEWASM_OK, "find gd");

    /* Initial values, with tag checks per type. */
    spacewasm_value_t out = i32_val(0);
    CHECK(spacewasm_get_global(store, mod_idx, gi, &out) == SPACEWASM_OK, "get gi");
    CHECK(out.tag == SPACEWASM_I32 && out.u.i32_ == 10, "gi init = %d", out.u.i32_);
    CHECK(spacewasm_get_global(store, mod_idx, gi64, &out) == SPACEWASM_OK, "get gI");
    CHECK(out.tag == SPACEWASM_I64 && out.u.i64_ == 20, "gI init");
    CHECK(spacewasm_get_global(store, mod_idx, gf, &out) == SPACEWASM_OK, "get gf");
    CHECK(out.tag == SPACEWASM_F32 && out.u.f32_ == 1.5f, "gf init");
    CHECK(spacewasm_get_global(store, mod_idx, gd, &out) == SPACEWASM_OK, "get gd");
    CHECK(out.tag == SPACEWASM_F64 && out.u.f64_ == 2.5, "gd init");

    /* Round-trip a value of each type, including a negative i64 and -0.0f. */
    spacewasm_value_t iv = {SPACEWASM_I64, {.i64_ = -9000000000LL}};
    spacewasm_value_t fv = {SPACEWASM_F32, {.f32_ = -0.0f}};
    spacewasm_value_t dv = {SPACEWASM_F64, {.f64_ = 3.25}};
    CHECK(spacewasm_set_global(store, mod_idx, gi64, iv) == SPACEWASM_OK, "set gI");
    CHECK(spacewasm_set_global(store, mod_idx, gf, fv) == SPACEWASM_OK, "set gf");
    CHECK(spacewasm_set_global(store, mod_idx, gd, dv) == SPACEWASM_OK, "set gd");

    CHECK(spacewasm_get_global(store, mod_idx, gi64, &out) == SPACEWASM_OK, "get gI back");
    CHECK(out.u.i64_ == -9000000000LL, "i64 round-trip");
    CHECK(spacewasm_get_global(store, mod_idx, gf, &out) == SPACEWASM_OK, "get gf back");
    /* -0.0 compares equal to 0.0; check the sign bit survived via 1/x = -inf. */
    CHECK(out.u.f32_ == 0.0f && 1.0f / out.u.f32_ < 0.0f, "f32 -0.0 round-trip");
    CHECK(spacewasm_get_global(store, mod_idx, gd, &out) == SPACEWASM_OK, "get gd back");
    CHECK(out.u.f64_ == 3.25, "f64 round-trip");

    /* The i64 write is observable from Wasm. */
    uint32_t get_gi64 = 0;
    CHECK(spacewasm_find_export_func(store, mod_idx, "get_gI", &get_gi64) == SPACEWASM_OK,
          "find get_gI");
    CHECK(spacewasm_invoke(store, mod_idx, get_gi64, NULL, 0) == SPACEWASM_OK, "invoke get_gI");
    spacewasm_trap_t trap = SPACEWASM_TRAP_NONE;
    CHECK(run_to_completion(store, &trap) == SPACEWASM_RUN_FINISHED, "run get_gI");
    CHECK(spacewasm_get_result(store, SPACEWASM_I64, &out) == SPACEWASM_OK, "result get_gI");
    CHECK(out.u.i64_ == -9000000000LL, "get_gI observes set_global");

    spacewasm_destroy(store);
    spacewasm_allocator_destroy(alloc);
    return 0;
}

/* Proves find_global returns the module-local index (skipping imported globals)
 * and that a re-exported imported global misses. */
static int test_globals_imported(void) {
    spacewasm_host_t host;
    CHECK(spacewasm_host_new(0, &host) == SPACEWASM_OK, "host_new");
    spacewasm_t* store = NULL;
    CHECK(spacewasm_new(&host, 1024, 2, opts(256), &store) == SPACEWASM_OK, "store_new");

    spacewasm_allocator_t* alloc =
        spacewasm_allocator_new(mem_alloc, mem_realloc, mem_dealloc, NULL);

    /* The exporter (`b`) must load before the importer (`a`). */
    uint32_t b = 0, a = 0;
    CHECK(load_module_onto(alloc, store, "b", GLOBAL_EXPORTER_WASM, sizeof(GLOBAL_EXPORTER_WASM), 0,
                           &b) == SPACEWASM_OK,
          "load b");
    CHECK(load_module_onto(alloc, store, "a", GLOBAL_IMPORTER_WASM, sizeof(GLOBAL_IMPORTER_WASM), 0,
                           &a) == SPACEWASM_OK,
          "load a");
    CHECK(b == 0 && a == 1, "module indices b=%u a=%u", b, a);

    /* `ag` is index-space slot 1 (after the import) but module-local global 0. */
    uint32_t ag = 999;
    CHECK(spacewasm_find_global(store, a, "ag", &ag) == SPACEWASM_OK, "find ag");
    CHECK(ag == 0, "ag module-local index = %u (expected 0)", ag);

    /* The re-exported import belongs to module b, so it misses through a. */
    uint32_t sink = 999;
    CHECK(spacewasm_find_global(store, a, "reexport", &sink) == SPACEWASM_ERR_NOT_FOUND,
          "re-exported import misses");

    spacewasm_value_t out = i32_val(0);
    CHECK(spacewasm_get_global(store, a, ag, &out) == SPACEWASM_OK, "get ag");
    CHECK(out.u.i32_ == 11, "ag init = %d", out.u.i32_);
    CHECK(spacewasm_set_global(store, a, ag, i32_val(77)) == SPACEWASM_OK, "set ag");
    CHECK(spacewasm_get_global(store, a, ag, &out) == SPACEWASM_OK, "get ag back");
    CHECK(out.u.i32_ == 77, "ag after set = %d", out.u.i32_);

    /* Module b's own global is reachable directly and untouched. */
    uint32_t bg = 999;
    CHECK(spacewasm_find_global(store, b, "bg", &bg) == SPACEWASM_OK, "find bg");
    CHECK(bg == 0, "bg module-local index = %u", bg);
    CHECK(spacewasm_get_global(store, b, bg, &out) == SPACEWASM_OK, "get bg");
    CHECK(out.u.i32_ == 55, "bg unchanged = %d", out.u.i32_);

    spacewasm_destroy(store);
    spacewasm_allocator_destroy(alloc);
    return 0;
}

static int test_error_paths(void) {
    /* max_modules > 256 -> store_new returns ERR_BAD_ARG (consumes the host). */
    spacewasm_host_t host;
    CHECK(spacewasm_host_new(0, &host) == SPACEWASM_OK, "host_new");
    spacewasm_t* store = NULL;
    CHECK(spacewasm_new(&host, 1024, 257, opts(256), &store) == SPACEWASM_ERR_BAD_ARG,
          "oversized max_modules");

    /* Host function signature errors each map to a distinct status, no panic. */
    CHECK(spacewasm_host_new(1, &host) == SPACEWASM_OK, "host_new");
    uint32_t hmod = 0;
    CHECK(spacewasm_add_host_module(&host, "env", 4, 0, &hmod) == SPACEWASM_OK, "add_host_module");
    /* Invalid value-list character in the parameter signature. */
    CHECK(spacewasm_add_host_function(&host, hmod, "bad_param", "x", "", add_one, NULL) ==
              SPACEWASM_ERR_BAD_ARG,
          "invalid param char");
    /* Invalid value-list character in the return signature. */
    CHECK(spacewasm_add_host_function(&host, hmod, "bad_ret", "i", "z", add_one, NULL) ==
              SPACEWASM_ERR_BAD_ARG,
          "invalid return char");
    /* More than MAX_HOST_FUNCTION_PARAMS (9) parameters. */
    CHECK(spacewasm_add_host_function(&host, hmod, "too_many", "iiiiiiiiii", "", add_one, NULL) ==
              SPACEWASM_ERR_FUNCTION_PARAMETERS_TOO_LARGE,
          "too many params");
    /* More than one return value is not supported. */
    CHECK(spacewasm_add_host_function(&host, hmod, "multi_ret", "i", "ii", add_one, NULL) ==
              SPACEWASM_ERR_FUNCTION_RETURNS_TOO_LARGE,
          "multiple returns");
    /* A valid signature still succeeds after the rejected attempts. */
    CHECK(spacewasm_add_host_function(&host, hmod, "ok", "i", "i", add_one, NULL) == SPACEWASM_OK,
          "valid signature");
    spacewasm_host_destroy(&host);

    /* Malformed wasm -> parse error; the store is still created fine. */
    CHECK(spacewasm_host_new(0, &host) == SPACEWASM_OK, "host_new");
    CHECK(spacewasm_new(&host, 1024, 1, opts(256), &store) == SPACEWASM_OK, "store_new");
    const uint8_t junk[] = {0, 1, 2, 3, 4, 5, 6, 7};
    uint32_t mod_idx = 0;
    spacewasm_allocator_t* alloc =
        spacewasm_allocator_new(mem_alloc, mem_realloc, mem_dealloc, NULL);
    CHECK(alloc, "allocator_new");
    cursor_t cursor = {junk, sizeof(junk), 0, 0};
    spacewasm_status_t st =
        spacewasm_load_module(store, "main", cursor_read, &cursor, alloc, &mod_idx);
    spacewasm_allocator_destroy(alloc);
    CHECK(st == SPACEWASM_ERR_MALFORMED_MAGIC, "expected ERR_MALFORMED_MAGIC, got %d", (int)st);
    spacewasm_destroy(store);
    return 0;
}

static int test_null_arg_handling(void) {
    /* NULL name to load_module. */
    spacewasm_host_t host;
    CHECK(spacewasm_host_new(0, &host) == SPACEWASM_OK, "host_new");
    spacewasm_t* store = NULL;
    CHECK(spacewasm_new(&host, 1024, 1, opts(256), &store) == SPACEWASM_OK, "store_new");
    spacewasm_allocator_t* alloc =
        spacewasm_allocator_new(mem_alloc, mem_realloc, mem_dealloc, NULL);
    CHECK(alloc, "allocator_new");
    cursor_t cursor = {ADD_WASM, sizeof(ADD_WASM), 0, 0};
    uint32_t mod_idx = 0;
    spacewasm_status_t st =
        spacewasm_load_module(store, NULL, cursor_read, &cursor, alloc, &mod_idx);
    spacewasm_allocator_destroy(alloc);
    CHECK(st == SPACEWASM_ERR_NULL_ARG, "expected NULL_ARG, got %d", (int)st);

    /* NULL store to find_export_func. */
    uint32_t idx = 0;
    CHECK(spacewasm_find_export_func(NULL, 0, "add", &idx) == SPACEWASM_ERR_NULL_ARG,
          "null store");

    spacewasm_destroy(store);
    return 0;
}

static int test_statistics_available(void) {
    spacewasm_memory_statistics_t stats = spacewasm_memory_statistics();
    /* Reported by the page allocator's local tracking; just confirm it's wired.
     */
    (void)stats.total_bytes;
    (void)stats.pad_bytes;
    return 0;
}

static int run_add_once(void) {
    spacewasm_host_t host;
    if (spacewasm_host_new(0, &host) != SPACEWASM_OK) {
        return 1;
    }
    spacewasm_t* store = NULL;
    if (spacewasm_new(&host, 1024, 1, opts(256), &store) != SPACEWASM_OK) {
        return 1;
    }

    spacewasm_allocator_t* alloc =
        spacewasm_allocator_new(mem_alloc, mem_realloc, mem_dealloc, NULL);

    uint32_t mod_idx = 0;
    if (load_module_onto(alloc, store, "main", ADD_WASM, sizeof(ADD_WASM), 0, &mod_idx) !=
        SPACEWASM_OK) {
        return 1;
    }
    uint32_t idx = 0;
    if (spacewasm_find_export_func(store, 0, "add", &idx) != SPACEWASM_OK) {
        return 1;
    }
    int32_t v = 0;
    int rc = invoke_add(store, 0, idx, 1, 2, &v);
    spacewasm_destroy(store);
    spacewasm_allocator_destroy(alloc);
    return rc;
}

/* Create and destroy many stores; the tracked live-byte total must return to
 * its baseline, validating drop order and that names/closures are freed. */
static int test_no_leak_across_lifecycle(void) {
    CHECK(run_add_once() == 0, "warmup"); /* absorb one-time allocations */
    int32_t baseline = spacewasm_memory_statistics().total_bytes;
    for (int i = 0; i < 50; i++) {
        CHECK(run_add_once() == 0, "iter %d", i);
    }
    int32_t after = spacewasm_memory_statistics().total_bytes;
    CHECK(after == baseline, "memory drifted: baseline=%d after=%d", baseline, after);
    return 0;
}

/* ---- pause/resume host callbacks ----------------------------------------- */

static spacewasm_hostcall_result_t pause_host(spacewasm_caller_t* caller, void* userdata,
                                              const spacewasm_value_t* params, size_t n,
                                              spacewasm_value_t* out) {
    (void)caller;
    (void)userdata;
    (void)params;
    (void)n;
    (void)out;
    return SPACEWASM_PAUSE;
}

static spacewasm_hostcall_result_t pause_i32_host(spacewasm_caller_t* caller, void* userdata,
                                                  const spacewasm_value_t* params, size_t n,
                                                  spacewasm_value_t* out) {
    (void)caller;
    (void)userdata;
    (void)params;
    (void)n;
    (void)out;
    return SPACEWASM_PAUSE;
}

/* ---- pause/resume tests -------------------------------------------------- */

static int test_pause_and_resume_no_value(void) {
    spacewasm_host_t host;
    CHECK(spacewasm_host_new(1, &host) == SPACEWASM_OK, "host_new");

    uint32_t hmod;
    CHECK(spacewasm_add_host_module(&host, "env", 1, 0, &hmod) == SPACEWASM_OK,
          "add_host_module");
    CHECK(spacewasm_add_host_function(&host, hmod, "pause", "", "", pause_host, NULL) ==
              SPACEWASM_OK,
          "add_host_function");

    spacewasm_t* store = NULL;
    spacewasm_compiler_options_t opts = {false, 0, 256};
    CHECK(spacewasm_new(&host, 1024, 1, opts, &store) == SPACEWASM_OK, "store_new");

    cursor_t cursor = {PAUSE_WASM, sizeof(PAUSE_WASM), 0, 0};
    spacewasm_allocator_t* alloc = spacewasm_allocator_new(mem_alloc, mem_realloc, mem_dealloc, NULL);
    CHECK(alloc != NULL, "allocator_new");
    uint32_t idx;
    CHECK(spacewasm_load_module(store, "main", cursor_read, &cursor, alloc, &idx) ==
              SPACEWASM_OK,
          "load_module");

    /* No start function */
    uint32_t start_mod = 0;
    uint32_t start_func = 0;
    CHECK(spacewasm_module_start(store, idx, &start_mod, &start_func) == SPACEWASM_ERR_NOT_FOUND,
          "no start");

    uint32_t func;
    CHECK(spacewasm_find_export_func(store, idx, "test_pause", &func) == SPACEWASM_OK,
          "find test_pause");
    CHECK(spacewasm_invoke(store, idx, func, NULL, 0) == SPACEWASM_OK, "invoke");

    /* Run until pause */
    spacewasm_trap_t trap = SPACEWASM_TRAP_NONE;
    CHECK(spacewasm_run(store, 10000, &trap) == SPACEWASM_RUN_PAUSE, "run until pause");

    /* Resume without value */
    CHECK(spacewasm_resume(store) == SPACEWASM_OK, "resume");

    /* Continue running to completion */
    while (spacewasm_run(store, 10000, &trap) == SPACEWASM_RUN_OUT_OF_FUEL)
        ;
    CHECK(trap == SPACEWASM_TRAP_NONE, "no trap");

    /* Check result */
    spacewasm_value_t out = {SPACEWASM_I32, {.i32_ = 0}};
    CHECK(spacewasm_get_result(store, SPACEWASM_I32, &out) == SPACEWASM_OK, "get_result");
    CHECK(out.u.i32_ == 42, "expected 42, got %d", out.u.i32_);

    spacewasm_allocator_destroy(alloc);
    spacewasm_destroy(store);
    return 0;
}

static int test_pause_and_resume_with_value(void) {
    spacewasm_host_t host;
    CHECK(spacewasm_host_new(1, &host) == SPACEWASM_OK, "host_new");

    uint32_t hmod;
    CHECK(spacewasm_add_host_module(&host, "env", 1, 0, &hmod) == SPACEWASM_OK,
          "add_host_module");
    CHECK(spacewasm_add_host_function(&host, hmod, "pause_i32", "", "i", pause_i32_host, NULL) ==
              SPACEWASM_OK,
          "add_host_function");

    spacewasm_t* store = NULL;
    spacewasm_compiler_options_t opts = {false, 0, 256};
    CHECK(spacewasm_new(&host, 1024, 1, opts, &store) == SPACEWASM_OK, "store_new");

    cursor_t cursor = {PAUSE_I32_WASM, sizeof(PAUSE_I32_WASM), 0, 0};
    spacewasm_allocator_t* alloc = spacewasm_allocator_new(mem_alloc, mem_realloc, mem_dealloc, NULL);
    CHECK(alloc != NULL, "allocator_new");
    uint32_t idx;
    CHECK(spacewasm_load_module(store, "main", cursor_read, &cursor, alloc, &idx) ==
              SPACEWASM_OK,
          "load_module");

    /* No start function */
    uint32_t start_mod = 0;
    uint32_t start_func = 0;
    CHECK(spacewasm_module_start(store, idx, &start_mod, &start_func) == SPACEWASM_ERR_NOT_FOUND,
          "no start");

    uint32_t func;
    CHECK(spacewasm_find_export_func(store, idx, "test_pause_i32", &func) == SPACEWASM_OK,
          "find test_pause_i32");
    CHECK(spacewasm_invoke(store, idx, func, NULL, 0) == SPACEWASM_OK, "invoke");

    /* Run until pause */
    spacewasm_trap_t trap = SPACEWASM_TRAP_NONE;
    CHECK(spacewasm_run(store, 10000, &trap) == SPACEWASM_RUN_PAUSE, "run until pause");

    /* Resume with value 99 */
    spacewasm_value_t resume_val = {SPACEWASM_I32, {.i32_ = 99}};
    CHECK(spacewasm_resume_value(store, resume_val) == SPACEWASM_OK, "resume_value");

    /* Continue running to completion */
    while (spacewasm_run(store, 10000, &trap) == SPACEWASM_RUN_OUT_OF_FUEL)
        ;
    CHECK(trap == SPACEWASM_TRAP_NONE, "no trap");

    /* Check result - should be the resumed value (99) */
    spacewasm_value_t out = {SPACEWASM_I32, {.i32_ = 0}};
    CHECK(spacewasm_get_result(store, SPACEWASM_I32, &out) == SPACEWASM_OK, "get_result");
    CHECK(out.u.i32_ == 99, "expected 99, got %d", out.u.i32_);

    spacewasm_allocator_destroy(alloc);
    spacewasm_destroy(store);
    return 0;
}

/* ---- runner -------------------------------------------------------------- */

int main(void) {
    if (spacewasm_set_global_allocator(heap_alloc, heap_dealloc, NULL) != 0) {
        fprintf(stderr, "set_global_allocator failed\n");
        return 1;
    }

    struct {
        const char* name;
        int (*fn)(void);
    } tests[] = {
        {"add_module_invoke", test_add_module_invoke},
        {"two_modules_on_one_store", test_two_modules_on_one_store},
        {"streaming_load", test_streaming_load},
        {"streaming_read_error", test_streaming_read_error},
        {"host_function_and_memory", test_host_function_and_memory},
        {"globals", test_globals},
        {"globals_all_types", test_globals_all_types},
        {"globals_imported", test_globals_imported},
        {"error_paths", test_error_paths},
        {"null_arg_handling", test_null_arg_handling},
        {"statistics_available", test_statistics_available},
        {"pause_and_resume_no_value", test_pause_and_resume_no_value},
        {"pause_and_resume_with_value", test_pause_and_resume_with_value},
        {"no_leak_across_lifecycle", test_no_leak_across_lifecycle},
    };

    int failures = 0;
    for (size_t i = 0; i < sizeof(tests) / sizeof(tests[0]); i++) {
        int rc = tests[i].fn();
        printf("%-28s %s\n", tests[i].name, rc == 0 ? "ok" : "FAILED");
        failures += rc != 0;
    }

    if (failures) {
        fprintf(stderr, "%d test(s) failed\n", failures);
        return 1;
    }
    printf("all %zu C ABI tests passed\n", sizeof(tests) / sizeof(tests[0]));
    return 0;
}
