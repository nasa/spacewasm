#![cfg_attr(not(any(target_arch = "aarch64", target_arch = "x86_64")), no_main)]
#![cfg_attr(not(any(target_arch = "aarch64", target_arch = "x86_64")), no_std)]
use core::ops::ControlFlow;

#[cfg_attr(target_arch = "arm", path = "arm.rs")]
#[cfg_attr(
    any(target_arch = "riscv64", target_arch = "riscv32"),
    path = "riscv.rs"
)]
#[cfg_attr(
    any(target_arch = "aarch64", target_arch = "x86_64"),
    path = "linux.rs"
)]
mod bench;
mod bytes;

#[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
use spacewasm_util::RustSystemAllocator as Allocator;

#[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
mod alloc;
#[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
use {alloc::BareMetalAllocator as Allocator, bench::entry, semihosting::print};

use spacewasm::{
    CodeBuilder, CompilerOptions, Engine, ExportDesc, HostFunction, HostModule, Interpreter,
    InterpreterResult, InterpreterRunner, InvokeError, ModuleRef, PageAllocator, RawValue, Ref,
    TrapReason, Value, WasmRef,
};

const STACK_SIZE: usize = 1024;
const MAX_PAGES: usize = 256;
const MAX_CONTROL_FRAMES: usize = 64;
const MAX_STACK_DEPTH: usize = 256;

spacewasm::global_allocator!(
    PageAllocator<Allocator, MAX_PAGES>,
    PageAllocator::new(Allocator, 8192)
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
                let ms = bench::clock_get_ms();

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

    let mut file_stream = bytes::ByteStream::new(include_bytes!("coremark.wasm"));

    let module = spacewasm::Module::new::<MAX_CONTROL_FRAMES, MAX_STACK_DEPTH>(
        "main",
        &mut file_stream,
        &mut engine.store,
        &mut code_builder,
        spacewasm::Rc::new(Allocator)
            .unwrap()
            .into_wasm_memory_allocator(),
    )
    .unwrap();

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
        _ => panic!(),
    }

    let module: &spacewasm::Module = engine.store.modules().last().unwrap();

    let fi = {
        let f = module.exports.iter().find(|f| &f.name == "run").unwrap();
        let ExportDesc::Func(fi) = f.desc else {
            panic!()
        };
        fi
    };

    let Ref::Module(fi) = module.get_func_ref(fi).unwrap() else {
        panic!()
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

#[cfg_attr(not(any(target_arch = "aarch64", target_arch = "x86_64")), entry)]
fn entrypoint() -> ! {
    bench::init_allocator();

    bench::clock_setup();

    let result: f32 = coremark();
    print!("{result}");

    bench::exit(0);
}

#[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
fn main() {
    entrypoint();
}
