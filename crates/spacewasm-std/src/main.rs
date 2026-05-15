use spacewasm::{
    Box, ExportDesc, FuncRef, HostFunction, HostFunctionBreak, HostModule, InterpreterResult,
    InterpreterRunner, Memory, SectionKind, Store, Value, vec,
};
use spacewasm_std::FileStream;
use std::alloc::{Layout, alloc};
use std::ops::ControlFlow;

fn main() {
    let fprime_core = HostModule {
        name: "fprime_core",
        globals: vec![],
        functions: vec![
            HostFunction::new("panic", "iii".into(), "".into(), |state, a| {
                let Some(Value::I32(addr)) = a.get(0) else {
                    panic!("expected i32");
                };
                let Some(Value::I32(len)) = a.get(1) else {
                    panic!("expected i32");
                };
                let Some(Value::I32(line_no)) = a.get(2) else {
                    panic!("expected i32");
                };

                let f = state.ram.load(*addr as usize, *len as usize).unwrap();
                let s: &str = core::str::from_utf8(f).unwrap();

                eprintln!("PANIC {}:{}", s, line_no);
                ControlFlow::Break(HostFunctionBreak::Trap)
            }),
            HostFunction::new("rsleep", "I".into(), "".into(), |_, a| {
                eprintln!("RSLEEP {:?}", a.get(0));
                ControlFlow::Continue(None)
            }),
            HostFunction::new("command", "ii".into(), "i".into(), |_, a| {
                eprintln!("COMMAND {:?} {:?}", a.get(0), a.get(1));
                ControlFlow::Continue(Some(Value::I32(0)))
            }),
            HostFunction::new("message", "ii".into(), "".into(), |state, a| {
                let Some(Value::I32(msg_ptr)) = a.get(0) else {
                    panic!("expected i32");
                };
                let Some(Value::I32(msg_len)) = a.get(1) else {
                    panic!("expected i32");
                };

                let msg_r = state
                    .ram
                    .load(*msg_ptr as usize, *msg_len as usize)
                    .unwrap();

                let msg: &str = core::str::from_utf8(msg_r).unwrap();

                eprintln!("MESSAGE {msg}");
                ControlFlow::Continue(None)
            }),
            HostFunction::new("telemetry", "iiiii".into(), "i".into(), |state, a| {
                let Some(Value::I32(id)) = a.get(0) else {
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
                state.ram.store_u16(*time_ptr as usize, 0).unwrap();

                // Time context
                state.ram.store_u8((*time_ptr as usize) + 2, 0).unwrap();

                // Seconds
                state.ram.store_u32((*time_ptr as usize) + 3, 0).unwrap();

                // Useconds
                state.ram.store_u32((*time_ptr as usize) + 7, 0).unwrap();

                eprintln!("TELEMETRY {id}");
                ControlFlow::Continue(Some(Value::I32(0)))
            }),
        ],
    };

    let env = HostModule {
        name: "env",
        globals: vec![],
        functions: vec![HostFunction::new(
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
    };

    let mut store = Store::new(3, [fprime_core, env]).unwrap();

    std::env::args().skip(1).for_each(|path| {
        let file = std::fs::File::open(path).expect("failed to open file");
        match spacewasm::Module::new_with_statistics::<256>(
            "main",
            &mut FileStream::new(file),
            &store,
        ) {
            Ok((module, stats)) => {
                let mut total: usize = 0;
                for (i, section) in stats.iter().enumerate() {
                    let section_kind = SectionKind::convert(i as u8).unwrap();
                    eprintln!("{:?}: {} bytes", section_kind, section.total_bytes);
                    total += section.total_bytes as usize;
                }

                eprintln!("Total: {}", total);
                eprintln!(
                    "Compilation Ratio: {:.2}x",
                    (total as f64) / (module.wasm_size as f64)
                );

                let full_page_usage = if module.text.len() > 1 {
                    (module.text.len() - 1) * 256
                } else {
                    0
                };

                eprintln!("Code pages: {}", module.text.len());
                eprintln!(
                    "Code word usage (16-bits): {} / {} ({:.2}%)",
                    full_page_usage + module.final_page_offset as usize,
                    module.text.len() * 256,
                    100.0 * ((full_page_usage + module.final_page_offset as usize) as f64)
                        / (module.text.len() * 256) as f64
                );
                eprintln!(
                    "Final page: {} / 256 ({:.2}%)",
                    module.final_page_offset,
                    100.0 * (module.final_page_offset as f64 / 256.0)
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

                store.modules.push(Box::new(module).unwrap());
                let module = store.modules.last().unwrap();
                let heap_size =
                    if let Some(spacewasm::MemType(spacewasm::Limit { min })) = module.memory {
                        min
                    } else {
                        0
                    } * 65536;

                let mut state = spacewasm::InterpreterState::new(
                    1024,
                    Memory::from(
                        unsafe { alloc(Layout::from_size_align(heap_size as usize, 16).unwrap()) },
                        heap_size as usize,
                    ),
                );

                state.initialize(&module).unwrap();

                let fi = match &module.start {
                    None => {
                        let f = module.exports.iter().find(|f| &f.name == "run").unwrap();
                        let ExportDesc::Func(fi) = f.desc else {
                            panic!()
                        };
                        fi
                    }
                    Some(fi) => *fi,
                };

                let interpreter = spacewasm::Interpreter::new(&store, module);

                let FuncRef::Func(fi) = module.get_func_ref(fi).unwrap() else {
                    panic!()
                };
                let f = &module.functions[fi as usize];
                interpreter.invoke(&mut state, f, &[]);

                eprintln!("====");

                let mut result = InterpreterResult::OutOfFuel;
                while result == InterpreterResult::OutOfFuel {
                    result = interpreter.run(&module.text, &mut state, usize::MAX)
                }

                eprintln!("Interpreter result: {:?}", result)
            }
            Err(err) => {
                eprintln!("Failed to parse: {:?}", err)
            }
        }
    });
}
