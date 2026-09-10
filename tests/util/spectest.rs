///
/// Copyright 2026 California Institute of Technology
///
/// Licensed under the Apache License, Version 2.0 (the "License");
/// you may not use this file except in compliance with the License.
/// You may obtain a copy of the License at
///
/// http://www.apache.org/licenses/LICENSE-2.0
///
/// ---
/// Portions of this file are derived from https://github.com/DLR-FT/wasm-interpreter:
/// Copyright © 2024-2026 Deutsches Zentrum für Luft- und Raumfahrt e.V.
/// (DLR).
/// Copyright © 2024-2025 OxidOS Automotive SRL.
use super::inspector::{Inspector, LimitedVec};
use core::panic;
use serde::{Deserialize, Serialize};
use spacewasm::{
    AllocError, Allocator, CodeBuilder, CompilerOptions, ConstantExprError, Engine, ExportDesc,
    GlobalValue, GlobalValueError, HostFunction, HostGlobal, HostModule, InnerVec, Interpreter,
    InterpreterResult, InterpreterRunner, InvokeError, Limit, Memory, MemoryError, Module,
    ModuleRef, ParseError, Ref, TrapReason, ValType, ValidationError, Value, WasmMemoryAllocator,
    WasmRef, WasmStream, global_allocator, vec,
};
use std::alloc::Layout;
use std::cell::RefCell;
use std::ops::ControlFlow;
use std::panic::catch_unwind;
use std::path::Path;
use std::path::PathBuf;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

#[cfg(not(miri))]
use std::process::Command as ProcessCommand;
#[cfg(not(miri))]
use std::sync::atomic::{AtomicU64, Ordering};

type SubtestLogType = Arc<Mutex<Option<Rc<RefCell<LimitedVec<String>>>>>>;

#[derive(Debug, Deserialize, Serialize)]
struct TestFile {
    source_filename: String,
    commands: Vec<Command>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
enum Command {
    Module {
        line: u32,
        #[serde(default)]
        name: Option<String>,
        filename: String,
    },
    AssertReturn {
        line: u32,
        action: Action,
        expected: Vec<ValueSpec>,
    },
    AssertTrap {
        line: u32,
        action: Action,
        text: String,
    },
    AssertUninstantiable {
        line: u32,
        filename: String,
        text: String,
        module_type: String,
    },
    AssertMalformed {
        line: u32,
        filename: String,
        text: String,
        module_type: String,
    },
    AssertInvalid {
        line: u32,
        filename: String,
        text: String,
        module_type: String,
    },
    AssertUnlinkable {
        line: u32,
        filename: String,
        text: String,
        module_type: String,
    },
    AssertExhaustion {
        line: u32,
        action: Action,
        text: String,
    },
    Register {
        line: u32,
        name: Option<String>,
        #[serde(rename = "as")]
        as_name: String,
    },
    Action {
        line: u32,
        action: Action,
    },
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
enum Action {
    Invoke {
        #[serde(default)]
        module: Option<String>,
        field: String,
        args: Vec<ValueSpec>,
    },
    Get {
        #[serde(default)]
        module: Option<String>,
        field: String,
    },
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct ValueSpec {
    #[serde(rename = "type")]
    ty: String,
    #[serde(default)]
    value: Option<String>,
}

/// The phase a test thread is currently in, controlling how the global
/// [`SpecTestAllocator`] treats allocations and deallocations.
#[derive(Clone, Copy)]
enum AllocPhase {
    /// No allocation check. Used during JSON parsing etc.
    Unchecked,
    /// Loading Wasm module. Allows allocation, does not allow deallocation
    Loading { freed: bool },
    /// No memory allocation may occur. During execution
    Locked,
}

thread_local! {
    /// Per-thread allocator phase. `cargo test` runs each `#[test]`
    /// concurrently in one process while sharing the single global
    /// [`SpecTestAllocator`]; keeping the phase thread-local means each test
    /// thread only gates its own allocations and cannot corrupt another's.
    static ALLOC_PHASE: RefCell<AllocPhase> = const { RefCell::new(AllocPhase::Unchecked) };
}

/// RAII guard that sets the current thread's [`AllocPhase`] and restores the
/// previous phase on drop. Restoring (rather than resetting to `Unchecked`)
/// keeps nesting and unwinding correct, so the `catch_unwind` in
/// [`run_wast_test_file`] still reports the offending wast line.
struct PhaseGuard(AllocPhase);

impl Drop for PhaseGuard {
    fn drop(&mut self) {
        ALLOC_PHASE.with(|p| *p.borrow_mut() = self.0);
    }
}

/// Enter a module-load epoch (see [`AllocPhase::Loading`]).
fn enter_loading() -> PhaseGuard {
    let prev = ALLOC_PHASE.with(|p| p.replace(AllocPhase::Loading { freed: false }));
    PhaseGuard(prev)
}

/// Enter a pure-execution region (see [`AllocPhase::Locked`]).
fn enter_locked() -> PhaseGuard {
    let prev = ALLOC_PHASE.with(|p| p.replace(AllocPhase::Locked));
    PhaseGuard(prev)
}

/// The spectest allocator is a simple model of the production `PageAllocator`
/// used to verify flight memory constraints of spacewasm.
struct SpecTestAllocator;

unsafe impl Allocator for SpecTestAllocator {
    unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
        ALLOC_PHASE.with(|p| match &mut *p.borrow_mut() {
            AllocPhase::Locked => panic!("unexpected allocation during execution"),
            AllocPhase::Loading { freed: true } => {
                panic!("allocation after deallocation within module load")
            }
            AllocPhase::Loading { freed: false } | AllocPhase::Unchecked => {}
        });

        // `Allocator` requires that `Ok` carry a valid pointer, so a failed
        // allocation must be reported as an error rather than a null `Ok`.
        let ptr = unsafe { std::alloc::alloc(layout) };
        if ptr.is_null() {
            Err(AllocError::AllocationFailed)
        } else {
            Ok(ptr)
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        ALLOC_PHASE.with(|p| match &mut *p.borrow_mut() {
            AllocPhase::Locked => panic!("unexpected deallocation during execution"),
            AllocPhase::Loading { freed } => *freed = true,
            AllocPhase::Unchecked => {}
        });

        unsafe { std::alloc::dealloc(ptr, layout) }
    }
}

struct RustSystemAllocator;
impl WasmMemoryAllocator for RustSystemAllocator {
    fn allocate(&self, layout: Layout) -> Result<NonNull<u8>, AllocError> {
        unsafe { NonNull::new(std::alloc::alloc(layout)).ok_or(AllocError::AllocationFailed) }
    }

    fn reallocate(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        layout: Layout,
    ) -> Result<NonNull<u8>, AllocError> {
        unsafe {
            NonNull::new(std::alloc::realloc(ptr.as_ptr(), old_layout, layout.size()))
                .ok_or(AllocError::AllocationFailed)
        }
    }

    fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        unsafe { std::alloc::dealloc(ptr.as_ptr(), layout) }
    }
}

global_allocator!(SpecTestAllocator, SpecTestAllocator);

pub struct ByteStream {
    buffer: Option<Vec<u8>>,
    consumed: bool,
}

impl ByteStream {
    fn new(data: &[u8]) -> Self {
        Self {
            buffer: Some(data.to_vec()),
            consumed: false,
        }
    }
}

struct StaticGlobal {
    value: Mutex<Value>,
    ty: ValType,
}

impl GlobalValue for StaticGlobal {
    fn write(&self, _value: Value) -> Result<(), GlobalValueError> {
        Err(GlobalValueError)
    }

    fn read(&self) -> Result<Value, GlobalValueError> {
        Ok(*self.value.lock().unwrap())
    }

    fn ty(&self) -> ValType {
        self.ty
    }

    fn mutable(&self) -> bool {
        false
    }
}

pub struct MutableStaticGlobal {
    pub value: Mutex<Value>,
    pub ty: ValType,
}

impl GlobalValue for MutableStaticGlobal {
    fn write(&self, value: Value) -> Result<(), GlobalValueError> {
        *self.value.lock().unwrap() = value;
        Ok(())
    }

    fn read(&self) -> Result<Value, GlobalValueError> {
        Ok(*self.value.lock().unwrap())
    }

    fn ty(&self) -> ValType {
        self.ty
    }

    fn mutable(&self) -> bool {
        true
    }
}

impl WasmStream for ByteStream {
    fn read(&mut self) -> Result<Option<InnerVec<u8>>, u8> {
        if self.consumed {
            return Ok(None);
        }

        if let Some(ref mut vec) = self.buffer {
            self.consumed = true;
            debug_assert!(
                u32::try_from(vec.len()).is_ok(),
                "wasm module length {} does not fit in u32",
                vec.len()
            );
            let inner = unsafe { InnerVec::from_raw_parts(vec.as_mut_ptr(), vec.len(), vec.len()) };
            Ok(Some(inner))
        } else {
            Ok(None)
        }
    }

    fn return_(&mut self, _chunk: InnerVec<u8>) {
        // No-op: the stream owns `self.buffer` and never handed off ownership,
        // so there is nothing to reclaim. See the handoff rationale in `read`.
    }
}

const MAX_CODE_PAGES: usize = 256;
const MAX_CONTROL_FRAMES: usize = 128;
const MAX_STACK_DEPTH: usize = 256;
/// Instruction budget for a single invoke/resume, used to catch infinite loops.
const MAX_INVOKE_FUEL: usize = 10_000_000;

/// Builds the set of host modules an engine is instantiated with. A factory
/// (rather than a `Vec`) is required because the engine is rebuilt on every
/// [`TestContext::save_store`], and [`HostModule`] is not `Clone`.
type HostModuleFactory = fn() -> spacewasm::Vec<HostModule>;

struct TestContext {
    engine: Engine,
    code_builder: CodeBuilder,
    /// Maps instance names (like "$Mf") to module indices
    /// This is separate from the module's name field which is used for linking/imports
    instance_names: std::collections::HashMap<String, usize>,
    /// Produces the host modules exposed to the test's modules. Stored so the
    /// store can be rebuilt with the same host modules in `save_store`.
    host_modules: HostModuleFactory,
    /// Return types of the currently paused function (if any)
    paused_return_types: Option<spacewasm::Vec<ValType>>,
}

fn new_engine(host_modules: HostModuleFactory) -> Engine {
    Engine::new(1024, 256, host_modules()).unwrap()
}

impl TestContext {
    fn new(host_modules: HostModuleFactory) -> Self {
        TestContext {
            engine: new_engine(host_modules),
            code_builder: CodeBuilder::new(CompilerOptions {
                allow_memory_grow: true,
                max_backpatch_iterations: None,
                max_code_pages: MAX_CODE_PAGES,
            })
            .unwrap(),
            instance_names: std::collections::HashMap::new(),
            host_modules,
            paused_return_types: None,
        }
    }

    fn current_module_index(&self) -> usize {
        if self.engine.store.modules().is_empty() {
            0
        } else {
            self.engine.store.modules().len() - 1
        }
    }

    fn find_module_by_name(&self, name: &str) -> Option<usize> {
        // First check instance names
        if let Some(&idx) = self.instance_names.get(name) {
            return Some(idx);
        }
        // Fall back to checking the module's name field (registered name)
        self.engine
            .store
            .modules()
            .iter()
            .position(|m| m.name == name)
    }

    /// Save the current store state
    /// Used to restore state after failed module loads that mutate the store (memory/tables)
    fn save_store(&self) -> Engine {
        let mut cloned = new_engine(self.host_modules);

        // Clone all modules into the new store
        for module in self.engine.store.modules().iter() {
            let cloned_module = clone_module(module);
            cloned.store.push_module(cloned_module).unwrap();
        }

        cloned
    }

