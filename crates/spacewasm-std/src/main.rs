use spacewasm::{
    ExportDesc, FuncRef, HostFunction, HostFunctionPause, Memory, ModuleImports, SectionKind,
    ValType, Value,
};
use std::alloc::Layout;
use std::ops::ControlFlow;

use spacewasm_std::FileStream;

fn main() {
    let imports = ModuleImports {
        globals: &[],
        functions: &[
            HostFunction::new(
                "fprime_core",
                "panic",
                &[ValType::I32, ValType::I32, ValType::I32],
                &[],
                |state, a| {
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
                    ControlFlow::Break(HostFunctionPause::Trap)
                },
            ),
            HostFunction::new("fprime_core", "rsleep", &[ValType::I64], &[], |_, a| {
                eprintln!("RSLEEP {:?}", a.get(0));
                ControlFlow::Continue(None)
            }),
            HostFunction::new(
                "fprime_core",
                "command",
                &[ValType::I32, ValType::I32],
                &[ValType::I32],
                |_, a| {
                    eprintln!("COMMAND {:?} {:?}", a.get(0), a.get(1));
                    ControlFlow::Continue(Some(Value::I32(0)))
                },
            ),
            HostFunction::new(
                "fprime_core",
                "message",
                &[ValType::I32, ValType::I32],
                &[],
                |state, a| {
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
                },
            ),
            HostFunction::new(
                "fprime_core",
                "telemetry",
                &[
                    ValType::I32,
                    ValType::I32,
                    ValType::I32,
                    ValType::I32,
                    ValType::I32,
                ],
                &[ValType::I32],
                |state, a| {
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
                },
            ),
        ],
        memories: &[],
    };

    std::env::args().skip(1).for_each(|path| {
        let file = std::fs::File::open(path).expect("failed to open file");
        match spacewasm::Module::new::<256>(&mut FileStream::new(file), &imports) {
            Ok(module) => {
                let mut total: usize = 0;
                for (i, section) in module.memory_usage.iter().enumerate() {
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
                    full_page_usage + module.final_page_offset,
                    module.text.len() * 256,
                    100.0 * ((full_page_usage + module.final_page_offset) as f64)
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

                let mem = &module.memories[0];
                let heap_size = (mem.0.min as usize) * 65536;

                eprintln!("Heap size: {}", heap_size);

                let mut state = spacewasm::InterpreterState::new(
                    1024,
                    Memory::from(
                        unsafe {
                            std::alloc::alloc(Layout::from_size_align(heap_size, 64).unwrap())
                        },
                        heap_size,
                    ),
                );

                state.initialize(&module.globals, &module.data).unwrap();

                match module.start {
                    None => {
                        let f = module.exports.iter().find(|f| &f.name == "main").unwrap();
                        match f.desc {
                            ExportDesc::Func(fi) => {
                                let FuncRef::Func(fdi) = module.get_func_ref(fi).unwrap() else {
                                    panic!("Invalid main function ref")
                                };
                                let f = module.functions.get(fdi as usize).unwrap();
                                eprintln!(
                                    "fn main => {:?}",
                                    module.types.get(f.ty.0 as usize).unwrap()
                                );
                                state.invoke(f, &[]);
                            }
                            ExportDesc::Table(_) => {}
                            ExportDesc::Mem(_) => {}
                            ExportDesc::Global(_) => {}
                        }
                    }
                    Some(fi) => {
                        let f = module.functions.get(fi.0 as usize).unwrap();
                        state.invoke(f, &[]);
                    }
                }

                let interpreter = spacewasm::Interpreter::new(
                    &module.functions,
                    module.module_imports.globals,
                    module.module_imports.functions,
                    module.module_imports.memories,
                    &module.table,
                    &module.types,
                );

                eprintln!("====");

                let code = spacewasm::Code::new(module.text);
                let r = interpreter.run(&code, &mut state, 100000);

                eprintln!("Interpreter result: {:?}", r)
            }
            Err(err) => {
                eprintln!("Failed to parse: {:?}", err)
            }
        }
    });
}
