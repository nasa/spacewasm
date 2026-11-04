#![cfg_attr(target_os = "none", no_main)]
#![cfg_attr(target_os = "none", no_std)]

/// An embedding of the coremark benchmark in SpaceWasm, for desktop and embedded
/// environments (*nix x86_64/aarch64, and bare-metal arm/riscv32/riscv64)
///
/// Copyright 2026 California Institute of Technology
///
/// Licensed under the Apache License, Version 2.0 (the "License");
/// you may not use this file except in compliance with the License.
/// You may obtain a copy of the License at
///
/// <http://www.apache.org/licenses/LICENSE-2.0>
///
/// ---
/// Portions of this file are derived from <https://github.com/taiki-e/semihosting>
/// and the semihosting crate developed by Taiki Endo
use core::ops::ControlFlow;

#[cfg_attr(all(target_arch = "arm", target_os = "none"), path = "arm.rs")]
#[cfg_attr(
    any(target_arch = "riscv64", target_arch = "riscv32"),
    path = "riscv.rs"
)]
#[cfg_attr(not(target_os = "none"), path = "linux.rs")]
mod bench;
mod bytes;

#[cfg(not(target_os = "none"))]
use spacewasm_util::RustSystemAllocator as Allocator;

#[cfg(target_os = "none")]
mod alloc;
#[cfg(target_os = "none")]
use {alloc::BareMetalAllocator as Allocator, bench::entry, semihosting::io::Write};

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
        max_backpatch_iterations: None,
        max_code_pages: MAX_PAGES,
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

#[cfg_attr(target_os = "none", entry)]
fn entrypoint() -> ! {
    #[cfg(target_os = "none")]
    alloc::init();

    bench::clock_setup();

    let result: f32 = coremark();
    print_score(result);

    bench::exit(0);
}

/// Print the score as fixed-point decimal without `core::fmt`. Float formatting
/// pulls in the grisu/dragon `flt2dec` machinery which dominates `.text` on
/// embedded targets.
#[cfg(target_os = "none")]
fn print_score(score: f32) {
    const FRAC_DIGITS: usize = 6;

    let scaled = (score as f64 * 1e6 + 0.5) as u64;
    let mut buf = [0u8; 32];
    let mut i = buf.len();
    let mut n = scaled;
    let mut digits = 0;
    while n > 0 || digits <= FRAC_DIGITS {
        if digits == FRAC_DIGITS {
            i -= 1;
            buf[i] = b'.';
        }
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        digits += 1;
    }

    if let Ok(mut stdout) = semihosting::io::stdout() {
        let _ = stdout.write_all(&buf[i..]);
    }
}

#[cfg(not(target_os = "none"))]
fn print_score(score: f32) {
    print!("{score}");
}

/// Exit without formatting the panic message: `PanicInfo`'s `Display` impl
/// drags in `core::fmt` and every `Debug` impl reachable from an `unwrap()`.
#[cfg(target_os = "none")]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    bench::exit(101);
}

#[cfg(not(target_os = "none"))]
fn main() {
    entrypoint();
}
