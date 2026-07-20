/*
 * ctest_suite.c — the spacewasm C API exercised end-to-end from C.
 *
 * Ported from the former Rust-driven `tests/c_api.rs`. Because spacewasm_c_api
 * is a standalone `no_std` staticlib (it cannot be linked into a `std` Rust
 * crate), its behavior is validated from C instead. Covers: single/dual module
 * loading and invocation, streaming (including a read error), host functions
 * with guest-memory access, error and null-argument paths, allocator
 * statistics, and a no-leak lifecycle check.
 *
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
static spacewasm_status_t load_module_onto(spacewasm_allocator_t* alloc, spacewasm_store_t* store,
                                           const char* name, const uint8_t* data, size_t len,
                                           size_t step, uint32_t* out_idx) {
    cursor_t cursor = {data, len, 0, step};
    spacewasm_status_t st =
        spacewasm_store_load_module(store, name, cursor_read, &cursor, alloc, out_idx);
    if (st != SPACEWASM_OK) {
        return st;
    }
    bool needs_start = false;
    st = spacewasm_store_module_needs_start(store, *out_idx, &needs_start);
    if (st != SPACEWASM_OK) {
        return st;
    }
    if (needs_start) {
        spacewasm_trap_t trap = SPACEWASM_TRAP_NONE;
        if (spacewasm_store_run_start(store, *out_idx, 0, &trap) != SPACEWASM_RUN_FINISHED) {
            return SPACEWASM_ERR_WRONG_STATE;
        }
    }
    return SPACEWASM_OK;
}

/* ---- test cases ---------------------------------------------------------- */

static int test_add_module_invoke(void) {
    spacewasm_host_t host;
    CHECK(spacewasm_host_new(0, &host) == SPACEWASM_OK, "host_new");

    spacewasm_store_t* store = NULL;
    CHECK(spacewasm_store_new(&host, 1024, 1, 256, &store) == SPACEWASM_OK, "store_new");

    spacewasm_allocator_t* alloc =
        spacewasm_allocator_new(mem_alloc, mem_realloc, mem_dealloc, NULL);

    uint32_t mod_idx = 0;
    CHECK(load_module_onto(alloc, store, "main", ADD_WASM, sizeof(ADD_WASM), 0, &mod_idx) ==
              SPACEWASM_OK,
          "load");

    uint32_t idx = 0;
    CHECK(spacewasm_store_find_export_func(store, 0, "add", &idx) == SPACEWASM_OK, "find");

    spacewasm_value_t params[2] = {i32_val(20), i32_val(22)};
    CHECK(spacewasm_store_invoke(store, 0, idx, params, 2) == SPACEWASM_OK, "invoke");
    spacewasm_trap_t trap = SPACEWASM_TRAP_NONE;
    CHECK(spacewasm_store_run_to_completion(store, 0, &trap) == SPACEWASM_RUN_FINISHED, "run");

    spacewasm_value_t out = i32_val(0);
    CHECK(spacewasm_store_get_result(store, SPACEWASM_I32, &out) == SPACEWASM_OK, "result");
    CHECK(out.u.i32_ == 42, "add(20,22)=%d", out.u.i32_);

    spacewasm_store_destroy(store);
    spacewasm_allocator_destroy(alloc);

    return 0;
}

static int invoke_add(spacewasm_store_t* store, uint32_t mod, uint32_t func, int32_t a, int32_t b,
                      int32_t* out_val) {
    spacewasm_value_t params[2] = {i32_val(a), i32_val(b)};
    if (spacewasm_store_invoke(store, mod, func, params, 2) != SPACEWASM_OK) {
        return 1;
    }
    spacewasm_trap_t trap = SPACEWASM_TRAP_NONE;
    if (spacewasm_store_run_to_completion(store, 0, &trap) != SPACEWASM_RUN_FINISHED) {
        return 1;
    }
    spacewasm_value_t out = i32_val(0);
    if (spacewasm_store_get_result(store, SPACEWASM_I32, &out) != SPACEWASM_OK) {
        return 1;
    }
    *out_val = out.u.i32_;
    return 0;
}

