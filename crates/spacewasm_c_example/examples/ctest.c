/*
 * ctest.c — minimal C program exercising the spacewasm C API end-to-end.
 *
 * Loads a tiny module exporting `add(i32, i32) -> i32` by streaming it through
 * a read callback, invokes it, and checks the result. Built and run by
 * tests/c_abi.rs against the staticlib + header.
 */
#include "spacewasm.h"

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/*
 * Panic hook the interpreter calls on a fatal internal error. Must not return.
 * A real integrator would log and reset/halt; here we print and abort. All
 * strings are UTF-8 and not NUL-terminated, so print with an explicit length.
 */
void spacewasm_panic(const uint8_t *filename, size_t filename_len, uint32_t line,
                     const uint8_t *msg, size_t len) {
    fprintf(stderr, "spacewasm panic at %.*s:%u: %.*s\n", (int)filename_len,
            (const char *)filename, line, (int)len, (const char *)msg);
    abort();
}

/*
 * Process-wide heap allocator, backing the interpreter's internal Rust
 * allocations. spacewasm_c_api wraps these in a page allocator, so they run only
 * for large page-sized blocks — not per small allocation. `align` is honored via
 * aligned_alloc (size is rounded up to a multiple of align, as C requires).
 */
static uint8_t *heap_alloc(void *userdata, size_t size, size_t align) {
    (void)userdata;
    if (size == 0) {
        return NULL;
    }
    if (align < sizeof(void *)) {
        align = sizeof(void *);
    }
    size_t rounded = (size + align - 1) & ~(align - 1);
    return (uint8_t *)aligned_alloc(align, rounded);
}

static void heap_dealloc(void *userdata, uint8_t *ptr, size_t size, size_t align) {
    (void)userdata;
    (void)size;
    (void)align;
    free(ptr);
}

/* (module (func (export "add") (param i32 i32) (result i32)
 *    local.get 0 local.get 1 i32.add)) */
static const uint8_t ADD_WASM[] = {
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x07, 0x01, 0x60,
    0x02, 0x7f, 0x7f, 0x01, 0x7f, 0x03, 0x02, 0x01, 0x00, 0x07, 0x07, 0x01,
    0x03, 0x61, 0x64, 0x64, 0x00, 0x00, 0x0a, 0x09, 0x01, 0x07, 0x00, 0x20,
    0x00, 0x20, 0x01, 0x6a, 0x0b};

/*
 * Guest linear-memory allocator callbacks backed by the C standard library.
 * `align` is ignored: malloc returns memory aligned for any standard type
 * (max_align_t, >= 16 bytes), which satisfies the default 64 KiB page alignment
 * (16). A real integrator honoring larger alignments would use aligned_alloc.
 */
static uint8_t *mem_alloc(void *userdata, size_t size, size_t align) {
    (void)userdata;
    (void)align;
    return (uint8_t *)malloc(size);
}

static uint8_t *mem_realloc(void *userdata, uint8_t *ptr, size_t old_size,
                            size_t new_size, size_t align) {
    (void)userdata;
    (void)old_size;
    (void)align;
    return (uint8_t *)realloc(ptr, new_size);
}

static void mem_dealloc(void *userdata, uint8_t *ptr, size_t size, size_t align) {
    (void)userdata;
    (void)size;
    (void)align;
    free(ptr);
}

/* A simple cursor over ADD_WASM used as the streaming read source. The callback
 * owns the buffer: it hands back a pointer into ADD_WASM and its length. Here
 * `step` bytes are handed out per call to exercise multi-chunk streaming. */
typedef struct {
    const uint8_t *data;
    size_t len;
    size_t pos;
    size_t step;
} cursor_t;

