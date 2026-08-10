// Portions of this file are derived from the Wasmtime project
// (https://github.com/bytecodealliance/wasmtime), licensed under
// Apache-2.0 WITH LLVM-exception. These portions have been modified for
// SpaceWasm.

//! Oracles.
//!
//! Oracles take a test case and determine whether we have a bug. For example,
//! one of the simplest oracles is to take a Wasm binary as our input test case,
//! validate and instantiate it, and (implicitly) check that no assertions
//! failed or segfaults happened.
//!
//! When an oracle finds a bug, it should report it to the fuzzing engine by
//! panicking.

use spacewasm::*;
use std::alloc::Layout;
use std::ptr::NonNull;

static ORACLE_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Log a Wasm module to the filesystem for debugging.
///
/// This is only enabled when `RUST_LOG=debug` is set.
pub fn log_wasm(wasm: &[u8]) {
    crate::init_fuzzing();

    if !log::log_enabled!(log::Level::Debug) {
        return;
    }

    let i = ORACLE_COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let name = format!("testcase{i}.wasm");
    std::fs::write(&name, wasm).ok();
    log::debug!("wrote wasm file to `{name}`");

    let wat = format!("testcase{i}.wat");
    match wasmprinter::print_bytes(wasm) {
        Ok(s) => {
            std::fs::write(&wat, s).ok();
            log::debug!("wrote wat file to `{wat}`");
        }
        Err(e) => {
            log::debug!("failed to print to wat: {e}");
            std::fs::remove_file(&wat).ok();
        }
    }
}

/// A simple byte stream implementation for fuzzing.
pub(crate) struct ByteStream {
    buffer: Option<std::vec::Vec<u8>>,
    consumed: bool,
}

impl ByteStream {
    pub(crate) fn new(data: &[u8]) -> Self {
        Self {
            buffer: Some(data.to_vec()),
            consumed: false,
        }
    }
}

impl WasmStream for ByteStream {
    fn read(&mut self) -> Result<Option<InnerVec<u8>>, u8> {
        if self.consumed {
            return Ok(None);
        }

        if let Some(ref mut vec) = self.buffer {
            self.consumed = true;
            let inner = InnerVec {
                ptr: vec.as_mut_ptr(),
                capacity: vec.len() as u32,
                len: vec.len() as u32,
            };
            Ok(Some(inner))
        } else {
            Ok(None)
        }
    }

    fn return_(&mut self, _chunk: InnerVec<u8>) {
        // Buffer is kept alive in self.buffer, so nothing to do
    }
}

/// Largest single allocation either fuzz allocator will service, in bytes.
///
/// A malformed module can decode a bogus length prefix -- a function-section
/// count, a memory or table size, and so on -- into an enormous single
/// allocation request (a `0x0FFFFFFF` function count drives a ~8.6 GiB `Vec`,
/// for example). Handing that straight to the system allocator makes the
/// process OOM/abort, which libFuzzer reports as a crash even though it is just
/// an unreasonable request, not a bug in the decoder. Bounding each request to
/// a fixed cap turns that into a clean `AllocError::OutOfMemory`, which the
/// decoder already converts into a `ValidationError`, so the fuzzer keeps
/// hunting for real defects instead of tripping over absurd sizes.
///
/// This models a real embedding, which always drives the decoder through a
/// bounded allocator -- fixed, measured memory is the whole point of running
/// Wasm on-board. Cumulative growth across many allocations is left to
/// libFuzzer's own `-rss_limit_mb` guard.
const MAX_ALLOCATION_BYTES: usize = 1024 * 1024 * 64; // 64 MiB

/// The global allocator for fuzzing, backing the decoder's internal bookkeeping
/// (module tables, `Vec`s of parsed entries, ...) via `global_allocator!`.
///
/// A thin passthrough to the system allocator whose only policy is the shared
/// per-allocation cap ([`MAX_ALLOCATION_BYTES`]).
pub(crate) struct SystemAllocator;

unsafe impl Allocator for SystemAllocator {
    unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
        if layout.size() > MAX_ALLOCATION_BYTES {
            return Err(AllocError::OutOfMemory);
        }
        unsafe { Ok(std::alloc::alloc(layout)) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { std::alloc::dealloc(ptr, layout) }
    }

    fn memory_statistics(&self) -> MemoryStatistics {
        MemoryStatistics {
            total_bytes: 0,
            pad_bytes: 0,
        }
    }
}

/// A model of the production `PageAllocator` for fuzzing, backing guest memory.
///
/// Like the `RustSystemAllocator` used by the integration tests, this is a thin
/// passthrough to the system allocator with no running-total bookkeeping. Its
/// only policy is the same per-allocation size cap ([`MAX_ALLOCATION_BYTES`])
/// applied by [`SystemAllocator`], so a single unreasonable request from a
/// corrupted module is rejected rather than aborting the fuzzer.
pub(crate) struct FuzzAllocator;

