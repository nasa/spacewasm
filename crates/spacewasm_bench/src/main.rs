#![no_main]
#![no_std]

use core::ops::ControlFlow;

use panic_semihosting as _;

use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use rtic_monotonics::systick::prelude::*;

mod alloc;
use alloc::*;
mod bytes;

use spacewasm::{
    CodeBuilder, CompilerOptions, Engine, ExportDesc, HostFunction, HostModule, Interpreter,
    InterpreterResult, InterpreterRunner, InvokeError, ModuleRef, PageAllocator, RawValue, Ref,
    TrapReason, Value, WasmRef,
};

use crate::bytes::ByteStream;

const STACK_SIZE: usize = 1024;
const MAX_PAGES: usize = 256;
const MAX_CONTROL_FRAMES: usize = 64;
const MAX_STACK_DEPTH: usize = 256;

systick_monotonic!(Mono, 1_000);

spacewasm::global_allocator!(
    PageAllocator<BareMetalAllocator, MAX_PAGES>,
    PageAllocator::new(BareMetalAllocator, 8192)
);

fn coremark() -> f32 {
    let env = HostModule {
        name: "env".into(),
        globals: spacewasm::vec![],
        functions: spacewasm::vec![HostFunction::new(
            "clock_ms",
            "".into(),
            "I".into(),
            |_, _| {
                let ms = Mono::now().ticks() as i64;

                ControlFlow::Continue(Some(Value::I64(ms)))
            },
        )],
        memory: spacewasm::Vec::zero(),
        table: spacewasm::Vec::zero(),
    };

    let mut code_builder = CodeBuilder::new(CompilerOptions {
        allow_memory_grow: true,
        max_backpatch_iterations: 0,
        max_code_pages: MAX_PAGES as u32,
    })
    .unwrap();
    let mut engine = Engine::new(STACK_SIZE, 1, spacewasm::vec![env]).unwrap();

    let mut file_stream = ByteStream::new(include_bytes!("coremark.wasm"));

    let module = spacewasm::Module::new::<MAX_CONTROL_FRAMES, MAX_STACK_DEPTH>(
        "main",
        &mut file_stream,
        &mut engine.store,
        &mut code_builder,
        spacewasm::Rc::new(BareMetalAllocator)
            .unwrap()
            .into_wasm_memory_allocator(),
    )
    .expect("failed to parse WASM module");

    let module_ref = engine.push_module(module).unwrap();
    let init_result = match engine.module_start(module_ref) {
        None => InterpreterResult::Finished,
        Some(start) => match engine.invoke(start, &[]) {
            Ok(()) => Interpreter.run(code_builder.pages(), &mut engine, usize::MAX),
            Err(InvokeError::StackOverflow) => InterpreterResult::Trap(TrapReason::StackOverflow),
            Err(_) => unreachable!(),
        },
    };
    match init_result {
        InterpreterResult::Finished => {}
        InterpreterResult::OutOfFuel => {
            panic!("insufficient fuel for initialization");
        }
        InterpreterResult::Trap(t) => {
            panic!("trap during initialization {t:?}");
        }
        InterpreterResult::Pause => {
            panic!("pause during init");
        }
    }

    let module: &spacewasm::Module = engine.store.modules().last().unwrap();

    let fi = {
        let f = module.exports.iter().find(|f| &f.name == "run").unwrap();
        let ExportDesc::Func(fi) = f.desc else {
            panic!("error: the provided wasm module does not correctly export a run function");
        };
        fi
    };

    let Ref::Module(fi) = module.get_func_ref(fi).unwrap() else {
        panic!("error: the provided wasm module does not correctly export a run function");
    };

    engine
        .invoke(
            WasmRef {
                module: ModuleRef(0),
                index: fi,
            },
            &[],
        )
        .unwrap();

    let mut result = InterpreterResult::OutOfFuel;
    while result == InterpreterResult::OutOfFuel {
        result = Interpreter.run(code_builder.pages(), &mut engine, usize::MAX);
    }

    engine.result.unwrap_or(RawValue::from_32(0)).read_f32()
}

#[entry]
fn main() -> ! {
    let p = cortex_m::Peripherals::take().unwrap();
    Mono::start(p.SYST, 1_000_000_000);

    let result: f32 = coremark();

    hprintln!("{result}");

    debug::exit(debug::EXIT_SUCCESS);

    loop {}
}
