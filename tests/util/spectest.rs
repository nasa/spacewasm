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
use serde::{Deserialize, Serialize};
use spacewasm::{
    AllocError, Allocator, CodeBuilder, CompilerOptions, ConstantExprError, ExportDesc,
    GlobalValue, GlobalValueError, HostFunction, HostGlobal, HostModule, InnerVec, Interpreter,
    InterpreterResult, InterpreterRunner, InterpreterState, Limit, Memory, MemoryError,
    MemoryStatistics, Module, ModuleRef, ParseError, Ref, Stack, Store, TrapReason, ValType,
    ValidationError, Value, WasmMemoryAllocator, WasmRef, WasmStream, global_allocator, vec,
};
use std::alloc::Layout;
use std::cell::RefCell;
use std::ops::ControlFlow;
use std::panic::catch_unwind;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command as ProcessCommand;
use std::ptr::NonNull;
use std::rc::Rc;

type SubtestLogType = Arc<Mutex<Option<Rc<RefCell<LimitedVec<String>>>>>>;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

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

struct RustSystemAllocator;

unsafe impl Allocator for RustSystemAllocator {
    unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
        unsafe { Ok(std::alloc::alloc(layout)) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { std::alloc::dealloc(ptr, layout) }
    }

    fn memory_statistics(&self) -> MemoryStatistics {
        MemoryStatistics {
            total_bytes: 0,
            pad_bytes: 0,
        }
    }
}

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

global_allocator!(RustSystemAllocator, RustSystemAllocator);

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
        false
    }
}

struct MutableStaticGlobal {
    value: Mutex<Value>,
    ty: ValType,
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
            let inner = InnerVec {
                ptr: vec.as_mut_ptr(),
                capacity: vec.len() as u32,
                len: vec.len() as u32,
            };
            Ok(Some(inner))
        } else {
            Ok(None)
        }
    }

    fn return_(&mut self, _chunk: InnerVec<u8>) {
        // Buffer is kept alive in self.buffer, so nothing to do
    }
}

const MAX_CODE_PAGES: usize = 256;
const MAX_CONTROL_FRAMES: usize = 128;
const MAX_STACK_DEPTH: usize = 256;

struct TestContext {
    store: Store,
    stack: Stack,
    code_builder: CodeBuilder<MAX_CODE_PAGES>,
    /// Maps instance names (like "$Mf") to module indices
    /// This is separate from the module's name field which is used for linking/imports
    instance_names: std::collections::HashMap<String, usize>,
}

impl TestContext {
    fn new() -> Self {
        let store = Store::new(256, [test_host_module(), regression_host_module()]).unwrap();

        TestContext {
            store,
            stack: Stack::new(1024).unwrap(),
            code_builder: CodeBuilder::<256>::default(),
            instance_names: std::collections::HashMap::new(),
        }
    }