    /// Restore the store from a saved copy
    fn restore_store(&mut self, saved: Engine) {
        self.engine = saved;
    }
}

fn parse_value(spec: &ValueSpec) -> Value {
    let value_str = spec.value.as_ref().expect("Missing value field in spec");
    match spec.ty.as_str() {
        "i32" => Value::I32(
            value_str
                .parse::<u32>()
                .unwrap_or_else(|e| panic!("Failed to parse i32 '{value_str}': {e}"))
                as i32,
        ),
        "i64" => Value::I64(
            value_str
                .parse::<u64>()
                .unwrap_or_else(|e| panic!("Failed to parse i64 '{value_str}': {e}"))
                as i64,
        ),
        "f32" => {
            let bits = value_str
                .parse::<u32>()
                .unwrap_or_else(|e| panic!("Failed to parse f32 bits '{value_str}': {e}"));
            Value::F32(f32::from_bits(bits))
        }
        "f64" => {
            let bits = value_str
                .parse::<u64>()
                .unwrap_or_else(|e| panic!("Failed to parse f64 bits '{value_str}': {e}"));
            Value::F64(f64::from_bits(bits))
        }
        _ => panic!("Unsupported value type: {}", spec.ty),
    }
}

fn assert_nan_f32(z: f32, arithmetic: bool) {
    let bits = z.to_bits();

    let exponent = (bits >> 23) & 0xFF;
    let payload = bits & 0x7F_FFFF;

    if arithmetic {
        assert!(
            (exponent == 0xFF) && ((payload & 0x40_0000) != 0),
            "Expected arithmetic NaN f32 {} ({:x}) (exponent={}), (payload={:x})",
            z,
            bits,
            exponent,
            payload
        )
    } else {
        assert!(
            (exponent == 0xFF) && (payload == 0x400000),
            "Expected canonical NaN f32 {} ({:x}) (exponent={}), (payload={:x})",
            z,
            bits,
            exponent,
            payload
        );
    }
}

fn assert_nan_f64(z: f64, arithmetic: bool) {
    let bits = z.to_bits();

    let exponent = (bits >> 52) & 0x7FF;
    let payload = bits & 0xF_FFFF_FFFF_FFFF;

    if arithmetic {
        assert!(
            (exponent == 0x7FF) && ((payload & 0x8_0000_0000_0000) != 0),
            "Expected arithmetic NaN f64 {} ({:x}) (exponent={}), (payload={:x})",
            z,
            bits,
            exponent,
            payload
        )
    } else {
        assert!(
            (exponent == 0x7FF) && (payload == 0x8_0000_0000_0000),
            "Expected canonical NaN f32 {} ({:08x}) (exponent={}), (payload={:08x})",
            z,
            bits,
            exponent,
            payload
        );
    }
}

fn compare_values(actual: Value, expected: &ValueSpec) {
    let value_str = expected
        .value
        .as_ref()
        .expect("Missing expected value in spec");

    match expected.ty.as_str() {
        "i32" => {
            let Value::I32(a) = actual else {
                panic!("Expected i32, got {actual:?}");
            };
            let e = value_str.parse::<u32>().expect("failed to parse i32") as i32;
            assert_eq!(a, e, "Expected i32 {e}, got {a}");
        }
        "i64" => {
            let Value::I64(a) = actual else {
                panic!("Expected i64, got {actual:?}");
            };
            let e = value_str.parse::<u64>().expect("failed to parse i64") as i64;
            assert_eq!(a, e, "Expected i64 {e}, got {a}");
        }
        "f32" => {
            let Value::F32(a) = actual else {
                panic!("Expected f32, got {actual:?}");
            };

            match value_str.as_str() {
                "nan:arithmetic" => assert_nan_f32(a, true),
                "nan:canonical" => assert_nan_f32(a, false),
                _ => {
                    let expected_f32 =
                        f32::from_bits(value_str.parse::<u32>().expect("failed to parse f32 bits"));
                    assert_eq!(
                        a.to_bits(),
                        expected_f32.to_bits(),
                        "Expected f32 {} ({:08x}), got {} ({:08x})",
                        expected_f32,
                        expected_f32.to_bits(),
                        a,
                        a.to_bits()
                    );
                }
            };
        }
        "f64" => {
            let Value::F64(a) = actual else {
                panic!("Expected f64, got {actual:?}");
            };

            match value_str.as_str() {
                "nan:arithmetic" => assert_nan_f64(a, true),
                "nan:canonical" => assert_nan_f64(a, false),
                _ => {
                    let expected_f64 =
                        f64::from_bits(value_str.parse::<u64>().expect("failed to parse f64 bits"));
                    assert_eq!(
                        a.to_bits(),
                        expected_f64.to_bits(),
                        "Expected f64 {} ({:08x}), got {} ({:08x})",
                        expected_f64,
                        expected_f64.to_bits(),
                        a,
                        a.to_bits()
                    );
                }
            };
        }
        _ => panic!("Unsupported expected value type: {}", expected.ty),
    }
}

#[derive(Debug)]
#[allow(clippy::enum_variant_names)]
enum ModuleLoadError {
    DecodeError(ParseError),
    AllocationError(MemoryError),
    InitializeError(InterpreterResult),
}

impl From<ParseError> for ModuleLoadError {
    fn from(e: ParseError) -> Self {
        ModuleLoadError::DecodeError(e)
    }
}

impl From<MemoryError> for ModuleLoadError {
    fn from(value: MemoryError) -> Self {
        ModuleLoadError::AllocationError(value)
    }
}

fn clone_memory(memory: &Memory) -> spacewasm::Rc<Memory> {
    // Deep clone memory contents
    let mem_type = memory.mem_type();
    let mut new_memory = Memory::new(
        mem_type,
        spacewasm::Rc::new(RustSystemAllocator)
            .unwrap()
            .into_wasm_memory_allocator(),
    )
    .unwrap();

    // Grow the new memory to match the source memory size
    let current_size = memory.size();
    let initial_size = mem_type.min();

    // Only grow if the current size is larger than the initial size
    if current_size > initial_size {
        let grow_by = current_size - initial_size;
        if let Err(e) = new_memory.grow(grow_by) {
            panic!("Failed to grow cloned memory: {:?}", e);
        }
    }

    // Copy the memory contents. Use the memory's actual page size rather than
    // assuming the default 64 KiB Wasm page, so memories declared under the
    // custom-page-sizes proposal (e.g. `MemPageSize::_1`) are cloned with the
    // correct byte length instead of over-/under-reading.
    if current_size > 0 {
        let size_in_bytes = (current_size as usize) * mem_type.page_size();
        let data = memory.load(0, size_in_bytes).unwrap();
        new_memory.store(0, data).unwrap();
    }

    spacewasm::Rc::new(new_memory).unwrap()
}

// Clone a module with deep copies of memory and table contents
// This creates a true snapshot that can be restored after a failed module load
fn clone_module(module: &Module) -> Module {
    use spacewasm::{MemoryKind, TableKind};

    Module {
        name: module.name.clone(),
        types: module.types.clone(),
        functions: module.functions.clone(),
        table: match &module.table {
            None => None,
            Some(TableKind::Import(r)) => Some(TableKind::Import(*r)),
            Some(TableKind::ImportHost(r)) => Some(TableKind::ImportHost(*r)),
            Some(TableKind::Owned((r, ty))) => {
                // Deep clone table elements
                Some(TableKind::Owned((
                    spacewasm::Rc::new_slice(r.len(), |i| r[i]).unwrap(),
                    *ty,
                )))
            }
        },
        memory: match &module.memory {
            None => None,
            Some(MemoryKind::Import(r)) => Some(MemoryKind::Import(*r)),
            Some(MemoryKind::ImportHost(r)) => Some(MemoryKind::ImportHost(*r)),
            Some(MemoryKind::Owned(r)) => Some(MemoryKind::Owned(clone_memory(r))),
        },
        globals: module.globals.clone(),
        imports: module.imports.clone(),
        exports: module.exports.clone(),
        start: module.start,
    }
}

fn load_module(
    ctx: &mut TestContext,
    module_name: Option<String>,
    wasm_bytes: &[u8],
) -> Result<(), ModuleLoadError> {
    // Remove the last module if it has an empty name (unreferenceable)
    // This prevents hitting the 256 module limit in long test suites
    // We can only remove the last module to maintain index-based references
    {
        let modules = ctx.engine.store.modules();
        if !modules.is_empty() && modules[modules.len() - 1].name.is_empty() {
            ctx.engine.store.pop_module();
        }
    }

    // The engine persists across module loads
    // Clear the run state before invoking new functions
    ctx.engine.reset();

    // Create a ByteStream
    let mut stream = ByteStream::new(wasm_bytes);

    let module_ref = {
        let _loading = enter_loading();

        let module = Module::new::<MAX_CONTROL_FRAMES, MAX_STACK_DEPTH>(
            module_name.as_ref().map(|f| f.as_ref()).unwrap_or(""),
            &mut stream,
            &mut ctx.engine.store,
            &mut ctx.code_builder,
            spacewasm::Rc::new(RustSystemAllocator)
                .unwrap()
                .into_wasm_memory_allocator(),
        )?;

        ctx.engine.push_module(module).unwrap()
    };

    // Running the module's start function is pure execution: it must neither
    // allocate nor deallocate.
    let result = {
        let _locked = enter_locked();
        match ctx.engine.module_start(module_ref) {
            None => InterpreterResult::Finished,
            Some(start) => match ctx.engine.invoke(start, &[]) {
                Ok(()) => {
                    Interpreter.run(ctx.code_builder.pages(), &mut ctx.engine, MAX_INVOKE_FUEL)
                }
                Err(InvokeError::StackOverflow) => {
                    InterpreterResult::Trap(TrapReason::StackOverflow)
                }
                Err(_) => unreachable!(),
            },
        }
    };
    match result {
        InterpreterResult::Finished => Ok(()),
        result => Err(ModuleLoadError::InitializeError(result)),
    }
}

fn invoke_function(
    ctx: &mut TestContext,
    module_name: &Option<String>,
    func_name: &str,
    args: &[ValueSpec],
    test_log: Rc<RefCell<LimitedVec<String>>>,
) -> Result<Option<Value>, InterpreterResult> {
    // Check if the engine is paused from a previous invocation
    if ctx.engine.host_pause_result.is_some() {
        invoke_function_resume(ctx, args, test_log)
    } else {
        // Normal invocation path
        invoke_function_normal(ctx, module_name, func_name, args, test_log)
    }
}

fn invoke_function_resume(
    ctx: &mut TestContext,
    args: &[ValueSpec],
    test_log: Rc<RefCell<LimitedVec<String>>>,
) -> Result<Option<Value>, InterpreterResult> {
    // Engine is paused, resume with the provided arguments
    let resume_value = if args.is_empty() {
        None
    } else if args.len() == 1 {
        Some(parse_value(&args[0]))
    } else {
        panic!("Resume expects exactly 0 or 1 argument, got {}", args.len());
    };

    test_log
        .borrow_mut()
        .push(format!("resume {:?}", resume_value));

    // Continue execution from the paused state
    let test_runner: Inspector<'_, _, _, _> = Inspector {
        v: &Interpreter,
        out: test_log.clone(),
    };