impl WasmMemoryAllocator for FuzzAllocator {
    fn allocate(&self, layout: Layout) -> Result<NonNull<u8>, AllocError> {
        if layout.size() > MAX_ALLOCATION_BYTES {
            return Err(AllocError::OutOfMemory);
        }
        unsafe { NonNull::new(std::alloc::alloc(layout)).ok_or(AllocError::AllocationFailed) }
    }

    fn reallocate(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        layout: Layout,
    ) -> Result<NonNull<u8>, AllocError> {
        if layout.size() > MAX_ALLOCATION_BYTES {
            return Err(AllocError::OutOfMemory);
        }
        unsafe {
            NonNull::new(std::alloc::realloc(ptr.as_ptr(), old_layout, layout.size()))
                .ok_or(AllocError::AllocationFailed)
        }
    }

    fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        unsafe { std::alloc::dealloc(ptr.as_ptr(), layout) }
    }
}

// Set up the global allocator for fuzzing
#[allow(missing_docs)]
mod fuzz_alloc {
    use super::SystemAllocator;
    spacewasm::global_allocator!(SystemAllocator, SystemAllocator);
}

const MAX_CODE_PAGES: u32 = 128;
const MAX_CONTROL_FRAMES: usize = 128;
const MAX_STACK_DEPTH: usize = 256;

/// Decode and validate a Wasm module, discarding the outcome.
///
/// Both `Ok` and `Err` are acceptable results: a graceful decode error is a
/// correct rejection, not a bug. The only failures this surfaces are *implicit*
/// -- a `panic!` (including the `strict-assertions` checks), an abort, a
/// segfault, or a sanitizer trip during `Module::new`. libFuzzer treats all of
/// those as crashes.
///
/// Returns `true` if decoding succeeded, for callers that want to log it.
fn load_module(wasm: &[u8]) -> bool {
    let mut store = match Store::from_host_modules(256, Vec::zero()) {
        Ok(s) => s,
        Err(e) => {
            log::debug!("store creation failed: {e:?}");
            return false;
        }
    };

    let mut code_builder = CodeBuilder::new(CompilerOptions {
        allow_memory_grow: true,
        max_backpatch_iterations: 0,
        max_code_pages: MAX_CODE_PAGES,
    })
    .unwrap();
    let mut stream = ByteStream::new(wasm);

    let allocator = spacewasm::Rc::new(FuzzAllocator)
        .unwrap()
        .into_wasm_memory_allocator();

    // Attempt to decode and validate the module.
    let result = Module::new::<MAX_CONTROL_FRAMES, MAX_STACK_DEPTH>(
        "",
        &mut stream,
        &mut store,
        &mut code_builder,
        allocator.clone(),
    );

    result.is_ok()
}

/// Oracle: Validate a Wasm module.
///
/// This tests the decoder by attempting to validate the given Wasm bytes.
/// It checks that the module is structurally valid according to Wasm spec.
///
/// Inputs come from wasm-smith and are therefore expected to be valid, so a
/// decode error here points at SpaceWasm being stricter than the generator
/// (e.g. code-page or stack limits) rather than at malformed input.
pub fn validate(wasm: &[u8]) {
    log_wasm(wasm);

    if load_module(wasm) {
        log::debug!("validation succeeded");
    } else {
        log::debug!("validation failed (expected for invalid modules)");
    }
}

/// Oracle: Decode a possibly-malformed Wasm module.
///
/// This tests the decoder against intentionally corrupted input (see
/// [`crate::generators::MalformedModule`]). A clean `Err` is the correct
/// outcome for garbage input; the bug we are hunting is a *reachable panic* --
/// an out-of-bounds read, an unchecked index, an arithmetic overflow, or a
/// `strict-assertions` violation -- on the load path. The decoder must reject
/// bad modules gracefully, never crash on them.
pub fn decode(wasm: &[u8]) {
    log_wasm(wasm);

    if load_module(wasm) {
        log::debug!("decode succeeded (mutation happened to stay valid)");
    } else {
        log::debug!("decode rejected malformed module (expected)");
    }
}

