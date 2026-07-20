//! Execution tracer for Wasm modules.
//!
//! Runs a Wasm module and prints execution traces showing pc/sp/fp state
//! for each instruction.
//!
//! Usage:
//!   spacewasm-trace <file.wasm> [--limit N]
//!   spacewasm-trace --stdin [--limit N]

use spacewasm::{
    AllocError, Allocator, CodeBuilder, CompilerOptions, Engine, ExportDesc, InnerVec, Interpreter,
    InterpreterResult, InterpreterRunner, MemoryStatistics, Module, ModuleRef, Ref,
    StartInvocation, Vec as WasmVec, WasmMemoryAllocator, WasmRef, WasmStream,
};
use spacewasm::{ValType, Value};
use spacewasm_util::StateTracer;
use std::env;
use std::fs;
use std::io::{self, Read};
use std::process;

const MAX_CODE_PAGES: u32 = 128;
const MAX_CONTROL_FRAMES: usize = 128;
const MAX_STACK_DEPTH: usize = 256;

/// Simple byte stream that reads from a buffer.
struct ByteStream {
    buffer: Vec<u8>,
    consumed: bool,
}

impl ByteStream {
    fn new(data: Vec<u8>) -> Self {
        Self {
            buffer: data,
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
        let inner = InnerVec {
            ptr: self.buffer.as_mut_ptr(),
            capacity: self.buffer.len() as u32,
            len: self.buffer.len() as u32,
        };
        Ok(Some(inner))
    }

    fn return_(&mut self, _chunk: InnerVec<u8>) {
        // Buffer is kept alive in self.buffer
    }
}

/// System allocator.
struct SystemAllocator;

// Set up the global allocator for spacewasm
spacewasm::global_allocator!(SystemAllocator, SystemAllocator);

unsafe impl Allocator for SystemAllocator {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> Result<*mut u8, AllocError> {
        unsafe { Ok(std::alloc::alloc(layout)) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        unsafe { std::alloc::dealloc(ptr, layout) }
    }

    fn memory_statistics(&self) -> MemoryStatistics {
        MemoryStatistics {
            total_bytes: 0,
            pad_bytes: 0,
        }
    }
}

impl WasmMemoryAllocator for SystemAllocator {
    fn allocate(&self, layout: std::alloc::Layout) -> Result<std::ptr::NonNull<u8>, AllocError> {
        unsafe {
            std::ptr::NonNull::new(std::alloc::alloc(layout)).ok_or(AllocError::AllocationFailed)
        }
    }

    fn reallocate(
        &self,
        ptr: std::ptr::NonNull<u8>,
        old_layout: std::alloc::Layout,
        layout: std::alloc::Layout,
    ) -> Result<std::ptr::NonNull<u8>, AllocError> {
        unsafe {
            std::ptr::NonNull::new(std::alloc::realloc(ptr.as_ptr(), old_layout, layout.size()))
                .ok_or(AllocError::AllocationFailed)
        }
    }