    // Resuming and running the interpreter is pure execution: no allocation or
    // deallocation may occur.
    let result = {
        let _locked = enter_locked();
        ctx.engine
            .resume(resume_value)
            .expect("engine should be paused when resuming");
        test_runner.run(ctx.code_builder.pages(), &mut ctx.engine, MAX_INVOKE_FUEL)
    };

    // Get the return types we saved when the function paused
    let return_types = ctx
        .paused_return_types
        .take()
        .expect("No saved return types for paused function");

    match result {
        InterpreterResult::Finished => {
            if return_types.is_empty() {
                Ok(None)
            } else if return_types.len() == 1 {
                Ok(Some(ctx.engine.result.unwrap().to_value(return_types[0])))
            } else {
                panic!("Multi-value returns not supported");
            }
        }
        InterpreterResult::OutOfFuel => panic!("Infinite loop detected"),
        err => Err(err),
    }
}

fn invoke_function_normal(
    ctx: &mut TestContext,
    module_name: &Option<String>,
    func_name: &str,
    args: &[ValueSpec],
    test_log: Rc<RefCell<LimitedVec<String>>>,
) -> Result<Option<Value>, InterpreterResult> {
    // Resolve module index by name lookup
    let module_index = if let Some(name) = module_name {
        ctx.find_module_by_name(name)
            .unwrap_or_else(|| panic!("Module '{name}' not found"))
    } else {
        ctx.current_module_index()
    };

    // Look up function metadata from the store
    let (f_ref, return_types, params) = {
        let module = ctx
            .engine
            .store
            .modules()
            .get(module_index)
            .unwrap_or_else(|| panic!("Module at index {module_index} not found"));

        // Find the exported function
        let export = module
            .exports
            .iter()
            .find(|e| e.name == func_name)
            .expect("Export not found");

        let func_idx = match &export.desc {
            ExportDesc::Func(idx) => *idx,
            _ => panic!("{} is not a function export", func_name),
        };

        // Get the function reference
        let func_ref = module
            .get_func_ref(func_idx)
            .unwrap_or_else(|| panic!("Function {} not found in exports", func_name));

        let func_ref = match func_ref {
            Ref::Module(index) => WasmRef {
                module: ModuleRef(module_index as u8),
                index,
            },
            Ref::Extern { module, index } => WasmRef { module, index },
            _ => panic!(
                "Function {} is not a function export: {:?}",
                func_name, func_ref
            ),
        };

        // Get all the immutable data we need
        let m = &ctx.engine.store.modules()[func_ref.module.0 as usize];
        let f = &m.functions[func_ref.index as usize];
        let func_type = &m.types[f.ty.0 as usize];
        let return_types = func_type.returns.clone();

        // Convert arguments
        let params: Vec<Value> = args.iter().map(parse_value).collect();

        (func_ref, return_types, params)
    };

    let test_runner: Inspector<'_, _, _, _> = Inspector {
        v: &Interpreter,
        out: test_log.clone(),
    };

    test_runner
        .out
        .borrow_mut()
        .push(format!("invoke {}({:?})", func_name, params));

    // Run until completion - up to 10-million instructions to catch infinite loops
    let result = {
        let _locked = enter_locked();
        ctx.engine.invoke(f_ref, &params).unwrap();
        test_runner.run(ctx.code_builder.pages(), &mut ctx.engine, MAX_INVOKE_FUEL)
    };

    // Check the result
    match result {
        InterpreterResult::Finished => {
            if return_types.is_empty() {
                Ok(None)
            } else if return_types.len() == 1 {
                Ok(Some(ctx.engine.result.unwrap().to_value(return_types[0])))
            } else {
                panic!("Multi-value returns not supported");
            }
        }
        InterpreterResult::OutOfFuel => panic!("Infinite loop detected"),
        InterpreterResult::Pause => {
            // Save the return types so we can use them after resume
            ctx.paused_return_types = Some(return_types);
            Err(InterpreterResult::Pause)
        }
        err => Err(err),
    }
}