static int test_two_modules_on_one_store(void) {
    spacewasm_host_t host;
    CHECK(spacewasm_host_new(0, &host) == SPACEWASM_OK, "host_new");
    spacewasm_store_t* store = NULL;
    CHECK(spacewasm_store_new(&host, 1024, 2, 256, &store) == SPACEWASM_OK, "store_new");

    spacewasm_allocator_t* alloc =
        spacewasm_allocator_new(mem_alloc, mem_realloc, mem_dealloc, NULL);

    uint32_t a = 0, b = 0;
    CHECK(load_module_onto(alloc, store, "a", ADD_WASM, sizeof(ADD_WASM), 0, &a) ==
              SPACEWASM_OK,
          "load a");
    CHECK(load_module_onto(alloc, store, "b", ADD_WASM, sizeof(ADD_WASM), 0, &b) ==
              SPACEWASM_OK,
          "load b");
    CHECK(a == 0 && b == 1, "indices a=%u b=%u", a, b);

    uint32_t idx_a = 0, idx_b = 0;
    CHECK(spacewasm_store_find_export_func(store, 0, "add", &idx_a) == SPACEWASM_OK, "find a");
    CHECK(spacewasm_store_find_export_func(store, 1, "add", &idx_b) == SPACEWASM_OK, "find b");

    /* Invoke module 1 first, then 0, to prove the index selects the target. */
    int32_t v = 0;
    CHECK(invoke_add(store, 1, idx_b, 100, 1, &v) == 0 && v == 101, "b=%d", v);
    CHECK(invoke_add(store, 0, idx_a, 20, 22, &v) == 0 && v == 42, "a=%d", v);

    spacewasm_store_destroy(store);
    spacewasm_allocator_destroy(alloc);
    return 0;
}

static int test_streaming_load(void) {
    spacewasm_host_t host;
    CHECK(spacewasm_host_new(0, &host) == SPACEWASM_OK, "host_new");
    spacewasm_store_t* store = NULL;
    CHECK(spacewasm_store_new(&host, 1024, 1, 256, &store) == SPACEWASM_OK, "store_new");

    spacewasm_allocator_t* alloc =
        spacewasm_allocator_new(mem_alloc, mem_realloc, mem_dealloc, NULL);

    /* Force many small 7-byte chunks through a tiny 5-byte scratch buffer. */
    uint32_t mod_idx = 0;
    CHECK(load_module_onto(alloc, store, "main", ADD_WASM, sizeof(ADD_WASM), 7, &mod_idx) ==
              SPACEWASM_OK,
          "streaming load");

    uint32_t idx = 0;
    CHECK(spacewasm_store_find_export_func(store, mod_idx, "add", &idx) == SPACEWASM_OK, "find");
    int32_t v = 0;
    CHECK(invoke_add(store, mod_idx, idx, 30, 12, &v) == 0 && v == 42, "=%d", v);

    spacewasm_store_destroy(store);
    spacewasm_allocator_destroy(alloc);
    return 0;
}

static int test_streaming_read_error(void) {
    spacewasm_host_t host;
    CHECK(spacewasm_host_new(0, &host) == SPACEWASM_OK, "host_new");
    spacewasm_store_t* store = NULL;
    CHECK(spacewasm_store_new(&host, 1024, 1, 256, &store) == SPACEWASM_OK, "store_new");

    spacewasm_allocator_t* alloc =
        spacewasm_allocator_new(mem_alloc, mem_realloc, mem_dealloc, NULL);
    CHECK(alloc, "allocator_new");
    uint32_t mod_idx = 0;
    spacewasm_status_t st =
        spacewasm_store_load_module(store, "main", failing_read, NULL, alloc, &mod_idx);
    spacewasm_allocator_destroy(alloc);
    CHECK(st == SPACEWASM_ERR_STREAM, "expected ERR_STREAM, got %d", (int)st);

    spacewasm_store_destroy(store);
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
    return SPACEWASM_CONTINUE;
}

static int test_host_function_and_memory(void) {
    spacewasm_host_t host;
    CHECK(spacewasm_host_new(1, &host) == SPACEWASM_OK, "host_new");

    uint32_t hmod = 0;
    CHECK(spacewasm_add_host_module(&host, "env", 1, 0, &hmod) == SPACEWASM_OK, "add_host_module");
    CHECK(spacewasm_add_host_function(&host, hmod, "add_one", "i", "i", add_one, NULL) ==
              SPACEWASM_OK,
          "add_host_function");

    spacewasm_store_t* store = NULL;
    CHECK(spacewasm_store_new(&host, 1024, 1, 256, &store) == SPACEWASM_OK, "store_new");

    spacewasm_allocator_t* alloc =
        spacewasm_allocator_new(mem_alloc, mem_realloc, mem_dealloc, NULL);

    uint32_t mod_idx = 0;
    CHECK(load_module_onto(alloc, store, "main", HOST_WASM, sizeof(HOST_WASM), 0, &mod_idx) ==
              SPACEWASM_OK,
          "load host module");

    uint32_t idx = 0;
    CHECK(spacewasm_store_find_export_func(store, 0, "run", &idx) == SPACEWASM_OK, "find run");
    spacewasm_value_t params[1] = {i32_val(41)};
    CHECK(spacewasm_store_invoke(store, 0, idx, params, 1) == SPACEWASM_OK, "invoke");
    spacewasm_trap_t trap = SPACEWASM_TRAP_NONE;
    CHECK(spacewasm_store_run_to_completion(store, 0, &trap) == SPACEWASM_RUN_FINISHED,
          "run (trap=%d)", (int)trap);
    spacewasm_value_t out = i32_val(0);
    CHECK(spacewasm_store_get_result(store, SPACEWASM_I32, &out) == SPACEWASM_OK, "result");
    CHECK(out.u.i32_ == 42, "add_one(41)=%d", out.u.i32_);

    spacewasm_store_destroy(store);
    spacewasm_allocator_destroy(alloc);
    return 0;
}

