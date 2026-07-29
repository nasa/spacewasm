use spacewasm::{
    CodeBuilder, CompilerOptions, Engine, ExportDesc, HostFunction, HostModule, InterpreterResult,
    InterpreterRunner, ModuleRef, PageAllocator, RawValue, Ref, StartInvocation, Value,
    Vec as WasmVec, WasmRef,
};
use spacewasm_util::{FileStream, RustSystemAllocator};
use std::ops::ControlFlow;
use std::time::Instant;

spacewasm::global_allocator!(
    PageAllocator<16>,
    PageAllocator::new(&RustSystemAllocator {}, 8192)
);

const MAX_CODE_PAGES: u32 = 32;
const MAX_CONTROL_FRAMES: usize = 64;
const MAX_STACK_DEPTH: usize = 256;

/// Timestamps handed to the wasm module, in order, when `COREMARK_FIXED_CLOCK=1`.
///
/// CoreMark sizes its own workload from the clock: it times a run of ten
/// iterations, keeps multiplying by ten until that takes at least a second,
/// then settles on `iterations * (1 + 10 / floor(seconds))`. The divisor is an
/// integer, so a run that takes 1.9s and one that takes 2.1s end up doing
/// nearly twice as much work as each other. That is fine for a score, which
/// divides the work back out, but it makes the amount of code executed a
/// property of the machine rather than of the build, and so not worth counting.
///
/// These four values are what the module reads instead: one timed calibration
/// round reporting exactly one second, which pins the workload at 110
/// iterations, and a measured window of eleven seconds, which clears the ten
/// second minimum CoreMark requires for a valid result. The score is then a
/// constant 110 / 11, and [`FIXED_CLOCK_SCORE`] asserts it.
const FIXED_CLOCK_MS: [i64; 4] = [0, 1_000, 1_000, 12_000];

/// How far the fixed clock advances per call once [`FIXED_CLOCK_MS`] runs out,
/// so an unexpected extra timing round changes the score rather than seeing
/// time stand still.
const FIXED_CLOCK_STEP_MS: i64 = 12_000;

/// The only score [`FIXED_CLOCK_MS`] can produce, if the module still times
/// itself the way it does today.
const FIXED_CLOCK_SCORE: f32 = 10.0;

fn main() {
    println!("\n=== CoreMark Benchmark ===");
    println!("Reference: https://github.com/wasm3/wasm-coremark\n");

    // Fixed-clock runs are for counting instructions, not for timing: the
    // workload is a fraction of a normal run and the score is a self-check.
    let fixed_clock = std::env::var("COREMARK_FIXED_CLOCK").as_deref() == Ok("1");
    if fixed_clock {
        println!("Fixed clock: workload pinned for instruction counting.\n");
    }

    // According to the reference implementation, clock_ms should return current time in milliseconds
    // See: https://github.com/wasm3/wasm-coremark/blob/main/coremark-minimal.html
    // JavaScript: env: { clock_ms: () => BigInt(Date.now()) }
    // Python: return int(round(time.time() * 1000))

    use std::sync::atomic::{AtomicUsize, Ordering};
    static CLOCK_CALL_COUNT: AtomicUsize = AtomicUsize::new(0);

    let env = HostModule {
        name: "env".into(),
        globals: spacewasm::vec![],
        functions: spacewasm::vec![HostFunction::new(
            "clock_ms",
            "".into(),
            "I".into(),
            move |_, _| {
                let call = CLOCK_CALL_COUNT.fetch_add(1, Ordering::Relaxed);

                let ms = if fixed_clock {
                    let past_end = (call + 1).saturating_sub(FIXED_CLOCK_MS.len()) as i64;
                    FIXED_CLOCK_MS
                        .get(call)
                        .copied()
                        .unwrap_or(FIXED_CLOCK_MS[FIXED_CLOCK_MS.len() - 1])
                        + FIXED_CLOCK_STEP_MS * past_end
                } else {
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as i64
                };

                ControlFlow::Continue(Some(Value::I64(ms)))
            },
        )],
        memory: spacewasm::Vec::zero(),
        table: spacewasm::Vec::zero(),
    };

    let mut state = Engine::new(1024, 2, WasmVec::from_array([env]).unwrap()).unwrap();
    let mut code_builder = CodeBuilder::new(CompilerOptions {
        allow_memory_grow: false,
        max_backpatch_iterations: 0,
        max_code_pages: MAX_CODE_PAGES,
    })
    .unwrap();

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

    let module = spacewasm::Module::new::<MAX_CONTROL_FRAMES, MAX_STACK_DEPTH>(
        "coremark",
        &mut FileStream::new(file),
        &mut state.store,
        &mut code_builder,
        spacewasm::Rc::new(RustSystemAllocator)
            .unwrap()
            .into_wasm_memory_allocator(),
    )
    .expect("failed to parse wasm module");

    let text = code_builder.pages();

    let module_ref = state.push_module(module);
    match state.invoke_start(module_ref) {
        StartInvocation::Finished => {}
        StartInvocation::Trap(t) => panic!("trap during initialization {t:?}"),
        StartInvocation::Pause => panic!("pause during init"),
        StartInvocation::Running => {
            match spacewasm::Interpreter.run(text, &mut state, usize::MAX) {
                InterpreterResult::Finished => {}
                InterpreterResult::OutOfFuel => panic!("insufficient fuel for initialization"),
                InterpreterResult::Trap(t) => panic!("trap during initialization {t:?}"),
                InterpreterResult::ReaderError(e) => panic!("ir reader error {e:?}"),
                InterpreterResult::Pause => panic!("pause during init"),
            }
        }
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
    let mut result = InterpreterResult::OutOfFuel;
    while result == InterpreterResult::OutOfFuel {
        result = spacewasm::Interpreter.run(text, &mut state, usize::MAX)
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

            // CoreMark only returns a score at all once its own CRC checks
            // pass, and under the fixed clock there is exactly one score it can
            // return. Anything else means the workload is no longer the one
            // FIXED_CLOCK_MS pins, so a count taken from it is not comparable
            // to a count taken from another build.
            if fixed_clock && (coremark_score - FIXED_CLOCK_SCORE).abs() > 0.001 {
                eprintln!(
                    "Error: fixed-clock score is {coremark_score:.3}, expected {FIXED_CLOCK_SCORE:.3}"
                );
                eprintln!("The module no longer times itself the way FIXED_CLOCK_MS assumes.");
                std::process::exit(1);
            }

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
