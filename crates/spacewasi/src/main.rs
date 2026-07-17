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

use spacewasm::{
    CodeBuilder, CompilerOptions, ExportDesc, InterpreterResult, InterpreterRunner, ModuleRef,
    PageAllocator, Ref, WasmRef,
};
mod wasi_preview1;
use crate::wasi_preview1::make_wasi_preview1_module;
use clap::error::ErrorKind;
use clap::{CommandFactory, Parser};
use spacewasm_util::{FileStream, RustSystemAllocator};
use wasi_common::sync::{Dir, WasiCtxBuilder, ambient_authority};

spacewasm::global_allocator!(
    PageAllocator<0x200>,
    PageAllocator::new(&RustSystemAllocator {}, 0x2_000_000)
);

const MAX_PAGES: usize = 0x10_000;
const MAX_CONTROL_FRAMES: usize = 0x1_000;
const MAX_STACK_DEPTH: usize = 0x400;
const STACK_SIZE: usize = 0x100_000;

/// Execute WASI-compatible WASM modules with spacewasm
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

    /// Module filepath
    file: String,

    /// Raw arguments passed on to the module
    #[clap(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
}

fn main() {
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

    let preview1_module = make_wasi_preview1_module(wasi_ctx_builder.build());

    let mut code_builder = CodeBuilder::<MAX_PAGES>::default();
    let mut store = spacewasm::Store::new(1, [preview1_module]).unwrap();

    let Ok(file) = std::fs::File::open(args.file) else {
        cmd.error(ErrorKind::InvalidValue, "wasm module path does not exist")
            .exit();
    };
    let mut file_stream = FileStream::new(file);

    let Ok(module) = spacewasm::Module::new::<MAX_PAGES, MAX_CONTROL_FRAMES, MAX_STACK_DEPTH>(
        "main",
        &mut file_stream,
        &mut store,
        &mut code_builder,
        spacewasm::Rc::new(RustSystemAllocator)
            .unwrap()
            .into_wasm_memory_allocator(),
        CompilerOptions {
            allow_memory_grow: true,
        },
    ) else {
        eprintln!("failed to parse WASM module");
        std::process::exit(1);
    };

    let (text, _) = code_builder.finish().unwrap();

    let mut state = store.allocate(STACK_SIZE).unwrap();
    match state.initialize_module(module, &text, usize::MAX) {
        InterpreterResult::Finished => {}
        InterpreterResult::OutOfFuel => {
            eprintln!("insufficient fuel for initialization");
            std::process::exit(1);
        }
        InterpreterResult::Trap(t) => {
            eprintln!("trap during initialization {t:?}");
            std::process::exit(1);
        }
        InterpreterResult::ReaderError(e) => {
            eprintln!("ir reader error {e:?}");
            std::process::exit(1);
        }
        InterpreterResult::Pause => {
            eprintln!("pause during init");
            std::process::exit(1);
        }
    }

    let module: &spacewasm::Module = state.store.modules().last().unwrap();

    let fi = {
        let f = module.exports.iter().find(|f| &f.name == "_start").unwrap();
        let ExportDesc::Func(fi) = f.desc else {
            eprintln!(
                "error: the provided wasm module does not correctly export a _start function"
            );
            std::process::exit(1);
        };
        fi
    };

    let Ref::Module(fi) = module.get_func_ref(fi).unwrap() else {
        eprintln!("error: the provided wasm module does not correctly export a _start function");
        std::process::exit(1);
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

    // TODO(cbwilson) Need to enable raw terminal mode somehow for TTY escape codes and control sequences

    let mut result = InterpreterResult::OutOfFuel;
    while result == InterpreterResult::OutOfFuel {
        result = interpreter.run(&text, &mut state, usize::MAX)
    }

    let InterpreterResult::Finished = result else {
        eprintln!("interpreter failed: {:?}", result);
        std::process::exit(1);
    };
}