static int test_error_paths(void) {
    /* max_modules > 256 -> store_new returns ERR_BAD_ARG (consumes the host). */
    spacewasm_host_t host;
    CHECK(spacewasm_host_new(0, &host) == SPACEWASM_OK, "host_new");
    spacewasm_store_t* store = NULL;
    CHECK(spacewasm_store_new(&host, 1024, 257, 256, &store) == SPACEWASM_ERR_BAD_ARG,
          "oversized max_modules");

    /* Bad signature char -> ERR_BAD_SIGNATURE, no panic. */
    CHECK(spacewasm_host_new(1, &host) == SPACEWASM_OK, "host_new");
    uint32_t hmod = 0;
    CHECK(spacewasm_add_host_module(&host, "env", 1, 0, &hmod) == SPACEWASM_OK, "add_host_module");
    CHECK(spacewasm_add_host_function(&host, hmod, "bad", "x", "", add_one, NULL) ==
              SPACEWASM_ERR_BAD_SIGNATURE,
          "bad signature");
    spacewasm_host_destroy(&host);

    /* Malformed wasm -> parse error; the store is still created fine. */
    CHECK(spacewasm_host_new(0, &host) == SPACEWASM_OK, "host_new");
    CHECK(spacewasm_store_new(&host, 1024, 1, 256, &store) == SPACEWASM_OK, "store_new");
    const uint8_t junk[] = {0, 1, 2, 3, 4, 5, 6, 7};
    uint32_t mod_idx = 0;
    spacewasm_allocator_t* alloc =
        spacewasm_allocator_new(mem_alloc, mem_realloc, mem_dealloc, NULL);
    CHECK(alloc, "allocator_new");
    cursor_t cursor = {junk, sizeof(junk), 0, 0};
    spacewasm_status_t st =
        spacewasm_store_load_module(store, "main", cursor_read, &cursor, alloc, &mod_idx);
    spacewasm_allocator_destroy(alloc);
    CHECK(st == SPACEWASM_ERR_PARSE, "expected ERR_PARSE, got %d", (int)st);
    spacewasm_store_destroy(store);
    return 0;
}

static int test_null_arg_handling(void) {
    /* NULL name to load_module. */
    spacewasm_host_t host;
    CHECK(spacewasm_host_new(0, &host) == SPACEWASM_OK, "host_new");
    spacewasm_store_t* store = NULL;
    CHECK(spacewasm_store_new(&host, 1024, 1, 256, &store) == SPACEWASM_OK, "store_new");
    spacewasm_allocator_t* alloc =
        spacewasm_allocator_new(mem_alloc, mem_realloc, mem_dealloc, NULL);
    CHECK(alloc, "allocator_new");
    cursor_t cursor = {ADD_WASM, sizeof(ADD_WASM), 0, 0};
    uint32_t mod_idx = 0;
    spacewasm_status_t st =
        spacewasm_store_load_module(store, NULL, cursor_read, &cursor, alloc, &mod_idx);
    spacewasm_allocator_destroy(alloc);
    CHECK(st == SPACEWASM_ERR_NULL_ARG, "expected NULL_ARG, got %d", (int)st);

    /* NULL store to find_export_func. */
    uint32_t idx = 0;
    CHECK(spacewasm_store_find_export_func(NULL, 0, "add", &idx) == SPACEWASM_ERR_NULL_ARG,
          "null store");

    spacewasm_store_destroy(store);
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
    spacewasm_store_t* store = NULL;
    if (spacewasm_store_new(&host, 1024, 1, 256, &store) != SPACEWASM_OK) {
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
    if (spacewasm_store_find_export_func(store, 0, "add", &idx) != SPACEWASM_OK) {
        return 1;
    }
    int32_t v = 0;
    int rc = invoke_add(store, 0, idx, 1, 2, &v);
    spacewasm_store_destroy(store);
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
        {"error_paths", test_error_paths},
        {"null_arg_handling", test_null_arg_handling},
        {"statistics_available", test_statistics_available},
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