/// Oracle: Execute module that should not trap.
///
/// This tests modules generated with disallow_traps configuration.
/// Such modules should never trap during execution - if they do, it's a bug
/// in either the generator or the interpreter.
///
/// This oracle uses execution tracing to record pc/sp/fp history,
/// which is dumped on panic for debugging.
pub fn no_traps(wasm: &[u8]) {
    log_wasm(wasm);

    // Create engine with reduced store size for better parallel fuzzing
    let mut state = match Engine::new(512, 16, Vec::zero()) {
        Ok(s) => s,
        Err(e) => {
            log::debug!("engine creation failed: {e:?}");
            return;
        }
    };

    // Compile module with reduced code pages
    let mut code_builder = CodeBuilder::new(CompilerOptions {
        allow_memory_grow: true,
        max_backpatch_iterations: 0,
        max_code_pages: MAX_CODE_PAGES,
    })
    .unwrap();
    let mut stream = ByteStream::new(wasm);

    let allocator = Rc::new(FuzzAllocator)
        .unwrap()
        .into_wasm_memory_allocator();

    let module = match Module::new::<MAX_CONTROL_FRAMES, MAX_STACK_DEPTH>(
        "",
        &mut stream,
        &mut state.store,
        &mut code_builder,
        allocator.clone(),
    ) {
        Ok(m) => m,
        Err(e) => {
            log::debug!("compilation failed: {e:?}");
            return;
        }
    };

    log::debug!("module compiled successfully");

    // Borrow the compiled text straight from the builder (no copy needed).
    let text = code_builder.pages();

    let module_ref = state.push_module(module).unwrap();
    let start_result = match state.invoke_start(module_ref) {
        StartInvocation::Finished => InterpreterResult::Finished,
        StartInvocation::Trap(t) => InterpreterResult::Trap(t),
        StartInvocation::Pause => InterpreterResult::Pause,
        StartInvocation::Running => Interpreter.run(text, &mut state, 10000),
    };
    match start_result {
        InterpreterResult::Finished => {}
        InterpreterResult::OutOfFuel => {
            log::debug!("start routine out of fuel");
            return;
        }
        InterpreterResult::Trap(TrapReason::StackOverflow) => {
            // Wasm Smith cannot avoid this. Also this is not a bug so it's ok to drop this run
            log::debug!("module hit a stack overflow during initialization");
            return;
        }
        InterpreterResult::Trap(trap_reason) => {
            panic!("Trap during initialization: {trap_reason:?}")
        }
        InterpreterResult::Pause => panic!("Host init pause"),
    }

    log::debug!("module instantiated");

    // Get the last module index and collect exported function refs with their signatures
    let module_idx = state.store.modules().len().saturating_sub(1);

    let exported_funcs: std::vec::Vec<(WasmRef, std::vec::Vec<Value>)> = {
        let Some(module) = state.store.modules().get(module_idx) else {
            log::debug!("failed to get module");
            return;
        };

        module
            .exports
            .iter()
            .filter_map(|export| {
                if let ExportDesc::Func(func_idx) = export.desc {
                    // Get the function reference which handles import resolution
                    let func_ref = module.get_func_ref(func_idx)?;

                    // Look up the function type based on the resolved reference
                    let func_type = match func_ref {
                        Ref::Module(index) => {
                            // Local function in this module
                            let func = module.functions.get(index as usize)?;
                            module.types.get(func.ty.0 as usize)?
                        }
                        Ref::Extern {
                            module: mod_ref,
                            index,
                        } => {
                            // Function from another Wasm module
                            let other_module = state.store.modules().get(mod_ref.0 as usize)?;
                            let func = other_module.functions.get(index as usize)?;
                            other_module.types.get(func.ty.0 as usize)?
                        }
                        Ref::Host { .. } => {
                            // Host function - skip these for now since they have different handling
                            return None;
                        }
                    };

                    // Convert func_ref to WasmRef
                    let wasm_ref = match func_ref {
                        Ref::Module(index) => WasmRef {
                            module: ModuleRef(module_idx as u8),
                            index,
                        },
                        Ref::Extern { module, index } => WasmRef { module, index },
                        _ => return None,
                    };

                    // Generate default parameters based on the function signature
                    let params: std::vec::Vec<Value> = func_type
                        .params
                        .iter()
                        .map(|val_type| match val_type {
                            ValType::I32 => Value::I32(0),
                            ValType::I64 => Value::I64(0),
                            ValType::F32 => Value::F32(0.0),
                            ValType::F64 => Value::F64(0.0),
                        })
                        .collect();

                    Some((wasm_ref, params))
                } else {
                    None
                }
            })
            .collect()
    };

    // Try to invoke each exported function
    // These should never trap since the module was generated with disallow_traps
    for (wasm_ref, params) in exported_funcs {
        state.reset();
        state.invoke(wasm_ref, &params).unwrap();

        // Run the interpreter with limited instructions
        let interpreter = Interpreter;
        let result = interpreter.run(text, &mut state, 10000);

        // Check for traps - this is the key assertion
        match result {
            InterpreterResult::OutOfFuel => {
                log::debug!("ran out of fuel (instruction limit reached)");
            }
            InterpreterResult::Finished => {
                log::debug!("execution completed without traps");
            }
            InterpreterResult::Trap(TrapReason::StackOverflow) => {
                log::debug!("execution completed with stack overflow");
            }
            InterpreterResult::Trap(reason) => {
                // A trap in a no_traps module is a bug!
                panic!("unexpected trap in no_traps module: {reason:?}");
            }
            InterpreterResult::Pause => {
                panic!("interpreter paused by host function")
            }
        }
    }
}