    fn deallocate(&self, ptr: std::ptr::NonNull<u8>, layout: std::alloc::Layout) {
        unsafe { std::alloc::dealloc(ptr.as_ptr(), layout) }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();

    // Parse arguments
    let mut input_file: Option<String> = None;
    let mut use_stdin = false;
    let mut limit = 200; // Default trace buffer size

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--stdin" => use_stdin = true,
            "--limit" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("Error: --limit requires a number");
                    process::exit(1);
                }
                limit = args[i].parse().unwrap_or_else(|_| {
                    eprintln!("Error: invalid limit '{}'", args[i]);
                    process::exit(1);
                });
            }
            arg if !arg.starts_with('-') => {
                if input_file.is_some() {
                    eprintln!("Error: multiple input files specified");
                    process::exit(1);
                }
                input_file = Some(arg.to_string());
            }
            _ => {
                eprintln!("Error: unknown option '{}'", args[i]);
                process::exit(1);
            }
        }
        i += 1;
    }

    if input_file.is_none() && !use_stdin {
        eprintln!("Usage: {} <file.wasm> [--limit N]", args[0]);
        eprintln!("       {} --stdin [--limit N]", args[0]);
        eprintln!();
        eprintln!("Trace Wasm module execution, printing pc/sp/fp for each instruction.");
        eprintln!();
        eprintln!("Options:");
        eprintln!("  --stdin       Read Wasm from stdin instead of a file");
        eprintln!("  --limit N     Show last N instructions (default: 200)");
        eprintln!();
        eprintln!("Examples:");
        eprintln!("  # Trace from file");
        eprintln!("  {} module.wasm", args[0]);
        eprintln!();
        eprintln!("  # Trace from stdin with custom limit");
        eprintln!("  cat module.wasm | {} --stdin --limit 50", args[0]);
        eprintln!();
        eprintln!("  # Convert fuzzer seed and trace");
        eprintln!("  seed_to_wasm crash-xxx --stdout | {} --stdin", args[0]);
        process::exit(1);
    }

    // Read Wasm bytes
    let wasm_bytes: Vec<u8> = if use_stdin {
        let mut buffer = Vec::new();
        io::stdin().read_to_end(&mut buffer).unwrap_or_else(|e| {
            eprintln!("Failed to read from stdin: {}", e);
            process::exit(1);
        });
        buffer
    } else {
        let file = input_file.unwrap();
        fs::read(&file).unwrap_or_else(|e| {
            eprintln!("Failed to read '{}': {}", file, e);
            process::exit(1);
        })
    };

    eprintln!("Loaded {} bytes", wasm_bytes.len());

    // Create the engine (owns the store + execution state).
    let mut state = Engine::new(512, 16, WasmVec::zero()).unwrap_or_else(|e| {
        eprintln!("Failed to create engine: {:?}", e);
        process::exit(1);
    });

    // Compile module
    let mut code_builder = CodeBuilder::new(CompilerOptions {
        allow_memory_grow: true,
        max_backpatch_iterations: 0,
        max_code_pages: MAX_CODE_PAGES,
    })
    .unwrap();
    let mut stream = ByteStream::new(wasm_bytes);

    let module = Module::new::<MAX_CONTROL_FRAMES, MAX_STACK_DEPTH>(
        "",
        &mut stream,
        &mut state.store,
        &mut code_builder,
        spacewasm::Rc::new(SystemAllocator)
            .unwrap()
            .into_wasm_memory_allocator(),
    )
    .unwrap_or_else(|e| {
        eprintln!("Failed to compile module: {:?}", e);
        process::exit(1);
    });

    // Borrow the compiled text straight from the builder (no copy needed).
    let text = code_builder.pages();

    // Initialize with instruction limit to prevent infinite loops in start functions
    // Catch panics (e.g., from strict-assertions) during initialization
    let module_ref = state.push_module(module);
    let init_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        match state.invoke_start(module_ref) {
            StartInvocation::Finished => InterpreterResult::Finished,
            StartInvocation::Trap(t) => InterpreterResult::Trap(t),
            StartInvocation::Pause => InterpreterResult::Pause,
            StartInvocation::Running => Interpreter.run(text, &mut state, 10000),
        }
    }));

    let init_result = match init_result {
        Ok(result) => result,
        Err(panic_payload) => {
            eprintln!("\nPanic during module initialization (likely in start function)");
            eprintln!("This usually indicates:");
            eprintln!("  - Stack overflow in start function");
            eprintln!("  - Infinite recursion");
            eprintln!("  - Buffer overflow during initialization");
            eprintln!("\nUse 'wasm2wat' to inspect the start function.");
            std::panic::resume_unwind(panic_payload);
        }
    };

    match init_result {
        InterpreterResult::Finished => {}
        InterpreterResult::OutOfFuel => {
            eprintln!("Module initialization ran out of fuel (instruction limit)");
            process::exit(1);
        }
        InterpreterResult::Trap(trap_reason) => {
            eprintln!("Trap during initialization: {trap_reason:?}");
            process::exit(1);
        }
        InterpreterResult::ReaderError(ir_reader_error) => {
            eprintln!("Reader error during initialization: {ir_reader_error:?}");
            process::exit(1);
        }
        InterpreterResult::Pause => {
            eprintln!("Module initialization paused");
            process::exit(1);
        }
    }

    let module_idx = state.store.modules().len().saturating_sub(1);

    // Find exported functions
    let exported_funcs: Vec<(WasmRef, Vec<Value>)> = {
        let Some(module) = state.store.modules().get(module_idx) else {
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
                    let params: Vec<Value> = func_type
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

    if exported_funcs.is_empty() {
        eprintln!("No exported functions found");
        process::exit(1);
    }

    eprintln!("Found {} exported function(s)", exported_funcs.len());

    // Execute each exported function
    for (wasm_ref, values) in exported_funcs {
        state.reset();
        state.invoke(wasm_ref, &values).unwrap();

        let tracer = StateTracer::new(&Interpreter, limit);

        // Catch panics to dump trace before crashing
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tracer.run(text, &mut state, 10000)
        }));

        // Always dump trace
        println!("{}", tracer.dump_history());

        // Handle result or panic
        match result {
            Ok(InterpreterResult::OutOfFuel) => {
                eprintln!("\nResult: Out of fuel (instruction limit reached)");
            }
            Ok(InterpreterResult::Finished) => {
                eprintln!("\nResult: Completed successfully");
            }
            Ok(InterpreterResult::Trap(reason)) => {
                eprintln!("\nResult: Trapped - {:?}", reason);
                process::exit(1);
            }
            Ok(InterpreterResult::Pause) => {
                eprintln!("\nResult: Paused");
            }
            Ok(InterpreterResult::ReaderError(e)) => {
                eprintln!("\nResult: Reader error - {:?}", e);
                process::exit(1);
            }
            Err(panic_payload) => {
                eprintln!("\nResult: Panicked during execution");
                std::panic::resume_unwind(panic_payload);
            }
        }
    }
}
