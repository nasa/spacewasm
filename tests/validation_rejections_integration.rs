//! Integration tests for the SpaceWasm-specific validation rejections that are
//! driven by [`CompilerOptions`] and therefore *cannot* be expressed as a
//! `.wast` file.

use core::alloc::Layout;
use core::ptr::NonNull;

use spacewasm::{
    AllocError, Allocator, CodeBuilder, CompilerOptions, Engine, InnerVec, Module, ValidationError,
    WasmMemoryAllocator, WasmStream, global_allocator,
};

extern crate std;

// `CodeBuilder::new` allocates its code pages through the global allocator on
// the success path, so this test binary must provide the `__spacewasm_*`
// symbols. Back them with the system heap.
struct SystemAllocator;

unsafe impl Allocator for SystemAllocator {
    unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
        let ptr = unsafe { std::alloc::alloc(layout) };
        if ptr.is_null() {
            Err(AllocError::AllocationFailed)
        } else {
            Ok(ptr)
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { std::alloc::dealloc(ptr, layout) }
    }
}

global_allocator!(SystemAllocator, SystemAllocator);

#[test]
fn code_builder_rejects_oversized_max_code_pages() {
    // `1 << 24` is the first value that does not fit the 24-bit code-page index;
    // `CodeBuilder::new` must return an allocation error rather than panic.
    let opts = CompilerOptions {
        allow_memory_grow: false,
        max_backpatch_iterations: None,
        max_code_pages: 1 << 24,
    };
    assert!(matches!(
        CodeBuilder::new(opts),
        Err(AllocError::AllocationFailed)
    ));

    // One below the limit still builds successfully, pinning the boundary.
    let opts_ok = CompilerOptions {
        allow_memory_grow: false,
        max_backpatch_iterations: None,
        max_code_pages: (1 << 24) - 1,
    };
    assert!(CodeBuilder::new(opts_ok).is_ok());
}

const MAX_CONTROL_FRAMES: usize = 128;
const MAX_STACK_DEPTH: usize = 256;

/// Single-shot [`WasmStream`] that hands its whole buffer over once, then
/// reports EOF.
struct ByteStream {
    buffer: Vec<u8>,
    consumed: bool,
}

impl ByteStream {
    fn new(data: &[u8]) -> Self {
        Self {
            buffer: data.to_vec(),
            consumed: false,
        }
    }
}

impl WasmStream for ByteStream {
    fn read(&mut self) -> Result<Option<InnerVec<u8>>, u8> {
        if self.consumed {
            return Ok(None);
        }
        self.consumed = true;
        Ok(Some(unsafe {
            InnerVec::from_raw_parts(
                self.buffer.as_mut_ptr(),
                self.buffer.len() as u32,
                self.buffer.len() as u32,
            )
        }))
    }

    fn return_(&mut self, _chunk: InnerVec<u8>) {}
}

/// Backs guest linear memory with the system heap (mirrors the
/// `RustSystemAllocator` in `tests/util/spectest.rs`). Needed because a module
/// declaring `(memory ...)` allocates its pages during `Module::new`.
struct GuestMemoryAllocator;

impl WasmMemoryAllocator for GuestMemoryAllocator {
    fn allocate(&self, layout: Layout) -> Result<NonNull<u8>, AllocError> {
        unsafe { NonNull::new(std::alloc::alloc(layout)).ok_or(AllocError::AllocationFailed) }
    }

    fn reallocate(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        layout: Layout,
    ) -> Result<NonNull<u8>, AllocError> {
        unsafe {
            NonNull::new(std::alloc::realloc(ptr.as_ptr(), old_layout, layout.size()))
                .ok_or(AllocError::AllocationFailed)
        }
    }

    fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        unsafe { std::alloc::dealloc(ptr.as_ptr(), layout) }
    }
}