    fn with_state<F, R>(&mut self, f: F) -> R
    where
        F: for<'a> FnOnce(&mut InterpreterState<'a>) -> R,
    {
        let mut state = self.store.allocate(1024).unwrap();
        state.stack = core::mem::replace(&mut self.stack, Stack::new(1024).unwrap());
        let result = f(&mut state);
        self.stack = state.stack;
        result
    }

    fn current_module_index(&self) -> usize {
        if self.store.modules().is_empty() {
            0
        } else {
            self.store.modules().len() - 1
        }
    }

    fn find_module_by_name(&self, name: &str) -> Option<usize> {
        // First check instance names
        if let Some(&idx) = self.instance_names.get(name) {
            return Some(idx);
        }
        // Fall back to checking the module's name field (registered name)
        self.store.modules().iter().position(|m| m.name == name)
    }

    /// Save the current store state
    /// Used to restore state after failed module loads that mutate the store (memory/tables)
    fn save_store(&self) -> Store {
        let mut cloned = Store::new(256, [test_host_module(), regression_host_module()]).unwrap();

        // Clone all modules into the new store
        for module in self.store.modules().iter() {
            let cloned_module = clone_module(module);
            cloned.push_module(cloned_module);
        }

        cloned
    }

    /// Restore the store from a saved copy
    fn restore_store(&mut self, saved: Store) {
        self.store = saved;
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

    // Copy the memory contents
    if current_size > 0 {
        let size_in_bytes = (current_size as usize) * 65536;
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

// We need to add a method to Store to support pushing modules
// For now, TestContext will manage store cloning by saving/restoring the entire Store

fn load_module(
    ctx: &mut TestContext,
    module_name: Option<String>,
    wasm_bytes: &[u8],
) -> Result<(), ModuleLoadError> {
    // Remove the last module if it has an empty name (unreferenceable)
    // This prevents hitting the 256 module limit in long test suites
    // We can only remove the last module to maintain index-based references
    {
        let modules = ctx.store.modules();
        if !modules.is_empty() && modules[modules.len() - 1].name.is_empty() {
            ctx.store.pop_module();
        }
    }

    // Create a ByteStream
    let mut stream = ByteStream::new(wasm_bytes);

    // Parse and validate the module
    let module = Module::new::<MAX_CODE_PAGES, MAX_CONTROL_FRAMES, MAX_STACK_DEPTH>(
        module_name.as_ref().map(|f| f.as_ref()).unwrap_or(""),
        &mut stream,
        &mut ctx.store,
        &mut ctx.code_builder,
        spacewasm::Rc::new(RustSystemAllocator)
            .unwrap()
            .into_wasm_memory_allocator(),
        CompilerOptions {
            allow_memory_grow: true,
        },
    )?;

    // Finish the code builder to get the text
    let (text, _final_page_offset) = ctx.code_builder.clone().finish().unwrap();

    // Initialize the module
    ctx.with_state(
        |state| match state.initialize_module(module, &text, usize::MAX) {
            InterpreterResult::Finished => Ok(()),
            result => Err(ModuleLoadError::InitializeError(result)),
        },
    )
}

fn invoke_function(
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
        let m = &ctx.store.modules()[func_ref.module.0 as usize];
        let f = &m.functions[func_ref.index as usize];
        let func_type = &m.types[f.ty.0 as usize];
        let return_types = func_type.returns.clone();

        // Convert arguments
        let params: Vec<Value> = args.iter().map(parse_value).collect();

        (func_ref, return_types, params)
    };

    let (text, _final_page_offset) = ctx.code_builder.clone().finish().unwrap();

    ctx.with_state(|state| {
        state.invoke(f_ref, &params).unwrap();

        let interpreter = Interpreter::default();

        let test_runner = Inspector {
            v: &interpreter,
            out: test_log.clone(),
        };

        test_runner
            .out
            .borrow_mut()
            .push(format!("invoke {}({:?})", func_name, params));

        // Run until completion - up to 10-million instructions to catch infinite loops
        let result = test_runner.run(&text, state, 10000000);

        // Check the result
        match result {
            InterpreterResult::Finished => {
                if return_types.is_empty() {
                    Ok(None)
                } else if return_types.len() == 1 {
                    Ok(Some(state.result.unwrap().to_value(return_types[0])))
                } else {
                    panic!("Multi-value returns not supported");
                }
            }
            InterpreterResult::ReaderError(err) => panic!("Reader error: {err:?}"),
            InterpreterResult::OutOfFuel => panic!("Infinite loop detected"),
            err => Err(err),
        }
    })
}

fn check_trap_reason(reason: TrapReason, text: &str) {
    /*
    RuntimeError::Trap(TrapError::DivideBy0) => Ok("integer divide by zero"),
        RuntimeError::Trap(TrapError::UnrepresentableResult) => Ok("integer overflow"),
        RuntimeError::Trap(TrapError::BadConversionToInteger) => {
            Ok("invalid conversion to integer")
        }
        RuntimeError::Trap(TrapError::ReachedUnreachable) => Ok("unreachable"),
        RuntimeError::Trap(TrapError::MemoryOrDataAccessOutOfBounds) => {
            Ok("out of bounds memory access")
        }
        RuntimeError::Trap(TrapError::TableOrElementAccessOutOfBounds) => {
            Ok("out of bounds table access")
        }
        RuntimeError::Trap(TrapError::UninitializedElement) => Ok("uninitialized element"),
        RuntimeError::Trap(TrapError::SignatureMismatch) => Ok("indirect call type mismatch"),
        RuntimeError::Trap(TrapError::TableAccessOutOfBounds) => Ok("undefined element"),

        RuntimeError::StackExhaustion => Ok("call stack exhausted"),
        RuntimeError::ModuleNotFound => Ok("module not found"),
        RuntimeError::FunctionNotFound => Err(WastError::UnrepresentedRuntimeError),
        RuntimeError::HostFunctionSignatureMismatch => Ok("host function signature mismatch"),

     */
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

fn check_decode_error(err: ParseError, text: String) {
    match (err.err.err, text.as_str()) {
        (
            ValidationError::MalformedInteger,
            "integer too large" | "integer representation too long",
        ) => {}
        (ValidationError::MalformedMagic, "magic header not detected") => {}
        (ValidationError::MalformedVersion, "unknown binary version") => {}
        (ValidationError::ExpectedTerminal(0), "zero byte expected") => {}
        (
            ValidationError::Eof,
            "unexpected end" | "length out of bounds" | "unexpected end of section or function",
        ) => {}
        (ValidationError::TooManyLocals, "too many locals") => {}
        (ValidationError::MalformedUtf8, "malformed UTF-8 encoding") => {}
        (
            ValidationError::InvalidCodeSectionFunctionCount,
            "function and code section have inconsistent lengths",
        ) => {}
        (ValidationError::MalformedSectionSize, "section size mismatch") => {}
        (ValidationError::LocalIdxOutOfRange, "unknown local") => {}
        (ValidationError::MultipleMemories, "multiple memories") => {}
        (ValidationError::AlignmentLargerThanType, "alignment must not be larger than natural") => {
        }
        (ValidationError::TypeMismatch, "type mismatch") => {}
        (ValidationError::BlockResultTypeMismatch, "type mismatch") => {}
        (ValidationError::InvalidLabelIndex, "unknown label") => {}
        (ValidationError::MalformedSectionSize, "unexpected end") => {}
        (ValidationError::GlobalIdxOutOfRange, "unknown global") => {}
        (ValidationError::MalformedSectionId(_), "malformed section id") => {}
        (ValidationError::VecTooLong, "length out of bounds") => {}
        (ValidationError::StackUnderflow, "type mismatch") => {}
        (ValidationError::TypeIdxOutOfRange, "unknown type") => {}
        (ValidationError::FunctionResultTypeMismatch, "type mismatch") => {}
        (ValidationError::FunctionIdxOutOfRange, "unknown function") => {}
        (ValidationError::FunctionReturnsTooLarge, "invalid result arity") => {}
        (ValidationError::TableNotDefined, "unknown table") => {}
        (ValidationError::InvalidTableIndex, "malformed value type") => {}
        (ValidationError::InvalidLabelIndex, "unexpected end of section or function") => {}
        (ValidationError::MalformedValueType(_), "malformed value type") => {}
        (ValidationError::DuplicateSection(_), "unexpected content after last section") => {}
        (ValidationError::GlobalIsNotMutable, "immutable global") => {}
        (ValidationError::InvalidElementOffset, "type mismatch") => {}
        (
            ValidationError::InvalidConstantExpr(ConstantExprError::InvalidConstantInstruction),
            "constant expression required",
        ) => {}
        (ValidationError::FunctionImportOutOfRange, "unknown type") => {}
        (ValidationError::GlobalTypeMismatch, "type mismatch") => {}
        (
            ValidationError::InvalidConstantExpr(ConstantExprError::AlreadyHasValue),
            "type mismatch",
        ) => {}
        (ValidationError::InvalidConstantExpr(ConstantExprError::NoValue), "type mismatch") => {}
        (
            ValidationError::InvalidConstantExpr(ConstantExprError::InvalidGlobal),
            "unknown global",
        ) => {}
        (ValidationError::ExpectedConstOrVar(_), "malformed mutability") => {}
        (ValidationError::MemoryNotDefined, "unknown memory") => {}
        (ValidationError::InvalidMaxLimit, "size minimum must not be greater than maximum") => {}
        (ValidationError::MemoryTooLarge, "memory size must be at most 65536 pages (4GiB)") => {}
        (ValidationError::MemoryTooLarge, "memory size must be at most 4 GiB") => {}
        (ValidationError::InvalidNegativeMemOffset, "data segment does not fit") => {}
        (ValidationError::InvalidMemOffsetType, "type mismatch") => {}
        (ValidationError::InvalidStartFunctionSignature, "start function") => {}
        (ValidationError::DuplicateExportName, "duplicate export name") => {}
        (ValidationError::InvalidTableIndex, "unknown table") => {}
        (ValidationError::MemoryError(MemoryError::OutOfBounds), "data segment does not fit") => {}
        (ValidationError::InvalidMemIndex, "unknown memory") => {}
        (ValidationError::FunctionImportNotFound, "unknown import") => {}
        (ValidationError::GlobalImportNotFound, "unknown import") => {}
        (ValidationError::MemoryImportNotFound, "unknown import") => {}
        (ValidationError::FunctionImportTypeMismatch, "incompatible import type") => {}
        (ValidationError::GlobalImportTypeMismatch, "incompatible import type") => {}
        (ValidationError::MemoryImportTypeMismatch, "incompatible import type") => {}
        (ValidationError::FunctionImportNotFound, "incompatible import type") => {}
        (ValidationError::GlobalImportNotFound, "incompatible import type") => {}
        (ValidationError::MemoryImportNotFound, "incompatible import type") => {}
        (ValidationError::GlobalIsNotMutable, "incompatible import type") => {}
        (ValidationError::InvalidElementOutOfBounds, "elements segment does not fit") => {}
        (ValidationError::InvalidElementOffset, "elements segment does not fit") => {}
        (ValidationError::MultipleTables, "multiple tables") => {}
        (ValidationError::TableImportNotFound, "unknown import") => {}
        (ValidationError::TableImportIncompatibleSize, "incompatible import type") => {}
        (ValidationError::TableImportTypeMismatch, "incompatible import type") => {}
        (ValidationError::TableImportNotFound, "incompatible import type") => {}
        (ValidationError::MemoryImportTooLarge, "incompatible import type") => {}
        (ValidationError::InvalidPageSize(_), "invalid custom page size") => {}
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

// Simple temp directory that cleans up on drop
struct TempDir {
    path: PathBuf,
}

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

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn test_host_module() -> HostModule {
    HostModule {
        name: "spectest",
        globals: vec![
            HostGlobal {
                name: "global_i32",
                value: spacewasm::Box::new(StaticGlobal {
                    value: Mutex::new(Value::I32(666)),
                    ty: ValType::I32,
                })
                .unwrap()
                .into_global_value_dyn(),
            },
            HostGlobal {
                name: "global_i64",
                value: spacewasm::Box::new(StaticGlobal {
                    value: Mutex::new(Value::I64(666)),
                    ty: ValType::I64,
                })
                .unwrap()
                .into_global_value_dyn(),
            },
            HostGlobal {
                name: "global_f32",
                value: spacewasm::Box::new(StaticGlobal {
                    value: Mutex::new(Value::F32(666.6)),
                    ty: ValType::F32,
                })
                .unwrap()
                .into_global_value_dyn(),
            },
            HostGlobal {
                name: "global_f64",
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
            name: "memory",
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
            name: "table",
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

fn regression_host_module() -> HostModule {
    HostModule {
        name: "regression",
        globals: vec![
            HostGlobal {
                name: "mut_global_i32",
                value: spacewasm::Box::new(MutableStaticGlobal {
                    value: Mutex::new(Value::I32(0)),
                    ty: ValType::I32,
                })
                .unwrap()
                .into_global_value_dyn(),
            },
            HostGlobal {
                name: "mut_global_i64",
                value: spacewasm::Box::new(MutableStaticGlobal {
                    value: Mutex::new(Value::I64(0)),
                    ty: ValType::I64,
                })
                .unwrap()
                .into_global_value_dyn(),
            },
            HostGlobal {
                name: "mut_global_f32",
                value: spacewasm::Box::new(MutableStaticGlobal {
                    value: Mutex::new(Value::F32(0.0)),
                    ty: ValType::F32,
                })
                .unwrap()
                .into_global_value_dyn(),
            },
            HostGlobal {
                name: "mut_global_f64",
                value: spacewasm::Box::new(MutableStaticGlobal {
                    value: Mutex::new(Value::F64(0.0)),
                    ty: ValType::F64,
                })
                .unwrap()
                .into_global_value_dyn(),
            },
        ],
        functions: vec![
            HostFunction::new(
                "return_i32_from_all_args",
                "iIfd".into(),
                "i".into(),
                |_, args| {
                    let Value::I32(v) = args[0] else {
                        unreachable!()
                    };
                    ControlFlow::Continue(Some(Value::I32(v)))
                },
            ),
            HostFunction::new("return_i64", "".into(), "I".into(), |_, _| {
                ControlFlow::Continue(Some(Value::I64(0x123456789)))
            }),
            HostFunction::new("return_f32", "".into(), "f".into(), |_, _| {
                ControlFlow::Continue(Some(Value::F32(12.5)))
            }),
            HostFunction::new("return_f64", "".into(), "d".into(), |_, _| {
                ControlFlow::Continue(Some(Value::F64(42.25)))
            }),
            HostFunction::new("noop", "".into(), "".into(), |_, _| {
                ControlFlow::Continue(None)
            }),
        ],
        memory: vec![],
        table: vec![],
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
                Action::Get { .. } => {
                    // Skip Get actions for now as they're not fully implemented
                    return;
                }
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
                Err(err) => {
                    panic!("Expected trap '{text}', got error: {err:?}")
                }
                Ok(_) => {
                    panic!("Expected trap '{text}', but execution succeeded")
                }
            },
            Action::Get { .. } => {
                panic!("Get actions not implemented yet")
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
            Action::Get { .. } => {
                panic!("Get actions not implemented yet")
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
            let module = ctx.store.modules_mut().get_mut(module_index).unwrap();
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
            Action::Get { .. } => {
                panic!("Get actions not implemented yet")
            }
        },
    }
}

fn run_wast_test_file_inner(
    test_dir: PathBuf,
    test_name: &str,
    wast_line: Arc<Mutex<Option<u32>>>,
    subtest_log: SubtestLogType,
) {
    let json_path = test_dir.join(format!("{}.json", test_name));

    let json_content = std::fs::read_to_string(&json_path)
        .unwrap_or_else(|e| panic!("Failed to read JSON file: {}: {e}", json_path.display()));

    let test_file: TestFile = serde_json::from_str(&json_content)
        .unwrap_or_else(|e| panic!("Failed to parse JSON file {}: {}", json_path.display(), e));

    let mut ctx = TestContext::new();

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

pub fn run_wast_test_file(test_name: &str) {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let source_wast_path = PathBuf::from(manifest_dir)
        .join("tests")
        .join(format!("{}.wast", test_name));

    // Create a temporary directory for generated files
    let temp_dir =
        TempDir::new().unwrap_or_else(|e| panic!("Failed to create temp directory: {e}"));
    let temp_path = temp_dir.path();

    // Extract just the filename (without directory path) for the JSON output
    let test_filename = PathBuf::from(test_name)
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .to_string();

    // Run wast2json to generate Wasm modules and JSON descriptor
    let output = ProcessCommand::new("wast2json")
        .arg(&source_wast_path)
        .arg("--enable-custom-page-sizes")
        .arg("-o")
        .arg(temp_path.join(format!("{}.json", test_filename)))
        .current_dir(temp_path)
        .output()
        .unwrap_or_else(|e| panic!("Failed to run wast2json: {e}"));

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!("wast2json failed: {}", stderr);
    }

    let wast_line = Arc::new(Mutex::new(None));
    #[allow(clippy::arc_with_non_send_sync)]
    let subtest_log = Arc::new(Mutex::new(None));

    match catch_unwind(|| {
        run_wast_test_file_inner(
            temp_path.to_path_buf(),
            &test_filename,
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
