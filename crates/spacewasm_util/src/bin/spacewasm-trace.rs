//! Execution tracer for WASM modules.
//!
//! Runs a WASM module and prints execution traces showing pc/sp/fp state
//! for each instruction.
//!
//! Usage:
//!   spacewasm-trace <file.wasm> [--limit N]
//!   spacewasm-trace --stdin [--limit N]

use spacewasm::InitializeResult;
use spacewasm::{
    AllocError, Allocator, Box, CodeBuilder, CompilerOptions, ExportDesc, InnerVec, Interpreter,
    InterpreterBreak, InterpreterResult, InterpreterRunner, MemoryStatistics, Module, ModuleRef,
    ReaderError, Ref, Store, WasmMemoryAllocator, WasmRef, WasmStream,
};
use spacewasm_util::StateTracer;
use std::env;
use std::fs;
use std::io::{self, Read};
use std::process;

/// Simple byte stream that reads from a buffer.
struct ByteStream {
    buffer: std::vec::Vec<u8>,
    consumed: bool,
}

impl ByteStream {
    fn new(data: std::vec::Vec<u8>) -> Self {
        Self {
            buffer: data,
            consumed: false,
        }
    }
}

impl WasmStream for ByteStream {
    fn read(&mut self) -> Result<Option<InnerVec<u8>>, ReaderError> {
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
            Ok(std::ptr::NonNull::new(std::alloc::alloc(layout))
                .ok_or(AllocError::AllocationFailed)?)
        }
    }

    fn reallocate(
        &self,
        ptr: std::ptr::NonNull<u8>,
        old_layout: std::alloc::Layout,
        layout: std::alloc::Layout,
    ) -> Result<std::ptr::NonNull<u8>, AllocError> {
        unsafe {
            Ok(
                std::ptr::NonNull::new(std::alloc::realloc(
                    ptr.as_ptr(),
                    old_layout,
                    layout.size(),
                ))
                .ok_or(AllocError::AllocationFailed)?,
            )
        }
    }

    fn deallocate(&self, ptr: std::ptr::NonNull<u8>, layout: std::alloc::Layout) {
        unsafe { std::alloc::dealloc(ptr.as_ptr(), layout) }
    }
}

fn main() {
    let args: std::vec::Vec<String> = env::args().collect();

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
        eprintln!("Trace WASM module execution, printing pc/sp/fp for each instruction.");
        eprintln!();
        eprintln!("Options:");
        eprintln!("  --stdin       Read WASM from stdin instead of a file");
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

    // Read WASM bytes
    let wasm_bytes: std::vec::Vec<u8> = if use_stdin {
        let mut buffer = std::vec::Vec::new();
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

    // Compile module
    let mut store = Store::new(16, []).unwrap_or_else(|e| {
        eprintln!("Failed to create store: {:?}", e);
        process::exit(1);
    });

    let mut code_builder = CodeBuilder::<128>::default();
    let mut stream = ByteStream::new(wasm_bytes);

    let module = Module::new::<128>(
        "",
        &mut stream,
        &mut store,
        &mut code_builder,
        &SystemAllocator,
        CompilerOptions {
            allow_memory_grow: true,
        },
    )
    .unwrap_or_else(|e| {
        eprintln!("Failed to compile module: {:?}", e);
        process::exit(1);
    });

    let (text, _) = code_builder.finish().unwrap_or_else(|e| {
        eprintln!("Failed to finish compilation: {:?}", e);
        process::exit(1);
    });

    // Initialize
    let mut state = store.allocate(512).unwrap_or_else(|e| {
        eprintln!("Failed to allocate state: {:?}", e);
        process::exit(1);
    });

    let module_box = Box::new(module).unwrap();

    // Initialize with instruction limit to prevent infinite loops in start functions
    // Catch panics (e.g., from strict-assertions) during initialization
    let init_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        state.initialize_module(module_box, &text, 10000)
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
        InitializeResult::Ok => {}
        InitializeResult::OutOfFuel => {
            eprintln!("Module initialization ran out of fuel (instruction limit)");
            process::exit(1);
        }
        InitializeResult::Trap(trap_reason) => {
            eprintln!("Trap during initialization: {trap_reason:?}");
            process::exit(1);
        }
        InitializeResult::ReaderError(ir_reader_error) => {
            eprintln!("Reader error during initialization: {ir_reader_error:?}");
            process::exit(1);
        }
        InitializeResult::Pause => {
            eprintln!("Module initialization paused");
            process::exit(1);
        }
    }

    let module_idx = state.store.modules().len().saturating_sub(1);
    let module = match state.store.modules().get(module_idx) {
        Some(m) => m,
        None => {
            eprintln!("No module in store after initialization");
            process::exit(1);
        }
    };

    // Find exported functions
    let exported_funcs: std::vec::Vec<_> = module
        .exports
        .iter()
        .filter_map(|export| {
            if let ExportDesc::Func(func_idx) = export.desc {
                module
                    .get_func_ref(func_idx)
                    .and_then(|func_ref| match func_ref {
                        Ref::Module(index) => Some((
                            export.name.clone(),
                            WasmRef {
                                module: ModuleRef(module_idx as u8),
                                index,
                            },
                        )),
                        Ref::Extern { module, index } => {
                            Some((export.name.clone(), WasmRef { module, index }))
                        }
                        _ => None,
                    })
            } else {
                None
            }
        })
        .collect();

    if exported_funcs.is_empty() {
        eprintln!("No exported functions found");
        process::exit(1);
    }

    eprintln!("Found {} exported function(s)", exported_funcs.len());

    // Execute each exported function
    for (name, wasm_ref) in exported_funcs {
        eprintln!("\n=== Executing: {} ===\n", name);

        state.invoke(wasm_ref, &[]).unwrap();

        let interpreter = Interpreter::default();
        let tracer = StateTracer::new(&interpreter, limit);

        // Catch panics to dump trace before crashing
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tracer.run(&text, &mut state, 10000)
        }));

        // Always dump trace
        println!("{}", tracer.dump_history());

        // Handle result or panic
        match result {
            Ok(InterpreterResult::OutOfFuel) => {
                eprintln!("\nResult: Out of fuel (instruction limit reached)");
            }
            Ok(InterpreterResult::Instruction(InterpreterBreak::Finished)) => {
                eprintln!("\nResult: Completed successfully");
            }
            Ok(InterpreterResult::Instruction(InterpreterBreak::Trap(reason))) => {
                eprintln!("\nResult: Trapped - {:?}", reason);
                process::exit(1);
            }
            Ok(InterpreterResult::Instruction(InterpreterBreak::Pause)) => {
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
