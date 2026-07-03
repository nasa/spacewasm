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
use std::cell::RefCell;
use std::ptr::NonNull;

static ORACLE_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Log a WASM module to the filesystem for debugging.
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
    fn read(&mut self) -> Result<Option<InnerVec<u8>>, ReaderError> {
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

pub(crate) struct SystemAllocator;

unsafe impl Allocator for SystemAllocator {
    unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
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

pub(crate) struct FuzzAllocator {
    allocated: RefCell<usize>,
    limit: usize,
}

impl WasmMemoryAllocator for FuzzAllocator {
    fn allocate(&self, layout: Layout) -> Result<NonNull<u8>, AllocError> {
        let new_size = *self.allocated.borrow() + layout.size();
        if new_size <= self.limit {
            *(self.allocated.borrow_mut()) = new_size;
            unsafe {
                Ok(NonNull::new(std::alloc::alloc(layout)).ok_or(AllocError::AllocationFailed)?)
            }
        } else {
            Err(AllocError::OutOfMemory)
        }
    }

    fn reallocate(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        layout: Layout,
    ) -> Result<NonNull<u8>, AllocError> {
        let new_size = *self.allocated.borrow() - old_layout.size() + layout.size();
        if new_size <= self.limit {
            *(self.allocated.borrow_mut()) = new_size;
            unsafe {
                Ok(
                    NonNull::new(std::alloc::realloc(ptr.as_ptr(), old_layout, layout.size()))
                        .ok_or(AllocError::AllocationFailed)?,
                )
            }
        } else {
            Err(AllocError::OutOfMemory)
        }
    }

    fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        *self.allocated.borrow_mut() -= layout.size();
        unsafe { std::alloc::dealloc(ptr.as_ptr(), layout) }
    }
}

// Set up the global allocator for fuzzing
#[allow(missing_docs)]
mod fuzz_alloc {
    use super::SystemAllocator;
    spacewasm::global_allocator!(SystemAllocator, SystemAllocator);
}

/// Oracle: Validate a WASM module.
///
/// This tests the decoder by attempting to validate the given WASM bytes.
/// It checks that the module is structurally valid according to WASM spec.
pub fn validate(wasm: &[u8]) {
    log_wasm(wasm);

    let mut store = match Store::new(256, []) {
        Ok(s) => s,
        Err(e) => {
            log::debug!("store creation failed: {e:?}");
            return;
        }
    };

    let mut code_builder = CodeBuilder::<256>::default();
    let mut stream = ByteStream::new(wasm);

    let allocator = spacewasm::Rc::new(FuzzAllocator {
        limit: 1024 * 1024 * 64, // 64MiB,
        allocated: RefCell::new(0),
    })
    .unwrap()
    .into_wasm_memory_allocator();

    // Attempt to decode and validate the module
    let result = Module::new::<256>(
        "",
        &mut stream,
        &mut store,
        &mut code_builder,
        allocator.clone(),
        CompilerOptions {
            allow_memory_grow: true,
        },
    );

    match result {
        Ok(_) => {
            log::debug!("validation succeeded");
        }
        Err(e) => {
            log::debug!("validation failed (expected for invalid modules): {e:?}");
        }
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

    // Create store with reduced size for better parallel fuzzing
    let mut store = match Store::new(16, []) {
        Ok(s) => s,
        Err(e) => {
            log::debug!("store creation failed: {e:?}");
            return;
        }
    };

    // Compile module with reduced code pages
    let mut code_builder = CodeBuilder::<128>::default();
    let mut stream = ByteStream::new(wasm);

    let allocator = Rc::new(FuzzAllocator {
        limit: 1024 * 1024 * 64, // 64MiB,
        allocated: RefCell::new(0),
    })
    .unwrap()
    .into_wasm_memory_allocator();

    let module = match Module::new::<128>(
        "",
        &mut stream,
        &mut store,
        &mut code_builder,
        allocator.clone(),
        CompilerOptions {
            allow_memory_grow: true,
        },
    ) {
        Ok(m) => m,
        Err(e) => {
            log::debug!("compilation failed: {e:?}");
            return;
        }
    };

    log::debug!("module compiled successfully");

    // Finish compilation to get the compiled text
    let Ok((text, _)) = code_builder.finish() else {
        log::debug!("code builder finish failed");
        return;
    };

    // Instantiate
    let mut state = match store.allocate(512) {
        Ok(s) => s,
        Err(e) => {
            log::debug!("state allocation failed: {e:?}");
            return;
        }
    };

    let Ok(module_box) = Box::new(module) else {
        log::debug!("module box creation failed");
        return;
    };

    match state.initialize_module(module_box, &text, 10000) {
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
        InterpreterResult::ReaderError(ir_reader_error) => {
            panic!("Ir Reader Error: {ir_reader_error:?}")
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
                            // Function from another WASM module
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
        let interpreter = Interpreter::default();
        let result = interpreter.run(&text, &mut state, 10000);

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
            InterpreterResult::ReaderError(e) => {
                panic!("failed to read ir instruction {e:?}")
            }
        }
    }
}
