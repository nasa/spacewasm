/// An executable program to run WASI compatible modules
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
/// Portions of this file are derived from <https://github.com/bytecodealliance/wasmtime>
/// and the wasi-common crate developed by the wasmtime community.
///
/// Portions of this file are derived from <https://github.com/clap-rs/clap>:
/// Copyright (c) 2026 Knapp, K. B., & The Clap Community.
///
/// Portions of this file are derived from <https://github.com/crossterm-rs/crossterm>
use spacewasm::{
    CodeBuilder, CompilerOptions, Engine, ExportDesc, Interpreter, InterpreterResult,
    InterpreterRunner, InvokeError, PageAllocator, Ref, TrapReason, WasmRef,
};
mod wasi_preview1;
use crate::wasi_preview1::make_wasi_preview1_module;
use clap::error::ErrorKind;
use clap::{CommandFactory, Parser};
use spacewasm_util::{FileStream, RustSystemAllocator};
use std::process::ExitCode;
use wasi_common::sync::{Dir, WasiCtxBuilder, ambient_authority};

/// Restores the terminal from raw mode when dropped.
struct RawTtyGuard;

impl Drop for RawTtyGuard {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

spacewasm::global_allocator!(
    PageAllocator<RustSystemAllocator, 0x200>,
    PageAllocator::new(RustSystemAllocator, 0x8_000_000)
);

const MAX_PAGES: usize = 0x10_000;
const MAX_CONTROL_FRAMES: usize = 0x1_000;
const MAX_STACK_DEPTH: usize = 0x400;
const STACK_SIZE: usize = 0x100_000;

/// Execute WASI-compatible WASM modules with SpaceWasm
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Mount the current working directory as the root directory (/) in WASM
    #[arg(long, value_name = "CWD_IS_ROOT", action = clap::ArgAction::SetTrue)]
    cwd_is_root: Option<bool>,

    /// Override argv[0] value
    #[arg(long, value_name = "ARGV0")]
    argv0: Option<String>,

    /// Mount directories
    #[arg(short, long, value_name = "HOST_DIR[::WASM_DIR]")]
    dir: Vec<String>,

    /// Set environment variables
    #[arg(short, long, value_name = "KEY[=VALUE]")]
    env: Vec<String>,

    /// Inherit all environment variables
    #[arg(long, value_name = "INHERIT_ENV", action = clap::ArgAction::SetTrue)]
    inherit_env: Option<bool>,

    /// Enable raw terminal mode
    #[arg(long, value_name = "RAW_TTY", action = clap::ArgAction::SetTrue)]
    raw_tty: Option<bool>,

    /// Module filepath
    file: String,

    /// Raw arguments passed on to the module
    #[clap(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
}

