use spacewasm::{
    CodeBuilder, CompilerOptions, ExportDesc, HostFunction, HostModule, InitializeResult
    , InterpreterResult, InterpreterRunner, ModuleRef, PageAllocator, RawValue,
    Ref, Store, Value, WasmRef,
};
use spacewasm_util::{FileStream, RustSystemAllocator};
use std::ops::ControlFlow;
use std::time::Instant;

spacewasm::global_allocator!(
    PageAllocator<16>,
    PageAllocator::new(&RustSystemAllocator {}, 8192)
);

fn main() {
    println!("\n=== CoreMark Benchmark ===");
    println!("Reference: https://github.com/wasm3/wasm-coremark\n");

    // According to the reference implementation, clock_ms should return current time in milliseconds
    // See: https://github.com/wasm3/wasm-coremark/blob/main/coremark-minimal.html
    // JavaScript: env: { clock_ms: () => BigInt(Date.now()) }
    // Python: return int(round(time.time() * 1000))

    use std::sync::atomic::{AtomicUsize, Ordering};
    static CLOCK_CALL_COUNT: AtomicUsize = AtomicUsize::new(0);

    let env = HostModule {
        name: "env",
        globals: spacewasm::vec![],
        functions: spacewasm::vec![HostFunction::new(
            "clock_ms",
            "".into(),
            "I".into(),
            |_, _| {
                let ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as i64;

                eprintln!("CLOCK_MS {}", ms);

                ControlFlow::Continue(Some(Value::I64(ms)))
            },
        )],
        memory: spacewasm::Vec::zero(),
        table: spacewasm::Vec::zero(),
    };

    let mut store = Store::new(2, [env]).unwrap();
    let mut code_builder = CodeBuilder::<256>::default();

    // Try multiple paths to find the wasm file
    let wasm_paths = [
        "benches/coremark-minimal.wasm",
        "crates/spacewasm_std/benches/coremark-minimal.wasm",
        concat!(env!("CARGO_MANIFEST_DIR"), "/benches/coremark-minimal.wasm"),
    ];

    let file = wasm_paths
        .iter()
        .find_map(|path| std::fs::File::open(path).ok())
        .expect("failed to open coremark-minimal.wasm in any expected location");

    let module = spacewasm::Module::new::<256>(
        "coremark",
        &mut FileStream::new(file),
        &mut store,
        &mut code_builder,
        spacewasm::Rc::new(RustSystemAllocator)
            .unwrap()
            .into_wasm_memory_allocator(),
        CompilerOptions::default(),
    )
    .expect("failed to parse wasm module");

    let (text, _final_page_offset) = code_builder.finish().unwrap();

    let mut state = store.allocate(1024).unwrap();
    match state.initialize_module(spacewasm::Box::new(module).unwrap(), &text, usize::MAX) {
        InitializeResult::Ok => {}
        InitializeResult::OutOfFuel => panic!("insufficient fuel for initialization"),
        InitializeResult::Trap(t) => panic!("trap during initialization {t:?}"),
        InitializeResult::ReaderError(e) => panic!("ir reader error {e:?}"),
        InitializeResult::Pause => panic!("pause during init"),
    }

    let module = state.store.modules().last().unwrap();
    let export = module
        .exports
        .iter()
        .find(|e| &e.name == "run")
        .expect("no run function found");
    let func = match export.desc {
        ExportDesc::Func(fi) => {
            let Ref::Module(fdi) = module.get_func_ref(fi).unwrap() else {
                panic!("invalid function ref")
            };
            WasmRef {
                module: ModuleRef(0),
                index: fdi,
            }
        }
        _ => panic!("run export is not a function"),
    };

    state.invoke(func, &[]).unwrap();

    let bench_start = Instant::now();

    eprintln!("Starting execution...");
    let interpreter = spacewasm::Interpreter::default();
    let mut result = InterpreterResult::OutOfFuel;
    while result == InterpreterResult::OutOfFuel {
        result = interpreter.run(&text, &mut state, usize::MAX)
    }
    let elapsed = bench_start.elapsed();

    eprintln!("Execution completed with result: {:?}", result);
    let total_calls = CLOCK_CALL_COUNT.load(Ordering::Relaxed);
    eprintln!("Total clock_ms calls: {}", total_calls);
    eprintln!(
        "Final PC: {:?}, SP: {}, FP: {}",
        state.pc, state.sp, state.fp
    );

    // Extract return value (CoreMark score as f32)
    // According to https://github.com/wasm3/wasm-coremark:
    // "Call f32 run() function. It should take 12..20 seconds to execute and return a CoreMark result."
    // "if res > 1: print(f'Result: {res:.3f}') else: print('Error')"
    match result {
        InterpreterResult::Finished => {
            // The run function returns f32, so interpret the bits as float
            let coremark_score = state.result.unwrap_or(RawValue::from_32(0)).read_f32();

            println!("Execution time: {:.3}s", elapsed.as_secs_f64());
            println!("Return value: {:.3}", coremark_score);
            println!();

            if coremark_score > 1.0 {
                println!("=== CoreMark Results ===");
                println!("CoreMark Score: {:.3}", coremark_score);
                println!("CoreMark/MHz: {:.3}", coremark_score);
                println!(
                    "Iterations/sec: {:.2}",
                    coremark_score as f64 / elapsed.as_secs_f64()
                );
                println!("========================");
            } else {
                println!(
                    "Error: CoreMark returned {:.3} (expected > 1.0)",
                    coremark_score
                );
                println!("This typically means:");
                println!("  - The benchmark didn't run for at least 10 seconds");
                println!("  - The clock_ms function is not working correctly");
                println!("  - There was an error during execution");
                std::process::exit(1);
            }
        }
        _ => {
            eprintln!("Error: Unexpected interpreter result: {:?}", result);
            std::process::exit(1);
        }
    }

    println!("\n");
}