fn get_global(ctx: &TestContext, module_name: &Option<String>, field: &str) -> Value {
    let module_index = if let Some(name) = module_name {
        ctx.find_module_by_name(name)
            .unwrap_or_else(|| panic!("Module '{name}' not found"))
    } else {
        ctx.current_module_index()
    };

    let global_ref = {
        let module = ctx
            .engine
            .store
            .modules()
            .get(module_index)
            .unwrap_or_else(|| panic!("Module at index {module_index} not found"));

        let export = module
            .exports
            .iter()
            .find(|e| e.name == field)
            .unwrap_or_else(|| panic!("Global export '{field}' not found"));

        let global_idx = match &export.desc {
            ExportDesc::Global(idx) => *idx,
            _ => panic!("Export '{field}' is not a global"),
        };

        module
            .get_global_ref(global_idx)
            .unwrap_or_else(|| panic!("Global export '{field}' does not resolve"))
    };

    match global_ref {
        Ref::Module(index) => {
            ctx.engine.store.modules()[module_index].globals[index as usize].value()
        }
        Ref::Extern { module, index } => {
            ctx.engine.store.modules()[module.0 as usize].globals[index as usize].value()
        }
        Ref::Host { module, index } => ctx.engine.store.host_modules()[module.0 as usize].globals
            [index as usize]
            .value
            .read()
            .unwrap_or_else(|_| panic!("Failed to read host global '{field}'")),
    }
}

fn check_trap_reason(reason: TrapReason, text: &str) {
    match (reason, text) {
        (TrapReason::Unreachable, "unreachable") => {}
        (TrapReason::DivideByZero, "integer divide by zero") => {}
        (TrapReason::InvalidTableIndex, "out of bounds table access") => {}
        (TrapReason::InvalidTableFunctionType, "indirect call type mismatch") => {}
        (TrapReason::MemoryOutOfBounds, "out of bounds memory access") => {}
        (TrapReason::StackOverflow, "stack overflow") => {}
        (TrapReason::InvalidTableIndex, "undefined element") => {}
        (TrapReason::UnrepresentableResult, "integer overflow") => {}
        (TrapReason::BadConversionToInteger, "invalid conversion to integer") => {}
        (TrapReason::IntegerOverflow, "integer overflow") => {}
        (TrapReason::UninitializedTableElement, "uninitialized element") => {}
        (TrapReason::StackOverflow, "call stack exhausted") => {}
        err => {
            panic!("Could not match expected trap text '{text}' with error {err:?}")
        }
    }
}

/// Assert that an interpreter [`ValidationError`] is an acceptable match for the
/// spec-suite's expected rejection `text`.
fn check_decode_error(err: ParseError, text: String) {
    match (err.err.err, text.as_str()) {
        // --- Encoding / structural malformations ---
        (ValidationError::MalformedMagic, "magic header not detected") => {}
        (ValidationError::MalformedVersion, "unknown binary version") => {}
        (ValidationError::MalformedUtf8, "malformed UTF-8 encoding") => {}
        (ValidationError::MalformedSectionId(_), "malformed section id") => {}
        (
            ValidationError::MalformedSectionSize,
            "section size mismatch" | "unexpected end" | "malformed value type",
        ) => {}
        (
            ValidationError::InvalidCodeSectionFunctionCount,
            "function and code section have inconsistent lengths",
        ) => {}
        (ValidationError::DuplicateSection(_), "unexpected content after last section") => {}
        (ValidationError::InvalidSectionOrdering(_, _), "unexpected section order") => {}
        (ValidationError::ExpectedTerminal(0), "zero byte expected") => {}
        (
            ValidationError::MalformedInteger,
            "integer too large" | "integer representation too long",
        ) => {}
        // A truncated stream
        (
            ValidationError::Eof,
            "unexpected end"
            | "length out of bounds"
            | "unexpected end of section or function"
            | "integer representation too long"
            | "malformed value type",
        ) => {}
        (ValidationError::VecTooLong, "length out of bounds") => {}

        // --- Value / type descriptors ---
        (ValidationError::MalformedValueType(_), "malformed value type") => {}
        (ValidationError::MalformedFunction(_), "malformed function type") => {}
        (ValidationError::MalformedElemType(_), "malformed element type") => {}
        (ValidationError::MalformedLimit(_), "malformed limits flag") => {}
        (ValidationError::MalformedMemType(_), "malformed memory type") => {}
        // Import and export descriptors share the same malformed-kind variant.
        (
            ValidationError::MalformedImportExportDesc(_),
            "malformed import kind" | "malformed export kind",
        ) => {}
        (ValidationError::ExpectedConstOrVar(_), "malformed mutability") => {}

        // --- Index-space / definition lookups ("unknown X" family) ---
        (ValidationError::LocalIdxOutOfRange, "unknown local" | "local offset out of range") => {}
        (ValidationError::GlobalIdxOutOfRange, "unknown global") => {}
        (ValidationError::TypeIdxOutOfRange, "unknown type") => {}
        (ValidationError::FunctionImportOutOfRange, "unknown type") => {}
        (ValidationError::FunctionIdxOutOfRange, "unknown function") => {}
        (ValidationError::TableNotDefined, "unknown table") => {}
        (ValidationError::MemoryNotDefined, "unknown memory") => {}
        (ValidationError::InvalidMemIndex, "unknown memory") => {}
        (ValidationError::InvalidTableIndex, "malformed value type" | "unknown table") => {}

        // --- Control flow ---
        (ValidationError::InvalidElseBlock, "else without matching if") => {}
        // An out-of-range label, or a branch whose target is truncated away.
        (
            ValidationError::InvalidLabelIndex,
            "unknown label" | "unexpected end of section or function",
        ) => {}

        // --- Type checking ("type mismatch" is a coarse spec category) ---
        (ValidationError::TypeMismatch, "type mismatch") => {}
        (ValidationError::StackUnderflow, "type mismatch") => {}
        (ValidationError::FunctionResultTypeMismatch, "type mismatch") => {}
        (ValidationError::GlobalTypeMismatch, "type mismatch") => {}
        (ValidationError::InvalidMemOffsetType, "type mismatch") => {}
        // Either a plain block-result mismatch, or a result-typed `if` missing
        // its `else` arm (both are the same internal state).
        (
            ValidationError::BlockResultTypeMismatch,
            "type mismatch" | "result-typed if without else",
        ) => {}
        (ValidationError::BrTableResultTypeMismatch, "type mismatch") => {}
        (ValidationError::FunctionReturnsTooLarge, "invalid result arity") => {}
        (ValidationError::TooManyLocals, "too many locals") => {}
        (ValidationError::AlignmentLargerThanType, "alignment must not be larger than natural") => {
        }

        // --- Constant expressions ---
        (
            ValidationError::InvalidConstantExpr(ConstantExprError::InvalidConstantInstruction),
            "constant expression required",
        ) => {}
        (
            ValidationError::InvalidConstantExpr(ConstantExprError::AlreadyHasValue),
            "type mismatch",
        ) => {}
        (ValidationError::InvalidConstantExpr(ConstantExprError::NoValue), "type mismatch") => {}
        (
            ValidationError::InvalidConstantExpr(ConstantExprError::InvalidGlobal),
            "unknown global",
        ) => {}

        // --- Immutable globals ---
        // `GlobalNotMutable` covers both a `global.set` on an immutable
        // global (`"immutable global"`) and an import whose mutability disagrees
        // with the definition (`"incompatible import type"`).
        (ValidationError::GlobalNotMutable, "immutable global" | "incompatible import type") => {}

        // --- Limits / sizing ---
        (ValidationError::InvalidMaxLimit, "size minimum must not be greater than maximum") => {}
        // Two upstream phrasings for the same 4 GiB memory cap.
        (
            ValidationError::MemoryTooLarge,
            "memory size must be at most 65536 pages (4GiB)" | "memory size must be at most 4 GiB",
        ) => {}
        (ValidationError::TableTooLarge, "table size too large") => {}
        (ValidationError::InvalidPageSize(_), "invalid custom page size") => {}
        (ValidationError::StackTooLarge, "call frame too large") => {}

        // --- Sections / names that must be unique ---
        (ValidationError::MultipleMemories, "multiple memories") => {}
        (ValidationError::MultipleTables, "multiple tables") => {}
        (ValidationError::DuplicateExportName, "duplicate export name") => {}

        // --- Start function ---
        (ValidationError::InvalidStartFunctionSignature, "start function") => {}
        (
            ValidationError::InvalidHostStartFunction,
            "start function must not be a host function",
        ) => {}

        // --- Data / element segment placement ---
        (ValidationError::InvalidNegativeMemOffset, "data segment does not fit") => {}
        (ValidationError::MemoryError(MemoryError::OutOfBounds), "data segment does not fit") => {}
        (ValidationError::InvalidElementOutOfBounds, "elements segment does not fit") => {}
        // A bad element offset is either a type mismatch on the offset
        // expression, or an out-of-range placement past the table end.
        (
            ValidationError::InvalidElementOffset,
            "type mismatch" | "elements segment does not fit",
        ) => {}
        (ValidationError::TableRefNotUnique, "table reference not unique") => {}

        // --- Imports ("unknown import" vs "incompatible import type") ---
        // Each `*ImportNotFound` variant is used both when no matching export
        // exists at all (`"unknown import"`) and when a candidate exists but is
        // rejected as incompatible (`"incompatible import type"`).
        (
            ValidationError::FunctionImportNotFound,
            "unknown import" | "incompatible import type",
        ) => {}
        (ValidationError::GlobalImportNotFound, "unknown import" | "incompatible import type") => {}
        (ValidationError::MemoryImportNotFound, "unknown import" | "incompatible import type") => {}
        (ValidationError::TableImportNotFound, "unknown import" | "incompatible import type") => {}
        (ValidationError::FunctionImportTypeMismatch, "incompatible import type") => {}
        (ValidationError::GlobalImportTypeMismatch, "incompatible import type") => {}
        (ValidationError::MemoryImportTypeMismatch, "incompatible import type") => {}
        (ValidationError::MemoryImportTooLarge, "incompatible import type") => {}
        (ValidationError::TableImportIncompatibleSize, "incompatible import type") => {}
        (ValidationError::TableImportTypeMismatch, "incompatible import type") => {}

        // --- Resource allocation ---
        (ValidationError::GuestMemoryAllocationFailure, "allocation failed") => {}

        err => {
            panic!("Could not match validation error text '{text}' with error {err:?}")
        }
    }
}