/// Decode + validate `wasm` under `options`, returning the innermost
/// [`ValidationError`] on rejection (or `Ok(())` if the module compiles). The
/// backpatch/`memory.grow` checks run inside `Module::new`, so the module never
/// needs to be pushed or executed.
fn compile(wasm: &[u8], options: CompilerOptions) -> Result<(), ValidationError> {
    let mut engine = Engine::new(1024, 256, spacewasm::vec![]).unwrap();
    let mut code_builder = CodeBuilder::new(options).unwrap();
    let mut stream = ByteStream::new(wasm);

    Module::new::<MAX_CONTROL_FRAMES, MAX_STACK_DEPTH>(
        "",
        &mut stream,
        &mut engine.store,
        &mut code_builder,
        spacewasm::Rc::new(GuestMemoryAllocator)
            .unwrap()
            .into_wasm_memory_allocator(),
    )
    // `ParseError.err` is a `SectionDecodeError`, whose `.err` is the
    // `ValidationError` we care about.
    .map(|_| ())
    .map_err(|e| e.err.err)
}

/// With `allow_memory_grow = false`, a module containing `memory.grow`
/// must be rejected with `IllegalMemoryGrow`; with the flag enabled the same
/// bytes compile. The harness (`.wast`) fixes the flag to `true`, so this is
/// the only place the rejection can be exercised.
#[test]
fn memory_grow_rejected_when_option_disabled() {
    // (module (memory 1) (func (drop (memory.grow (i32.const 0)))))
    // Generated with `wat2wasm` (WABT 1.0.41).
    const WASM: &[u8] = &[
        0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, // magic + version 1
        0x01, 0x04, 0x01, 0x60, 0x00, 0x00, // type section: one () -> ()
        0x03, 0x02, 0x01, 0x00, // function section: func 0 : type 0
        0x05, 0x03, 0x01, 0x00, 0x01, // memory section: 1 memory, min 1, no max
        0x0A, 0x09, 0x01, 0x07, 0x00, // code section: 1 body, size 7, 0 locals
        0x41, 0x00, // i32.const 0
        0x40, 0x00, // memory.grow (reserved memindex 0x00)
        0x1A, // drop
        0x0B, // end
    ];

    let disabled = CompilerOptions {
        allow_memory_grow: false,
        max_backpatch_iterations: None,
        max_code_pages: 256,
    };
    assert_eq!(
        compile(WASM, disabled),
        Err(ValidationError::IllegalMemoryGrow),
    );

    // Boundary: the identical bytes compile once the flag is enabled, proving
    // the rejection is caused solely by the option, not by a malformed module.
    let enabled = CompilerOptions {
        allow_memory_grow: true,
        ..disabled
    };
    assert!(compile(WASM, enabled).is_ok());
}

/// Must reject a module whose forward-branch backpatch chain is longer than the limit, with
/// `PossibleBackpatchCycle` — even though the module contains no real cycle.
/// With `max_backpatch_iterations = None` (unlimited, the harness default) the
/// same module compiles.
#[test]
fn backpatch_chain_exceeding_limit_is_rejected() {
    // (module (func (block (i32.const 0) (br_if 0)  ;; x4 )))
    // Four `br_if 0` to the same block build a forward-jump chain of length 4;
    // resolving it at the block `end` walks all four nodes. Generated with
    // `wat2wasm` (WABT 1.0.41).
    const WASM: &[u8] = &[
        0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, // magic + version 1
        0x01, 0x04, 0x01, 0x60, 0x00, 0x00, // type section: one () -> ()
        0x03, 0x02, 0x01, 0x00, // function section: func 0 : type 0
        0x0A, 0x17, 0x01, 0x15, 0x00, // code section: 1 body, size 21, 0 locals
        0x02, 0x40, // block (empty result type)
        0x41, 0x00, 0x0D, 0x00, // i32.const 0 ; br_if 0
        0x41, 0x00, 0x0D, 0x00, // i32.const 0 ; br_if 0
        0x41, 0x00, 0x0D, 0x00, // i32.const 0 ; br_if 0
        0x41, 0x00, 0x0D, 0x00, // i32.const 0 ; br_if 0
        0x0B, // end (block)
        0x0B, // end (func)
    ];

    // A chain of 4 comfortably exceeds a limit of 1.
    let limited = CompilerOptions {
        allow_memory_grow: false,
        max_backpatch_iterations: Some(1),
        max_code_pages: 256,
    };
    assert_eq!(
        compile(WASM, limited),
        Err(ValidationError::PossibleBackpatchCycle),
    );

    // Boundary: `None` disables the limit, so the identical (cycle-free) module
    // compiles — proving the rejection comes from the limit, not the bytes.
    let unlimited = CompilerOptions {
        max_backpatch_iterations: None,
        ..limited
    };
    assert!(compile(WASM, unlimited).is_ok());
}
