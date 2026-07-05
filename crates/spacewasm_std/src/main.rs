use spacewasm::{
    CodeBuilder, CompilerOptions, ExportDesc, HostFunction, HostFunctionBreak, HostModule,
    InterpreterResult, InterpreterRunner, ModuleRef, PageAllocator, Ref, SectionKind, ValType,
    Value, WasmRef, vec,
};
use spacewasm_util::{FileStream, RustSystemAllocator};
use std::ops::ControlFlow;
use std::time::Instant;

spacewasm::global_allocator!(
    PageAllocator<16>,
    PageAllocator::new(&RustSystemAllocator {}, 8192)
);

fn main() {
    let path = std::env::args().nth(1).unwrap();

    let start = Instant::now();
    let mut code_builder = CodeBuilder::<256>::default();

    let fprime_core = HostModule {
        name: "fprime_core",
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

                let f = state.memory.load(*addr as usize, *len as usize).unwrap();
                let s: &str = core::str::from_utf8(f).unwrap();

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

                let msg_r = state
                    .memory
                    .load(*msg_ptr as usize, *msg_len as usize)
                    .unwrap();

                let msg: &str = core::str::from_utf8(msg_r).unwrap();

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

                // Time base
                state.memory.store_u16(*time_ptr as usize, 0).unwrap();

                // Time context
                state.memory.store_u8((*time_ptr as usize) + 2, 0).unwrap();

                // Seconds
                state.memory.store_u32((*time_ptr as usize) + 3, 0).unwrap();

                // Useconds
                state.memory.store_u32((*time_ptr as usize) + 7, 0).unwrap();

                eprintln!("TELEMETRY {id}");
                ControlFlow::Continue(Some(Value::I32(0)))
            }),
        ],
        memory: spacewasm::Vec::zero(),
        table: spacewasm::Vec::zero(),
    };
    let env = HostModule {
        name: "env",
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

    let mut store = spacewasm::Store::new(1, [fprime_core, env]).unwrap();

    let file = std::fs::File::open(path).expect("failed to open file");
    let mut file_stream = FileStream::new(file);
    let (module, stats) = spacewasm::Module::new_with_statistics(
        "main",
        &mut file_stream,
        &mut store,
        &mut code_builder,
        spacewasm::Rc::new(RustSystemAllocator)
            .unwrap()
            .into_wasm_memory_allocator(),
        CompilerOptions::default(),
    )
    .expect("failed to parse wasm module");

    let (text, final_page_offset) = code_builder.finish().unwrap();

    let mut state = store.allocate(1024).unwrap();
    match state.initialize_module(module, &text, usize::MAX) {
        InterpreterResult::Finished => {}
        InterpreterResult::OutOfFuel => panic!("insufficient fuel for initialization"),
        InterpreterResult::Trap(t) => panic!("trap during initialization {t:?}"),
        InterpreterResult::ReaderError(e) => panic!("ir reader error {e:?}"),
        InterpreterResult::Pause => panic!("pause during init"),
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
                eprintln!("Function: {} {:?}", &i.name, fi);
            }
            ExportDesc::Table(_) => {}
            ExportDesc::Mem(_) => {}
            ExportDesc::Global(_) => {}
        }
    }
    eprintln!("====");

    let module = state.store.modules().last().unwrap();

    let fi = {
        let f = module.exports.iter().find(|f| &f.name == "run").unwrap();
        let ExportDesc::Func(fi) = f.desc else {
            panic!()
        };
        fi
    };

    let module = state.store.modules().last().unwrap();
    let Ref::Module(fi) = module.get_func_ref(fi).unwrap() else {
        panic!()
    };

    state
        .invoke(
            WasmRef {
                module: ModuleRef(0),
                index: fi,
            },
            &[],
        )
        .unwrap();

    let interpreter = spacewasm::Interpreter::default();

    // let dbg = Inspector {
    //     v: &interpreter,
    //     out: *out,
    // };

    let mut result = InterpreterResult::OutOfFuel;
    while result == InterpreterResult::OutOfFuel {
        result = interpreter.run(&text, &mut state, usize::MAX)
    }

    let InterpreterResult::Finished = result else {
        panic!("interpreter failed: {:?}", result)
    };

    eprintln!(
        "Interpreter result: {:?}",
        state.result.map(|v| v.to_value(ValType::F32))
    )
}
