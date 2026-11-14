//! A backtrace taken from a real run, resolved to the lines that made each call.
//!
//! `main` calls `f`, `f` calls `g`, and `g` calls the host, which pauses. The embedder
//! takes the engine's backtrace there, as `spacewasm_backtrace` would on board, and the
//! test resolves it through a symbol file split from the same module.

use spacewasm::{
    CodeBuilder, CompilerOptions, Engine, ExportDesc, HostFunction, HostFunctionBreak, HostModule,
    Interpreter, InterpreterResult, InterpreterRunner, PageAllocator, Ref, WasmRef,
};
use spacewasm_tools::{DecoderLimits, Symbols, host_modules_for, ir_map, resolve_pcs, split};
use std::ops::ControlFlow;
use std::path::Path;

spacewasm::global_allocator!(PageAllocator<System, 64>, PageAllocator::new(System, 64 * 1024));

struct System;

/// `spacewasm`'s allocator is process-global and single-threaded by contract, and libtest
/// runs tests on parallel threads: everything that decodes holds this.
static DECODER: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn decoder() -> std::sync::MutexGuard<'static, ()> {
    DECODER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

// SAFETY: forwards to the Rust global allocator.
unsafe impl spacewasm::Allocator for System {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> Result<*mut u8, spacewasm::AllocError> {
        // SAFETY: the caller guarantees a non-zero size.
        let ptr = unsafe { std::alloc::alloc(layout) };
        match ptr.is_null() {
            true => Err(spacewasm::AllocError::AllocationFailed),
            false => Ok(ptr),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        // SAFETY: `ptr` came from `alloc` with `layout`.
        unsafe { std::alloc::dealloc(ptr, layout) }
    }
}

/// Line numbers below are what the test expects each frame to resolve to.
const SOURCE: &str = "\
(module
  (import \"env\" \"probe\" (func $probe))
  (func $g
    call $probe)
  (func $f
    nop
    call $g)
  (func (export \"main\")
    call $f))
";
const PROBE_LINE: u32 = 4;
const CALL_G_LINE: u32 = 7;
const CALL_F_LINE: u32 = 9;

fn module_with_dwarf() -> Vec<u8> {
    let mut parser = wat::Parser::new();
    parser.generate_dwarf(wat::GenerateDwarf::Lines);
    parser
        .parse_str(Some(Path::new("chain.wat")), SOURCE)
        .expect("the fixture is valid WebAssembly text")
}

fn options() -> CompilerOptions {
    CompilerOptions::new(true, None, 16)
}

/// Run `uplinked` until its host call pauses and return the backtrace taken there.
fn flight_backtrace(uplinked: &[u8]) -> Vec<u32> {
    let probe = HostFunction::new("probe", "".into(), "".into(), |_, _| {
        ControlFlow::Break(HostFunctionBreak::Pause)
    });
    let host = HostModule {
        name: "env".into(),
        globals: spacewasm::vec![],
        functions: spacewasm::vec![probe],
        memory: spacewasm::vec![],
        table: spacewasm::vec![],
    };

    let mut engine = Engine::new(1024, 1, spacewasm::vec![host]).unwrap();
    let mut code = CodeBuilder::new(options()).unwrap();
    let mut control_frames = [spacewasm::ControlFrame::EMPTY; 64];
    let mut operands = [spacewasm::OperandType::EMPTY; 256];
    let module = spacewasm::Module::new(
        "chain",
        &mut Slice(uplinked, false),
        &mut engine.store,
        &mut code,
        &mut spacewasm::DecoderStack::new(&mut control_frames, &mut operands),
        spacewasm::Rc::new(NoMemory)
            .unwrap()
            .into_wasm_memory_allocator(),
    )
    .unwrap();
    let main = module
        .exports
        .iter()
        .find_map(|export| match export.desc {
            ExportDesc::Func(index) if export.name == "main" => Some(index),
            _ => None,
        })
        .unwrap();
    let Some(Ref::Module(index)) = module.get_func_ref(main) else {
        panic!("main is a module function");
    };
    let module = engine.push_module(module).unwrap();
    engine.invoke(WasmRef { module, index }, &[]).unwrap();
    assert_eq!(
        Interpreter.run(code.pages(), &mut engine, 1000),
        InterpreterResult::Pause
    );

    let backtrace = engine.backtrace().expect("paused in the host call");
    backtrace.map(|pc| pc.0).collect()
}

#[test]
fn a_host_call_backtrace_names_each_calling_line() {
    let _decoder = decoder();
    let scratch = std::env::temp_dir();
    let split = split(&module_with_dwarf(), None, &scratch).unwrap();
    let symbols = Symbols::parse(&split.symbols).unwrap();
    assert!(symbols.describes(&split.module));

    let backtrace = flight_backtrace(&split.module);
    assert_eq!(backtrace.len(), 3, "g, f and main: {backtrace:x?}");

    let hosts = host_modules_for(symbols.module()).unwrap();
    let map = ir_map(symbols.module(), options(), hosts, DecoderLimits::default()).unwrap();
    // Paused in a host call, every entry is a `call` itself: nothing to adjust.
    let resolved = resolve_pcs(&symbols, &map, &backtrace).unwrap();

    let lines: Vec<u32> = resolved
        .iter()
        .map(|entry| {
            let frame = entry.frames.first().expect("every call has a line");
            frame.location.as_ref().unwrap().line
        })
        .collect();
    assert_eq!(lines, [PROBE_LINE, CALL_G_LINE, CALL_F_LINE]);
    for entry in &resolved {
        let location = entry.frames[0].location.as_ref().unwrap();
        assert!(location.file.ends_with("chain.wat"), "{}", location.file);
    }
}

/// The stand-in host modules must compile host calls exactly as the real ones did, or
/// every PC after the first host call would land on the wrong instruction.
#[test]
fn stand_in_hosts_replay_the_same_layout() {
    let _decoder = decoder();
    let module = wat::parse_str(
        r#"(module
             (import "env" "a" (func $a (param i32) (result i32)))
             (import "env" "g" (global $g (mut i64)))
             (import "other" "b" (func $b (param f64)))
             (func (export "main")
               i32.const 300 call $a drop
               f64.const 1 call $b
               global.get $g drop))"#,
    )
    .unwrap();
    let hosts = host_modules_for(&module).unwrap();
    assert_eq!(hosts.len(), 2, "one per import module, `env` and `other`");
    let map = ir_map(&module, options(), hosts, DecoderLimits::default()).unwrap();
    assert!(!map.is_empty());
}

/// `wasm-opt` reorders and rewrites; a backtrace through an optimised module still has to
/// land on the line that made each call. Skipped where `wasm-opt` is not installed.
#[test]
fn an_optimised_module_resolves_the_same_lines() {
    let _decoder = decoder();
    if std::process::Command::new("wasm-opt")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("wasm-opt not found; skipping");
        return;
    }
    let scratch = tempdir();
    let optimizer = spacewasm_tools::Optimizer {
        program: "wasm-opt".into(),
        // `-O1`, not `-Os`: the latter inlines `f` and `g` into `main`, which is correct
        // but leaves one frame and nothing to check the walk against.
        args: ["--mvp-features", "-O1"].map(String::from).to_vec(),
    };
    let split = split(&module_with_dwarf(), Some(&optimizer), &scratch).unwrap();
    let symbols = Symbols::parse(&split.symbols).unwrap();