static spacewasm_read_result_t cursor_read(void *userdata, const uint8_t **out_buf,
                                            size_t *out_len) {
    cursor_t *c = (cursor_t *)userdata;
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

int main(void) {
    /* Install the heap allocator before any allocating API call. */
    if (spacewasm_set_global_allocator(heap_alloc, heap_dealloc, NULL) != 0) {
        fprintf(stderr, "set_global_allocator failed\n");
        return 1;
    }

    /* No host modules for this example; create an empty host vector. */
    spacewasm_host_t host;
    spacewasm_status_t st = spacewasm_host_new(0, &host);
    if (st != SPACEWASM_OK) {
        fprintf(stderr, "host_new: status=%d\n", (int)st);
        return 1;
    }

    /* Finish the host vector into a store (1024-byte guest stack, room for 1
     * guest module, 256 code pages). This consumes `host`. */
    spacewasm_store_t *store = NULL;
    st = spacewasm_store_new(&host, 1024, 1, 256, &store);
    if (st != SPACEWASM_OK) {
        fprintf(stderr, "store_new: status=%d\n", (int)st);
        return 1;
    }

    /* Build a guest linear-memory allocator from the malloc-backed callbacks. */
    spacewasm_allocator_t *alloc =
        spacewasm_allocator_new(mem_alloc, mem_realloc, mem_dealloc, NULL);
    if (!alloc) {
        fprintf(stderr, "allocator_new failed\n");
        return 1;
    }

    /* Load a guest module onto the store, streamed in 16-byte chunks. */
    cursor_t cursor = {ADD_WASM, sizeof(ADD_WASM), 0, 16};
    uint32_t mod_idx = 0;
    st = spacewasm_store_load_module(store, "main", cursor_read, &cursor, alloc,
                                     &mod_idx);
    /* The loaded module holds its own reference; the handle can go now. */
    spacewasm_allocator_destroy(alloc);
    if (st != SPACEWASM_OK) {
        fprintf(stderr, "load_module: status=%d\n", (int)st);
        return 1;
    }

    /* Run the module's start function if it declares one. This module does not,
     * but a well-behaved loader always checks. */
    bool needs_start = false;
    st = spacewasm_store_module_needs_start(store, mod_idx, &needs_start);
    if (st != SPACEWASM_OK) {
        fprintf(stderr, "module_needs_start: status=%d\n", (int)st);
        return 1;
    }
    if (needs_start) {
        spacewasm_trap_t start_trap = SPACEWASM_TRAP_NONE;
        spacewasm_run_status_t start_rs =
            spacewasm_store_run_start(store, mod_idx, /*fuel=*/0, &start_trap);
        if (start_rs != SPACEWASM_RUN_FINISHED) {
            fprintf(stderr, "run_start: status=%d trap=%d\n", (int)start_rs,
                    (int)start_trap);
            return 1;
        }
    }

    uint32_t idx = 0;
    st = spacewasm_store_find_export_func(store, mod_idx, "add", &idx);
    if (st != SPACEWASM_OK) {
        fprintf(stderr, "find_export: status=%d\n", (int)st);
        return 1;
    }

    spacewasm_value_t params[2];
    params[0].tag = SPACEWASM_I32;
    params[0].u.i32_ = 20;
    params[1].tag = SPACEWASM_I32;
    params[1].u.i32_ = 22;

    st = spacewasm_store_invoke(store, mod_idx, idx, params, 2);
    if (st != SPACEWASM_OK) {
        fprintf(stderr, "invoke: status=%d\n", (int)st);
        return 1;
    }

    spacewasm_trap_t trap = SPACEWASM_TRAP_NONE;
    spacewasm_run_status_t rs = spacewasm_store_run_to_completion(store, 0, &trap);
    if (rs != SPACEWASM_RUN_FINISHED) {
        fprintf(stderr, "run: status=%d trap=%d\n", (int)rs, (int)trap);
        return 1;
    }

    spacewasm_value_t out;
    st = spacewasm_store_get_result(store, SPACEWASM_I32, &out);
    if (st != SPACEWASM_OK) {
        fprintf(stderr, "get_result: status=%d\n", (int)st);
        return 1;
    }

    spacewasm_store_destroy(store);

    if (out.u.i32_ != 42) {
        fprintf(stderr, "wrong result: %d\n", out.u.i32_);
        return 1;
    }

    printf("add(20, 22) = %d\n", out.u.i32_);
    return 0;
}
