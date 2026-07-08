use spacewasm::{
    CodeBuilder, CompilerOptions, ExportDesc,
    InterpreterResult, InterpreterRunner, ModuleRef, PageAllocator, Ref,
    WasmRef,
};

mod wasi_preview1;

use spacewasm_util::{FileStream, RustSystemAllocator};
use wasi_common::sync::{Dir, WasiCtxBuilder, ambient_authority};

use crate::wasi_preview1::make_wasi_preview1_module;

use clap::Parser;

spacewasm::global_allocator!(
    PageAllocator<512>,
    PageAllocator::new(&RustSystemAllocator {}, 1024 * 1024 * 32)
);

const MAX_PAGES: usize = 1024 * 32;
const MAX_CONTROL_FRAMES: usize = 512;
const MAX_STACK_DEPTH: usize = 256;
const STACK_SIZE: usize = 1024 * 1024;

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

    let mut wasi_ctx_builder: WasiCtxBuilder = WasiCtxBuilder::new();
    
    // set argv
    let _ = wasi_ctx_builder.arg(&args.argv0.unwrap_or(args.file.clone()));
    let _ = wasi_ctx_builder.args(&args.args);

    // set env
    if args.inherit_env.unwrap_or(false) {
        let _ = wasi_ctx_builder.inherit_env();
    }
    for env in args.env {
        if env.contains("=") {
            let mut split = env.splitn(2, "=");
            let _ = wasi_ctx_builder.env(split.next().unwrap_or(""), split.next().unwrap_or(""));
        } else {
            let _ = wasi_ctx_builder.env(&env, &std::env::var(&env).unwrap_or("".to_owned()));
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
        println!("{host_dir} mapped to {guest_dir}");


        match Dir::open_ambient_dir(&host_dir, ambient_authority()) {
            Ok(opened_dir) => {
                let _ = wasi_ctx_builder.preopened_dir(opened_dir, guest_dir);
            }
            Err(error) => {
                panic!("cannot open host directory {host_dir}: {error}")
            }
        }
    }
    
    if args.cwd_is_root.unwrap_or(false) {
        match Dir::open_ambient_dir(".", ambient_authority()) {
            Ok(opened_dir) => {
                let _ = wasi_ctx_builder.preopened_dir(opened_dir, "/");
            }
            Err(error) => {
                panic!("error mounting cwd as root: {error}")
            }
        }
    }

    let preview1_module = make_wasi_preview1_module(wasi_ctx_builder.build());


    let mut code_builder = CodeBuilder::<MAX_PAGES>::default();
    let mut store = spacewasm::Store::new(1, [preview1_module]).unwrap();

    let file = std::fs::File::open(args.file).expect("failed to open file");
    let mut file_stream = FileStream::new(file);

    let module = spacewasm::Module::new::<MAX_PAGES, MAX_CONTROL_FRAMES, MAX_STACK_DEPTH>(
            "main",
            &mut file_stream,
            &mut store,
            &mut code_builder,
            spacewasm::Rc::new(RustSystemAllocator)
                .unwrap()
                .into_wasm_memory_allocator(),
            CompilerOptions {allow_memory_grow: true},
        )
        .expect("failed to parse wasm module");

    let (text, _) = code_builder.finish().unwrap();

    let mut state = store.allocate(STACK_SIZE).unwrap();
    match state.initialize_module(module, &text, usize::MAX) {
        InterpreterResult::Finished => {}
        InterpreterResult::OutOfFuel => panic!("insufficient fuel for initialization"),
        InterpreterResult::Trap(t) => panic!("trap during initialization {t:?}"),
        InterpreterResult::ReaderError(e) => panic!("ir reader error {e:?}"),
        InterpreterResult::Pause => panic!("pause during init"),
    }

    let module: &spacewasm::Module = state.store.modules().last().unwrap();

    let fi = {
        let f = module.exports.iter().find(|f| &f.name == "_start").unwrap();
        let ExportDesc::Func(fi) = f.desc else {panic!()};
        fi
    };

    let Ref::Module(fi) = module.get_func_ref(fi).unwrap() else {panic!("error: the provided wasm module does not correctly export a _start function")};

    state.invoke(WasmRef {module: ModuleRef(0),index: fi,},&[]).unwrap();

    let interpreter = spacewasm::Interpreter::default();

    let mut result = InterpreterResult::OutOfFuel;
    while result == InterpreterResult::OutOfFuel {
        result = interpreter.run(&text, &mut state, usize::MAX)
    }

    let InterpreterResult::Finished = result else {
        panic!("interpreter failed: {:?}", result)
    };
}
