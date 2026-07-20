#![no_std]
#![no_main]

extern crate cortex_m;

mod bytes;
mod alloc;

use spacewasm::{
    CodeBuilder, CompilerOptions, ExportDesc, HostFunction, HostModule, InterpreterResult,
    InterpreterRunner, ModuleRef, PageAllocator, Ref, Store, Value, WasmRef,
};

use core::ops::ControlFlow;
spacewasm::global_allocator!(
    PageAllocator<16>,
    PageAllocator::new(&alloc::BareMetalAllocator {}, 8192)
);

const MAX_CODE_PAGES: usize = 32;
const MAX_CONTROL_FRAMES: usize = 64;
const MAX_STACK_DEPTH: usize = 256;



fn main() {
    alloc::init_alloc();
    let env = HostModule {
        name: "env",
        globals: spacewasm::vec![],
        functions: spacewasm::vec![HostFunction::new(
            "clock_ms",
            "".into(),
            "I".into(),
            |_, _| {
                ControlFlow::Continue(Some(Value::I64(0)))
            },
        )],
        memory: spacewasm::Vec::zero(),
        table: spacewasm::Vec::zero(),
    };

    let mut store = Store::new(2, [env]).unwrap();
    let mut code_builder = CodeBuilder::<MAX_CODE_PAGES>::default();

    let bytes = include_bytes!("hello_universe.wasm");

    let module = spacewasm::Module::new::<MAX_CODE_PAGES, MAX_CONTROL_FRAMES, MAX_STACK_DEPTH>(
        "coremark",
        &mut bytes::ByteStream::new(bytes),
        &mut store,
        &mut code_builder,
        spacewasm::Rc::new(alloc::BareMetalAllocator)
            .unwrap()
            .into_wasm_memory_allocator(),
        CompilerOptions::default(),
    )
    .expect("failed to parse wasm module");

    let (text, _final_page_offset) = code_builder.finish().unwrap();

    let mut state = store.allocate(1024).unwrap();
    match state.initialize_module(module, &text, usize::MAX) {
        InterpreterResult::Finished => {}
        InterpreterResult::OutOfFuel => panic!("insufficient fuel for initialization"),
        InterpreterResult::Trap(t) => panic!("trap during initialization {t:?}"),
        InterpreterResult::ReaderError(e) => panic!("ir reader error {e:?}"),
        InterpreterResult::Pause => panic!("pause during init"),
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

    let interpreter = spacewasm::Interpreter::default();
    let mut result = InterpreterResult::OutOfFuel;
    while result == InterpreterResult::OutOfFuel {
        result = interpreter.run(&text, &mut state, usize::MAX)
    }

    match result {
        InterpreterResult::Finished => {}
        _ => {}
    }
}
use core::panic::PanicInfo;


// Entry point of the OS, called by the bootloader.
// Initializes the kernel and starts the main loop.
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
  main();
  loop {}
}
// Panic handler: called when a panic occurs.
// Since this is a bare-metal environment, it enters an infinite loop.
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
  loop {}
}