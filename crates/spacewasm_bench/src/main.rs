#![no_std]
#![no_main]

extern crate cortex_m;
use cortex_m_rt::entry;

mod alloc;
mod bytes;

use spacewasm::{
    CodeBuilder, CompilerOptions, Engine, ExportDesc, HostFunction, HostModule, InterpreterResult,
    InterpreterRunner, ModuleRef, PageAllocator, Ref, StartInvocation, Value, WasmRef,
};

use core::ops::ControlFlow;

use crate::bytes::ByteStream;

spacewasm::global_allocator!(
    PageAllocator<16>,
    PageAllocator::new(&alloc::BareMetalAllocator {}, 8192)
);

const MAX_CODE_PAGES: u32 = 32;
const MAX_CONTROL_FRAMES: usize = 64;
const MAX_STACK_DEPTH: usize = 256;

#[entry]
fn main() -> ! {
    let env = HostModule {
        name: "env".into(),
        globals: spacewasm::vec![],
        functions: spacewasm::vec![HostFunction::new(
            "clock_ms",
            "".into(),
            "I".into(),
            |_, _| {
                // let ms = std::time::SystemTime::now()
                //     .duration_since(std::time::UNIX_EPOCH)
                //     .unwrap()
                //     .as_millis() as i64;
                let ms = 0;
                ControlFlow::Continue(Some(Value::I64(ms * 1000)))
            },
        )],
        memory: spacewasm::Vec::zero(),
        table: spacewasm::Vec::zero(),
    };

    let mut state = Engine::new(1024, 2, spacewasm::Vec::from_array([env]).unwrap()).unwrap();
    let mut code_builder = CodeBuilder::new(CompilerOptions {
        allow_memory_grow: false,
        max_backpatch_iterations: 0,
        max_code_pages: MAX_CODE_PAGES,
    })
    .unwrap();

    let bytes = include_bytes!("coremark-minimal.wasm");

    let module = spacewasm::Module::new::<MAX_CONTROL_FRAMES, MAX_STACK_DEPTH>(
        "coremark",
        &mut ByteStream::new(bytes),
        &mut state.store,
        &mut code_builder,
        spacewasm::Rc::new(alloc::BareMetalAllocator)
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

    let mut result = InterpreterResult::OutOfFuel;
    while result == InterpreterResult::OutOfFuel {
        result = spacewasm::Interpreter.run(&text, &mut state, usize::MAX);
    }

    match result {
        InterpreterResult::Finished => {}
        _ => {}
    }
    // println!("done!!");
    loop {}
}
use core::panic::PanicInfo;
// Panic handler: called when a panic occurs.
// Since this is a bare-metal environment, it enters an infinite loop.
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}