    let backtrace = flight_backtrace(&split.module);
    let hosts = host_modules_for(symbols.module()).unwrap();
    let map = ir_map(symbols.module(), options(), hosts, DecoderLimits::default()).unwrap();
    let resolved = resolve_pcs(&symbols, &map, &backtrace).unwrap();
    let lines: Vec<u32> = resolved
        .iter()
        .map(|entry| entry.frames[0].location.as_ref().unwrap().line)
        .collect();
    assert_eq!(lines, [PROBE_LINE, CALL_G_LINE, CALL_F_LINE]);
    std::fs::remove_dir_all(&scratch).ok();
}

fn tempdir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("spacewasm-tools-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The whole module as one chunk.
struct Slice<'a>(&'a [u8], bool);

impl spacewasm::WasmStream for Slice<'_> {
    fn read(&mut self) -> Result<Option<spacewasm::InnerVec<u8>>, u8> {
        if self.1 {
            return Ok(None);
        }
        self.1 = true;
        // SAFETY: the decoder only reads through the view, and `self.0` outlives it.
        Ok(Some(unsafe {
            spacewasm::InnerVec::from_raw_parts(
                self.0.as_ptr().cast_mut(),
                self.0.len(),
                self.0.len(),
            )
        }))
    }

    fn return_(&mut self, _: spacewasm::InnerVec<u8>) {}
}

/// The fixture declares no memory, so none is ever allocated.
struct NoMemory;

impl spacewasm::WasmMemoryAllocator for NoMemory {
    fn allocate(
        &self,
        _: std::alloc::Layout,
    ) -> Result<std::ptr::NonNull<u8>, spacewasm::AllocError> {
        Err(spacewasm::AllocError::AllocationFailed)
    }

    fn reallocate(
        &self,
        _: std::ptr::NonNull<u8>,
        _: std::alloc::Layout,
        _: std::alloc::Layout,
    ) -> Result<std::ptr::NonNull<u8>, spacewasm::AllocError> {
        Err(spacewasm::AllocError::AllocationFailed)
    }

    fn deallocate(&self, _: std::ptr::NonNull<u8>, _: std::alloc::Layout) {}
}
