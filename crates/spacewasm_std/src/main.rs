use spacewasm::{
    CodeBuilder, CompilerOptions, ExportDesc, HostFunction, HostFunctionBreak, HostModule,
    InterpreterResult, InterpreterRunner, ModuleRef, PageAllocator, Ref, SectionKind, ValType,
    Value, WasmRef, vec,
};
use spacewasm_util::{FileStream, RustSystemAllocator};
use std::ops::ControlFlow;
use std::time::Instant;

spacewasm::global_allocator!(
    PageAllocator<RustSystemAllocator, 16>,
    PageAllocator::new(RustSystemAllocator, 8192)
);

const MAX_CODE_PAGES: u32 = 256;
const MAX_CONTROL_FRAMES: usize = 64;
const MAX_STACK_DEPTH: usize = 256;

fn guest_error(msg: impl core::fmt::Display) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(1);
}

fn main() {
    let path = std::env::args().nth(1).unwrap();

    let start = Instant::now();
    let mut code_builder = CodeBuilder::new(CompilerOptions {
        allow_memory_grow: false,
        max_backpatch_iterations: None,
        max_code_pages: MAX_CODE_PAGES,
    })
    .expect("failed to allocate code builder");

    let fprime_core = HostModule {
        name: "fprime_core".into(),
        globals: vec![],
        functions: vec![
            HostFunction::new("panic", "iii".into(), "".into(), |state, a| {
                let Some(Value::I32(addr)) = a.first() else {
                    panic!("expected i32");
                };
                let Some(Value::I32(len)) = a.get(1) else {
                    panic!("expected i32");
                };
                let Some(Value::I32(line_no)) = a.get(2) else {
                    panic!("expected i32");
                };

                // `addr`/`len` are guest-supplied: an out-of-bounds or
                // non-UTF-8 pointer must trap the guest, not panic the host CLI.
                let Ok(f) = state.memory.load(*addr as usize, *len as usize) else {
                    return ControlFlow::Break(HostFunctionBreak::Trap);
                };
                let Ok(s) = core::str::from_utf8(f) else {
                    return ControlFlow::Break(HostFunctionBreak::Trap);
                };

                eprintln!("PANIC {}:{}", s, line_no);
                ControlFlow::Break(HostFunctionBreak::Trap)
            }),
            HostFunction::new("rsleep", "I".into(), "".into(), |_, a| {
                eprintln!("RSLEEP {:?}", a.first());
                ControlFlow::Continue(None)
            }),
            HostFunction::new("command", "ii".into(), "i".into(), |_, a| {
                eprintln!("COMMAND {:?} {:?}", a.first(), a.get(1));
                ControlFlow::Continue(Some(Value::I32(0)))
            }),
            HostFunction::new("message", "ii".into(), "".into(), |state, a| {
                let Some(Value::I32(msg_ptr)) = a.first() else {
                    panic!("expected i32");
                };
                let Some(Value::I32(msg_len)) = a.get(1) else {
                    panic!("expected i32");
                };

                let Ok(msg_r) = state.memory.load(*msg_ptr as usize, *msg_len as usize) else {
                    return ControlFlow::Break(HostFunctionBreak::Trap);
                };
                let Ok(msg) = core::str::from_utf8(msg_r) else {
                    return ControlFlow::Break(HostFunctionBreak::Trap);
                };

                eprintln!("MESSAGE {msg}");
                ControlFlow::Continue(None)
            }),
            HostFunction::new("telemetry", "iiiii".into(), "i".into(), |state, a| {
                let Some(Value::I32(id)) = a.first() else {
                    panic!("expected i32");
                };
                let Some(Value::I32(time_ptr)) = a.get(1) else {
                    panic!("expected i32");
                };
                let Some(Value::I32(_time_len)) = a.get(2) else {
                    panic!("expected i32");
                };
                let Some(Value::I32(_value_ptr)) = a.get(3) else {
                    panic!("expected i32");
                };
                let Some(Value::I32(_value_len)) = a.get(4) else {
                    panic!("expected i32");
                };

                // `time_ptr` is guest-supplied: trap on any out-of-bounds store
                // rather than panicking the host CLI.
                let wrote = state
                    .memory
                    .store_u16(*time_ptr as usize, 0) // Time base
                    .and(state.memory.store_u8((*time_ptr as usize) + 2, 0)) // Time context
                    .and(state.memory.store_u32((*time_ptr as usize) + 3, 0)) // Seconds
                    .and(state.memory.store_u32((*time_ptr as usize) + 7, 0)); // Useconds
                if wrote.is_err() {
                    return ControlFlow::Break(HostFunctionBreak::Trap);
                }

                eprintln!("TELEMETRY {id}");
                ControlFlow::Continue(Some(Value::I32(0)))
            }),
        ],
        memory: spacewasm::Vec::zero(),
        table: spacewasm::Vec::zero(),
    };
    let env = HostModule {
        name: "env".into(),
        globals: vec![],
        functions: vec![HostFunction::new(
            "clock_ms",
            "".into(),
            "I".into(),
            move |_, _| {
                let elapse = start.elapsed();
                let ms = elapse.as_secs() * 1000 + (elapse.subsec_nanos() as u64 / 1_000_000);

                ControlFlow::Continue(Some(Value::I64(ms as i64)))
            },
        )],
        memory: spacewasm::Vec::zero(),
        table: spacewasm::Vec::zero(),
    };

    let mut state = spacewasm::Engine::new(
        1024,
        1,
        spacewasm::Vec::from_array([fprime_core, env]).unwrap(),
    )
    .unwrap();

    let file = std::fs::File::open(path).expect("failed to open file");
    let mut file_stream = FileStream::new(file);
    let (module, stats) =
        match spacewasm::Module::new_with_statistics::<MAX_CONTROL_FRAMES, MAX_STACK_DEPTH>(
            "main",
            &mut file_stream,
            &mut state.store,
            &mut code_builder,
            spacewasm::Rc::new(RustSystemAllocator)
                .unwrap()
                .into_wasm_memory_allocator(),
        ) {
            Ok(parsed) => parsed,
            Err(e) => guest_error(format!("failed to decode/validate wasm module: {e:?}")),
        };

    let text = code_builder.pages();
    let final_page_offset = code_builder.offset();

    let module_ref = match state.push_module(module) {
        Ok(module_ref) => module_ref,
        Err(e) => guest_error(format!("failed to instantiate wasm module: {e:?}")),
    };
    if let Some(start) = state.module_start(module_ref) {
        if let Err(e) = state.invoke(start, &[]) {
            guest_error(format!("failed to invoke start function: {e:?}"));
        }
        match spacewasm::Interpreter.run(text, &mut state, usize::MAX) {
            InterpreterResult::Finished => {}
            InterpreterResult::OutOfFuel => guest_error("insufficient fuel for initialization"),
            InterpreterResult::Trap(t) => guest_error(format!("trap during initialization: {t:?}")),
            InterpreterResult::Pause => guest_error("unexpected pause during initialization"),
        }
    }

    let module = state.store.modules().last().unwrap();

    let mut total: usize = 0;
    for (i, section) in stats.iter().enumerate() {
        let section_kind = SectionKind::convert(i as u8).unwrap();
        eprintln!("{:?}: {} bytes", section_kind, section.total_bytes);
        total += section.total_bytes as usize;
    }

    let wasm_size = file_stream.len();

    eprintln!("Total: {}", total);
    eprintln!(
        "Compilation Ratio: {:.2}x",
        (total as f64) / (wasm_size as f64)
    );

    let full_page_usage = if text.len() > 1 {
        (text.len() - 1) * 256
    } else {
        0
    };

    eprintln!("Code pages: {}", text.len());
    eprintln!(
        "Code word usage (16-bits): {} / {} ({:.2}%)",
        full_page_usage + final_page_offset,
        text.len() * 256,
        100.0 * ((full_page_usage + final_page_offset) as f64) / (text.len() * 256) as f64
    );
    eprintln!(
        "Final page: {} / 256 ({:.2}%)",
        final_page_offset,
        100.0 * (final_page_offset as f64 / 256.0)
    );

    eprintln!("Exports:");
    for i in &module.exports {
        match &i.desc {
            ExportDesc::Func(fi) => {
                eprintln!("Function: {} {:?}", i.name, fi);
            }
            ExportDesc::Table(_) => {}
            ExportDesc::Mem(_) => {}
            ExportDesc::Global(_) => {}
        }
    }
    eprintln!("====");

    let module = state.store.modules().last().unwrap();

    let fi = {
        let Some(f) = module.exports.iter().find(|f| &f.name == "run") else {
            guest_error("wasm module does not export a `run` function");
        };
        let ExportDesc::Func(fi) = f.desc else {
            guest_error("exported `run` is not a function");
        };
        fi
    };

    let module = state.store.modules().last().unwrap();
    let Some(Ref::Module(fi)) = module.get_func_ref(fi) else {
        guest_error("exported `run` function reference is invalid");
    };

    if let Err(e) = state.invoke(
        WasmRef {
            module: ModuleRef(0),
            index: fi,
        },
        &[],
    ) {
        guest_error(format!("failed to invoke `run` function: {e:?}"));
    }

    let mut result = InterpreterResult::OutOfFuel;
    while result == InterpreterResult::OutOfFuel {
        result = spacewasm::Interpreter.run(text, &mut state, usize::MAX)
    }

    let InterpreterResult::Finished = result else {
        guest_error(format!("interpreter failed: {result:?}"));
    };

    eprintln!(
        "Interpreter result: {:?}",
        state.result.map(|v| v.to_value(ValType::F32))
    )
}