fn check_initialization_error(result: InterpreterResult, text: &str) {
    match (result, text) {
        (InterpreterResult::Trap(TrapReason::Unreachable), "unreachable") => {}
        (InterpreterResult::Trap(TrapReason::StackOverflow), "stack overflow") => {}
        (result, text) => {
            panic!("Could not match initialization error text '{text}' with result {result:?}")
        }
    }
}

/// Wrapper for `wast2json`
#[cfg(not(miri))]
fn wast2json(source_wast_path: &Path, out_dir: &Path, test_filename: &str) {
    // Resolve the WABT `wast2json` tool used to compile the `.wast`
    let wast2json_bin = std::env::var_os("WABT_WAST2JSON")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("wast2json"));

    // Run wast2json to generate Wasm modules and JSON descriptor
    let output = ProcessCommand::new(&wast2json_bin)
        .arg(source_wast_path)
        .arg("--enable-custom-page-sizes")
        .arg("-o")
        .arg(out_dir.join(format!("{}.json", test_filename)))
        .current_dir(out_dir)
        .output()
        .unwrap_or_else(|e| panic!("Failed to run {}: {e}", wast2json_bin.display()));

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!("wast2json failed: {}", stderr);
    }
}

#[cfg(not(miri))]
pub fn convert_wast_for_miri() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let tests_dir = PathBuf::from(manifest_dir).join("tests");
    let converted_root = PathBuf::from(manifest_dir).join("target").join("miri-wast");

    let _ = std::fs::remove_dir_all(&converted_root);

    // The `tests/` subdirectories containing `.wast` files. Hard-coded
    // rather than walked, since it's a fixed, small set; add to this list
    // if a new `.wast` suite subdirectory is introduced.
    let wast_dirs: &[&str] = &["core", "regression", "custom-page-sizes"];

    for subdir in wast_dirs {
        let dir = tests_dir.join(subdir);
        for entry in
            std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("Failed to read {dir:?}: {e}"))
        {
            let wast_path = entry.unwrap().path();
            if wast_path.extension().is_none_or(|ext| ext != "wast") {
                continue;
            }

            let rel = wast_path
                .strip_prefix(&tests_dir)
                .unwrap()
                .with_extension("");
            let test_filename = rel.file_stem().unwrap().to_string_lossy().to_string();

            let out_dir = converted_root.join(&rel);
            std::fs::create_dir_all(&out_dir).unwrap();

            wast2json(&wast_path, &out_dir, &test_filename);
        }
    }
}

#[cfg(not(miri))]
struct TempDir {
    path: PathBuf,
}