fn main() -> ExitCode {
    let args = Args::parse();
    let mut cmd = Args::command();

    let mut wasi_ctx_builder: WasiCtxBuilder = WasiCtxBuilder::new();

    // set argv
    let Ok(_) = wasi_ctx_builder.arg(&args.argv0.unwrap_or(args.file.clone())) else {
        eprintln!("error setting argv[0]");
        std::process::exit(1);
    };
    let Ok(_) = wasi_ctx_builder.args(&args.args) else {
        eprintln!("error setting arguments");
        std::process::exit(1);
    };

    // set env
    if args.inherit_env.unwrap_or(false) {
        let Ok(_) = wasi_ctx_builder.inherit_env() else {
            eprintln!("error inheriting env");
            std::process::exit(1);
        };
    }
    for env in args.env {
        if env.contains("=") {
            let mut split = env.splitn(2, "=");
            let Ok(_) =
                wasi_ctx_builder.env(split.next().unwrap_or(""), split.next().unwrap_or(""))
            else {
                eprintln!("error setting env");
                std::process::exit(1);
            };
        } else {
            let Ok(_) = wasi_ctx_builder.env(&env, &std::env::var(&env).unwrap_or("".to_owned()))
            else {
                eprintln!("error setting env");
                std::process::exit(1);
            };
        }
    }

    wasi_ctx_builder.inherit_stdio();

    for dir in args.dir {
        let mut host_dir = dir.clone();
        let mut guest_dir = dir.clone();

        if dir.contains("::") {
            let mut split = dir.splitn(2, "::");
            host_dir = split.next().unwrap_or("").to_owned();
            guest_dir = split.next().unwrap_or("").to_owned();
        }

        match Dir::open_ambient_dir(&host_dir, ambient_authority()) {
            Ok(opened_dir) => {
                let Ok(_) = wasi_ctx_builder.preopened_dir(opened_dir, guest_dir) else {
                    eprintln!("cannot open preopened_dir in WASI context");
                    std::process::exit(1);
                };
            }
            Err(error) => {
                eprintln!("cannot open host directory {host_dir}: {error}");
                std::process::exit(1);
            }
        }
    }

    if args.cwd_is_root.unwrap_or(false) {
        match Dir::open_ambient_dir(".", ambient_authority()) {
            Ok(opened_dir) => {
                let Ok(_) = wasi_ctx_builder.preopened_dir(opened_dir, "/") else {
                    eprintln!("cannot open preopened_dir in WASI context");
                    std::process::exit(1);
                };
            }
            Err(error) => {
                eprintln!("error mounting cwd as root: {error}");
                std::process::exit(1);
            }
        }
    }

    let (preview1_module, exit_code) = make_wasi_preview1_module(wasi_ctx_builder.build());

    let mut code_builder = CodeBuilder::new(CompilerOptions {
        allow_memory_grow: true,
        max_backpatch_iterations: None,
        max_code_pages: MAX_PAGES as u32,
    })
    .unwrap();
    let mut engine = Engine::new(STACK_SIZE, 1, spacewasm::vec![preview1_module]).unwrap();

    let Ok(file) = std::fs::File::open(args.file) else {
        cmd.error(ErrorKind::InvalidValue, "wasm module path does not exist")
            .exit();
    };
    let mut file_stream = FileStream::new(file);

    let module = match spacewasm::Module::new::<MAX_CONTROL_FRAMES, MAX_STACK_DEPTH>(
        "main",
        &mut file_stream,
        &mut engine.store,
        &mut code_builder,
        spacewasm::Rc::new(RustSystemAllocator)
            .unwrap()
            .into_wasm_memory_allocator(),
    ) {
        Ok(module) => module,
        Err(error) => {
            eprintln!("failed to parse WASM module: {error:?}");
            std::process::exit(1);
        }
    };

    let module_ref = engine.push_module(module).unwrap();

    // Enable raw terminal mode (if requested)
    let _tty_guard = if args.raw_tty.unwrap_or(false) {
        if crossterm::terminal::enable_raw_mode().is_err() {
            eprintln!("error enabling raw terminal mode");
            return ExitCode::from(1);
        }
        Some(RawTtyGuard)
    } else {
        None
    };

    // Append the module and run its start function (if any). The interpreter
    // reads code directly from the builder's pages.
    let init_result = match engine.module_start(module_ref) {
        None => InterpreterResult::Finished,
        Some(start) => match engine.invoke(start, &[]) {
            Ok(()) => Interpreter.run(code_builder.pages(), &mut engine, usize::MAX),
            Err(InvokeError::StackOverflow) => InterpreterResult::Trap(TrapReason::StackOverflow),
            Err(_) => unreachable!(),
        },
    };
    // A guest may call `proc_exit` from its start function; honor the recorded
    // code before treating the resulting host trap as a failure.
    if let Some(code) = exit_code.get() {
        return ExitCode::from(code as u8);
    }
    match init_result {
        InterpreterResult::Finished => {}
        InterpreterResult::OutOfFuel => {
            eprintln!("insufficient fuel for initialization");
            return ExitCode::from(1);
        }
        InterpreterResult::Trap(t) => {
            eprintln!("trap during initialization {t:?}");
            return ExitCode::from(1);
        }
        InterpreterResult::Pause => {
            eprintln!("pause during init");
            return ExitCode::from(1);
        }
    }

    let module: &spacewasm::Module = engine.store.modules().last().unwrap();

    let fi = {
        let Some(f) = module.exports.iter().find(|f| &f.name == "_start") else {
            eprintln!(
                "error: the provided wasm module does not correctly export a _start function"
            );
            return ExitCode::from(1);
        };
        let ExportDesc::Func(fi) = f.desc else {
            eprintln!(
                "error: the provided wasm module does not correctly export a _start function"
            );
            return ExitCode::from(1);
        };
        fi
    };

    let Ref::Module(fi) = module.get_func_ref(fi).unwrap() else {
        eprintln!("error: the provided wasm module does not correctly export a _start function");
        return ExitCode::from(1);
    };

    let mut result = match engine.invoke(
        WasmRef {
            module: module_ref,
            index: fi,
        },
        &[],
    ) {
        Ok(()) => InterpreterResult::OutOfFuel,
        Err(InvokeError::StackOverflow) => InterpreterResult::Trap(TrapReason::StackOverflow),
        Err(_) => unreachable!(),
    };
    while result == InterpreterResult::OutOfFuel {
        result = Interpreter.run(code_builder.pages(), &mut engine, usize::MAX)
    }

    // If the guest called `proc_exit`, exit with the status it requested.
    if let Some(code) = exit_code.get() {
        return ExitCode::from(code as u8);
    }

    match result {
        InterpreterResult::Finished => ExitCode::SUCCESS,
        other => {
            eprintln!("interpreter failed: {other:?}");
            ExitCode::from(1)
        }
    }
}
