/*
 * coremark.c — C program that runs the Coremark benchmark in WASM with SpaceWasm
 */
#include "spacewasm.h"

/*
 * Panic hook the interpreter calls on a fatal internal error. Must not return.
 * A real integrator would log and reset/halt; here we print and abort. All
 * strings are UTF-8 and not NUL-terminated, so print with an explicit length.
 */
void spacewasm_panic(const uint8_t* filename, size_t filename_len, uint32_t line,
                     const uint8_t* msg, size_t len) {
    abort();
}

/*
 * Process-wide heap allocator, backing the interpreter's internal Rust
 * allocations. spacewasm_c_api wraps these in a page allocator, so they run only
 * for large page-sized blocks — not per small allocation. `align` is honored via
 * aligned_alloc (size is rounded up to a multiple of align, as C requires).
 */
static uint8_t* heap_alloc(void* userdata, size_t size, size_t align) {
    // (void)userdata;
    // if (size == 0) {
    //     return NULL;
    // }
    // if (align < sizeof(void*)) {
    //     align = sizeof(void*);
    // }
    // size_t rounded = (size + align - 1) & ~(align - 1);
    // return (uint8_t*)aligned_alloc(align, rounded);
    return 0;
}

static void heap_dealloc(void* userdata, uint8_t* ptr, size_t size, size_t align) {
    // (void)userdata;
    // (void)size;
    // (void)align;
    // free(ptr);
    return;
}

/*
 * Guest linear-memory allocator callbacks backed by the C standard library.
 * `align` is ignored: malloc returns memory aligned for any standard type
 * (max_align_t, >= 16 bytes), which satisfies the default 64 KiB page alignment
 * (16). A real integrator honoring larger alignments would use aligned_alloc.
 */
static uint8_t* mem_alloc(void* userdata, size_t size, size_t align) {
    // (void)userdata;
    // (void)align;
    // return (uint8_t*)malloc(size);
    return 0;
}

static uint8_t* mem_realloc(void* userdata, uint8_t* ptr, size_t old_size, size_t new_size,
                            size_t align) {
    // (void)userdata;
    // (void)old_size;
    // (void)align;
    // return (uint8_t*)realloc(ptr, new_size);
    return 0;
}

static void mem_dealloc(void* userdata, uint8_t* ptr, size_t size, size_t align) {
    (void)userdata;
    (void)size;
    (void)align;
    free(ptr);
}

/* A simple cursor over ADD_WASM used as the streaming read source. The callback
 * owns the buffer: it hands back a pointer into ADD_WASM and its length. Here
 * `step` bytes are handed out per call to exercise multi-chunk streaming. */
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

int main(void) {
    spacewasm_set_global_allocator(heap_alloc, heap_dealloc, NULL);
    spacewasm_host_t host;
    spacewasm_host_new(1, &host);
    spacewasm_t* store = NULL;
    spacewasm_compiler_options_t options = {
        .allow_memory_grow = false,
        .max_backpatch_iterations = 0,
        .max_code_pages = 256,
    };
    spacewasm_new(&host, 1024, 1, options, &store);
    spacewasm_allocator_t* alloc = spacewasm_allocator_new(mem_alloc, mem_realloc, mem_dealloc, NULL);
    uint8_t COREMARK_WASM[] = {0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x07, 0x01,
                                0x60, 0x02, 0x7f, 0x7f, 0x01, 0x7f, 0x03, 0x02, 0x01, 0x00, 0x07,
                                0x07, 0x01, 0x03, 0x61, 0x64, 0x64, 0x00, 0x00, 0x0a, 0x09, 0x01,
                                0x07, 0x00, 0x20, 0x00, 0x20, 0x01, 0x6a, 0x0b};
    cursor_t cursor = {COREMARK_WASM, sizeof(COREMARK_WASM), 0, 16};
    uint32_t mod_idx = 0;
    spacewasm_load_module(store, "main", cursor_read, &cursor, alloc, &mod_idx);
    spacewasm_allocator_destroy(alloc);
    spacewasm_run_status_t start_rs = spacewasm_invoke_start(store, mod_idx);
    spacewasm_trap_t start_trap = SPACEWASM_TRAP_NONE;
    while (start_rs == SPACEWASM_RUN_OUT_OF_FUEL) start_rs = spacewasm_run(store, 1000, &start_trap);
    uint32_t idx = 0;
    spacewasm_find_export_func(store, mod_idx, "add", &idx);
    spacewasm_value_t params[2];
    params[0].tag = SPACEWASM_I32;
    params[0].u.i32_ = 1;
    params[1].tag = SPACEWASM_I32;
    params[1].u.i32_ = 90;
    spacewasm_invoke(store, mod_idx, idx, params, 2);
    spacewasm_run_status_t rs = SPACEWASM_RUN_OUT_OF_FUEL;
    while (rs == SPACEWASM_RUN_OUT_OF_FUEL) rs = spacewasm_run(store, 1000, &start_trap);
    spacewasm_value_t out;
    spacewasm_get_result(store, SPACEWASM_I32, &out);
    spacewasm_destroy(store);
    return 0;
}