#[cfg(not(miri))]
impl TempDir {
    fn new() -> std::io::Result<Self> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let pid = std::process::id();
        let count = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir_name = format!("spacewasm-test-{}-{}", pid, count);
        let path = std::env::temp_dir().join(dir_name);
        std::fs::create_dir(&path)?;
        Ok(TempDir { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(not(miri))]
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// The standard `spectest` host module required by most of the spec test
/// suite (print functions, well-known globals, a memory and a table).
pub fn spectest_host_module() -> HostModule {
    HostModule {
        name: "spectest".into(),
        globals: vec![
            HostGlobal {
                name: "global_i32".into(),
                value: spacewasm::Box::new(StaticGlobal {
                    value: Mutex::new(Value::I32(666)),
                    ty: ValType::I32,
                })
                .unwrap()
                .into_global_value_dyn(),
            },
            HostGlobal {
                name: "global_i64".into(),
                value: spacewasm::Box::new(StaticGlobal {
                    value: Mutex::new(Value::I64(666)),
                    ty: ValType::I64,
                })
                .unwrap()
                .into_global_value_dyn(),
            },
            HostGlobal {
                name: "global_f32".into(),
                value: spacewasm::Box::new(StaticGlobal {
                    value: Mutex::new(Value::F32(666.6)),
                    ty: ValType::F32,
                })
                .unwrap()
                .into_global_value_dyn(),
            },
            HostGlobal {
                name: "global_f64".into(),
                value: spacewasm::Box::new(StaticGlobal {
                    value: Mutex::new(Value::F64(666.6)),
                    ty: ValType::F64,
                })
                .unwrap()
                .into_global_value_dyn(),
            },
        ],
        functions: vec![
            HostFunction::new("print", "".into(), "".into(), |_, _| {
                ControlFlow::Continue(None)
            }),
            HostFunction::new("print_i32", "i".into(), "".into(), |_, _| {
                ControlFlow::Continue(None)
            }),
            HostFunction::new("print_i64", "I".into(), "".into(), |_, _| {
                ControlFlow::Continue(None)
            }),
            HostFunction::new("print_f32", "f".into(), "".into(), |_, _| {
                ControlFlow::Continue(None)
            }),
            HostFunction::new("print_f64", "d".into(), "".into(), |_, _| {
                ControlFlow::Continue(None)
            }),
            HostFunction::new("print_i32_f32", "if".into(), "".into(), |_, _| {
                ControlFlow::Continue(None)
            }),
            HostFunction::new("print_f64_f64", "dd".into(), "".into(), |_, _| {
                ControlFlow::Continue(None)
            }),
        ],
        memory: vec![spacewasm::HostSymbol {
            name: "memory".into(),
            value: spacewasm::Rc::new(
                Memory::new(
                    spacewasm::MemType {
                        initial_pages: 1,
                        max_pages: Some(2),
                        page_size: spacewasm::MemPageSize::_65536,
                    },
                    spacewasm::Rc::new(RustSystemAllocator)
                        .unwrap()
                        .into_wasm_memory_allocator(),
                )
                .unwrap(),
            )
            .unwrap(),
        }],
        table: vec![spacewasm::HostSymbol {
            name: "table".into(),
            value: (
                spacewasm::Rc::new_slice_with_default(10).unwrap(),
                Limit {
                    min: 10,
                    max: Some(20),
                },
            ),
        }],
    }
}

fn run_wast_command(
    command: Command,
    test_dir: &Path,
    ctx: &mut TestContext,
    log: Rc<RefCell<LimitedVec<String>>>,
) {
    match command {
        Command::Module { name, filename, .. } => {
            let wasm_path = test_dir.join(&filename);
            let wasm_bytes =
                std::fs::read(&wasm_path).unwrap_or_else(|e| panic!("Failed to read module: {e}"));
            load_module(ctx, name.clone(), &wasm_bytes).unwrap();

            // Register the instance name if provided
            if let Some(instance_name) = name {
                let module_index = ctx.current_module_index();
                ctx.instance_names.insert(instance_name, module_index);
            }
        }
        Command::AssertReturn {
            action, expected, ..
        } => {
            let result = match action {
                Action::Invoke {
                    module,
                    field,
                    args,
                } => match invoke_function(ctx, &module, &field, &args, log) {
                    Ok(val) => val,
                    Err(e) => {
                        panic!("Invoke '{field}' failed: {e:?}")
                    }
                },
                Action::Get { module, field } => Some(get_global(ctx, &module, &field)),
            };

            if expected.is_empty() {
                assert!(result.is_none(), "Expected no return value, got {result:?}");
            } else if expected.len() == 1 {
                let actual = result.unwrap_or_else(|| panic!("Expected return value, got none"));
                compare_values(actual, &expected[0]);
            } else {
                panic!("Multi-value returns not yet supported");
            }
        }
        Command::AssertUninstantiable {
            text,
            filename,
            module_type,
            ..
        } => {
            if module_type != "text" {
                let wasm_path = test_dir.join(&filename);
                let wasm_bytes = std::fs::read(&wasm_path)
                    .unwrap_or_else(|e| panic!("Failed to read module: {e}"));

                match load_module(ctx, None, &wasm_bytes) {
                    Ok(_) => {
                        panic!("Expected error when linking/initializing module");
                    }
                    Err(ModuleLoadError::InitializeError(result)) => {
                        check_initialization_error(result, &text);
                    }
                    Err(err) => {
                        panic!("Failed to decode module '{err:?}'");
                    }
                }
            }
        }
        Command::AssertTrap { action, text, .. } => match action {
            Action::Invoke {
                module,
                field,
                args,
            } => match invoke_function(ctx, &module, &field, &args, log) {
                Err(InterpreterResult::Trap(reason)) => {
                    check_trap_reason(reason, &text);
                }
                Err(InterpreterResult::Pause) => {
                    if text != "paused" {
                        panic!("Interpreter paused while expecting trap '{text}'");
                    }
                }
                Err(err) => {
                    panic!("Expected trap '{text}', got error: {err:?}")
                }
                Ok(_) => {
                    panic!("Expected trap '{text}', but execution succeeded")
                }
            },
            // A global read cannot trap, so the spec never wraps `get` in
            // `assert_trap`.
            Action::Get { .. } => {
                panic!("assert_trap does not support the `get` action")
            }
        },
        Command::AssertMalformed {
            filename,
            module_type,
            text,
            ..
        } => {
            // Skip text format tests as we only handle binary Wasm
            if module_type != "text" {
                let wasm_path = test_dir.join(&filename);
                let wasm_bytes = std::fs::read(&wasm_path).unwrap();

                let saved_store = ctx.save_store();
                match load_module(ctx, None, &wasm_bytes) {
                    Err(ModuleLoadError::DecodeError(err)) => {
                        check_decode_error(err, text);
                        ctx.restore_store(saved_store);
                    }
                    _ => {
                        ctx.restore_store(saved_store);
                        panic!("Expected error when decoding module");
                    }
                }
            }
        }
        Command::AssertInvalid {
            filename,
            module_type,
            text,
            ..
        }
        | Command::AssertUnlinkable {
            filename,
            module_type,
            text,
            ..
        } => {
            if module_type != "text" {
                let wasm_path = test_dir.join(&filename);
                let wasm_bytes = std::fs::read(&wasm_path)
                    .unwrap_or_else(|e| panic!("Failed to read {}: {e}", wasm_path.display()));

                let saved_store = ctx.save_store();
                match load_module(ctx, None, &wasm_bytes) {
                    Err(ModuleLoadError::DecodeError(err)) => {
                        check_decode_error(err, text);
                        ctx.restore_store(saved_store);
                    }
                    Err(ModuleLoadError::AllocationError(err)) => {
                        ctx.restore_store(saved_store);
                        panic!("Expected error when decoding module '{err:?}'");
                    }
                    _ => {
                        ctx.restore_store(saved_store);
                        panic!("Expected error when decoding module");
                    }
                }
            }
        }
        Command::AssertExhaustion { action, text, .. } => match action {
            Action::Invoke {
                module,
                field,
                args,
            } => match invoke_function(ctx, &module, &field, &args, log) {
                Err(InterpreterResult::Trap(reason)) => check_trap_reason(reason, &text),
                Err(err) => {
                    panic!("Expected exhaustion '{text}', got error: {err:?}")
                }
                Ok(_) => {
                    panic!("Expected exhaustion '{text}', but execution succeeded")
                }
            },
            // A global read cannot exhaust resources, so the spec never wraps
            // `get` in `assert_exhaustion`.
            Action::Get { .. } => {
                panic!("assert_exhaustion does not support the `get` action")
            }
        },
        Command::Register { name, as_name, .. } => {
            // Register updates the module name in the store to the alias
            let module_index = if let Some(ref module_name) = name {
                ctx.find_module_by_name(module_name)
                    .unwrap_or_else(|| panic!("Module '{module_name}' not found for registration"))
            } else {
                ctx.current_module_index()
            };

            // Update the module name in the store to the registered alias
            // This allows linking to find it by the registered name
            let module = ctx
                .engine
                .store
                .modules_mut()
                .get_mut(module_index)
                .unwrap();
            module.name = as_name.as_str().try_into().unwrap();
        }
        Command::Action { action, .. } => match action {
            Action::Invoke {
                module,
                field,
                args,
            } => {
                invoke_function(ctx, &module, &field, &args, log).unwrap();
            }
            Action::Get { module, field } => {
                // A bare `get` action reads the global for its side effects and
                // discards the value.
                let _ = get_global(ctx, &module, &field);
            }
        },
    }
}

fn run_wast_test_file_inner(
    test_dir: PathBuf,
    test_name: &str,
    host_modules: HostModuleFactory,
    wast_line: Arc<Mutex<Option<u32>>>,
    subtest_log: SubtestLogType,
) {
    let json_path = test_dir.join(format!("{}.json", test_name));

    let json_content = std::fs::read_to_string(&json_path)
        .unwrap_or_else(|e| panic!("Failed to read JSON file: {}: {e}", json_path.display()));

    let test_file: TestFile = serde_json::from_str(&json_content)
        .unwrap_or_else(|e| panic!("Failed to parse JSON file {}: {}", json_path.display(), e));

    let mut ctx = TestContext::new(host_modules);

    for command in test_file.commands {
        let test_log = Rc::new(RefCell::new(LimitedVec::<String>::new()));
        *subtest_log.lock().unwrap() = Some(test_log.clone());
        *wast_line.lock().unwrap() = match &command {
            Command::Module { line, .. }
            | Command::AssertReturn { line, .. }
            | Command::AssertTrap { line, .. }
            | Command::AssertUninstantiable { line, .. }
            | Command::AssertMalformed { line, .. }
            | Command::AssertInvalid { line, .. }
            | Command::AssertExhaustion { line, .. }
            | Command::Register { line, .. }
            | Command::Action { line, .. }
            | Command::AssertUnlinkable { line, .. } => Some(*line),
        };

        run_wast_command(command, &test_dir, &mut ctx, test_log);

        *subtest_log.lock().unwrap() = None;
        *wast_line.lock().unwrap() = None;
    }
}

/// Run a spec test file with a caller-provided set of host modules. Use this
/// for suites that depend on host modules beyond the standard `spectest` one
/// (for example the regression tests, which also need
/// [`regression_host_module`]).
pub fn run_wast_test_file(test_name: &str, host_modules: HostModuleFactory) {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let source_wast_path = PathBuf::from(manifest_dir)
        .join("tests")
        .join(format!("{}.wast", test_name));

    // Create a temporary directory for generated files
    #[cfg(not(miri))]
    let temp_dir =
        TempDir::new().unwrap_or_else(|e| panic!("Failed to create temp directory: {e}"));
    #[cfg(not(miri))]
    let temp_path = temp_dir.path().to_path_buf();

    // Extract just the filename (without directory path) for the JSON output
    let test_filename = PathBuf::from(test_name)
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .to_string();

    // Pre-convert tests for Miri to run
    #[cfg(not(miri))]
    wast2json(&source_wast_path, &temp_path, &test_filename);
    #[cfg(not(miri))]
    let test_dir = temp_path;

    #[cfg(miri)]
    let test_dir = {
        let dir = PathBuf::from(manifest_dir)
            .join("target")
            .join("miri-wast")
            .join(test_name);

        if !dir.join(format!("{}.json", test_filename)).exists() {
            panic!(
                "Converted wast files missing at {}. Run `cargo test --test miri_wast_convert \
                 -- --ignored` (with `wast2json` on PATH) before `cargo miri test`.",
                dir.display()
            );
        }

        dir
    };

    let wast_line = Arc::new(Mutex::new(None));
    #[allow(clippy::arc_with_non_send_sync)]
    let subtest_log = Arc::new(Mutex::new(None));

    match catch_unwind(|| {
        run_wast_test_file_inner(
            test_dir,
            &test_filename,
            host_modules,
            wast_line.clone(),
            subtest_log.clone(),
        )
    }) {
        Ok(_) => {}
        Err(err) => {
            if let Some(log) = &*subtest_log.lock().unwrap() {
                let log_lines: Vec<String> = log.borrow().clone().into();
                if !log_lines.is_empty() {
                    eprintln!("Subtest failed, dumping invoke log");
                    for line in log_lines.iter() {
                        eprintln!("{}", line);
                    }
                    eprintln!("========")
                }
            }

            let msg = if let Some(s) = err.downcast_ref::<&'static str>() {
                s.to_string()
            } else if let Some(s) = err.downcast_ref::<String>() {
                s.clone()
            } else {
                "Unknown panic payload".to_string()
            };

            if let Some(line_no) = *wast_line.lock().unwrap() {
                panic!("{}:{}: {}", source_wast_path.display(), line_no, msg)
            } else {
                panic!("{}: {}", source_wast_path.display(), msg)
            }
        }
    }
}
