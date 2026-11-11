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
use util::RustSystemAllocator as Allocator;

#[cfg(target_os = "none")]
mod alloc;
#[cfg(target_os = "none")]
use {alloc::BareMetalAllocator as Allocator, bench::entry, semihosting::io::Write};

use spacewasm::{
    CodeBuilder, CompilerOptions, Engine, ExportDesc, HostFunction, HostModule, Interpreter,
    InterpreterResult, InterpreterRunner, InvokeError, ModuleRef, PageAllocator, RawValue, Ref,
    TrapReason, UnwrapNoFmt, Value, WasmRef,
};

const STACK_SIZE: usize = 1024;
const MAX_PAGES: usize = 256;
const MAX_CONTROL_FRAMES: usize = 64;
const MAX_STACK_DEPTH: usize = 256;

/// The coremark module, placed in its own `.wasm_module` section on embedded
/// targets so it is not counted towards `.rodata`.
#[cfg_attr(target_os = "none", unsafe(link_section = ".wasm_module"))]
static COREMARK_WASM: [u8; include_bytes!("coremark.wasm").len()] =
    *include_bytes!("coremark.wasm");

spacewasm::global_allocator!(
    PageAllocator<Allocator, MAX_PAGES>,
    PageAllocator::new(Allocator, 8192)
);

/// Runs coremark and returns the score along with the number of bytes
/// handed out by SpaceWasm's global allocator. Wasm linear memory is
/// allocated separately and is not included in the heap usage.
fn coremark() -> (f32, u32) {
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

    let mut code_builder =
        CodeBuilder::new(CompilerOptions::new(true, None, MAX_PAGES)).unwrap_nofmt();
    let mut engine = Engine::new(STACK_SIZE, 1, spacewasm::vec![env]).unwrap_nofmt();

    let mut file_stream = bytes::ByteStream::new(&COREMARK_WASM);

    let module = spacewasm::Module::new::<MAX_CONTROL_FRAMES, MAX_STACK_DEPTH>(
        "main",
        &mut file_stream,
        &mut engine.store,
        &mut code_builder,
        spacewasm::Rc::new(Allocator)
            .unwrap_nofmt()
            .into_wasm_memory_allocator(),
    )
    .unwrap_nofmt();

    let module_ref = engine.push_module(module).unwrap_nofmt();
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
        .unwrap_nofmt();

    let mut result = InterpreterResult::OutOfFuel;
    while result == InterpreterResult::OutOfFuel {
        result = Interpreter.run(code_builder.pages(), &mut engine, usize::MAX);
    }

    // Sample the heap before `engine` and `code_builder` are dropped and
    // return their pages to the allocator.
    let heap_bytes = unsafe { (*GLOBAL_ALLOCATOR).stats() }.total_bytes;

    let score = engine.result.unwrap_or(RawValue::from_32(0)).read_f32();
    (score, heap_bytes)
}

#[cfg_attr(target_os = "none", entry)]
fn entrypoint() -> ! {
    #[cfg(target_os = "none")]
    alloc::init();

    bench::clock_setup();

    let (score, heap_bytes) = coremark();
    print_results(score, heap_bytes);

    bench::exit(0);
}

/// Print the score and heap usage on separate lines without `core::fmt`.
/// Float formatting pulls in the grisu/dragon `flt2dec` machinery which
/// dominates `.text` on embedded targets.
#[cfg(target_os = "none")]
fn print_results(score: f32, heap_bytes: u32) {
    if let Ok(mut stdout) = semihosting::io::stdout() {
        let mut buf = [0u8; 32];
        let _ = stdout.write_all(write_decimal(
            &mut buf,
            (score as f64 * 1e6 + 0.5) as u64,
            6,
        ));
        let _ = stdout.write_all(b"\n");
        let _ = stdout.write_all(write_decimal(&mut buf, heap_bytes as u64, 0));
    }
}

/// Write `value / 10^frac_digits` as a fixed-point decimal into the tail of
/// `buf`, returning the written slice.
#[cfg(target_os = "none")]
fn write_decimal(buf: &mut [u8; 32], value: u64, frac_digits: usize) -> &[u8] {
    let mut i = buf.len();
    let mut n = value;
    let mut digits = 0;
    while n > 0 || digits <= frac_digits {
        if digits == frac_digits && frac_digits > 0 {
            i -= 1;
            buf[i] = b'.';
        }
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        digits += 1;
    }

    &buf[i..]
}

#[cfg(not(target_os = "none"))]
fn print_results(score: f32, heap_bytes: u32) {
    print!("{score}\n{heap_bytes}");
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
