use super::inspector::{Inspector, LimitedVec};
use serde::{Deserialize, Serialize};
use spacewasm::{
    global_allocator, vec, AllocError, Allocator, CodeBuilder, CompilerOptions, ConstantExprError,
    Error, ExportDesc, GlobalValue, GlobalValueError, HostFunction, HostGlobal,
    HostModule, InitializeError, InitializeResult, InnerVec, Interpreter,
    InterpreterBreak, InterpreterResult, InterpreterRunner, InterpreterState, MemoryError, MemoryStatistics,
    Module, ModuleRef, ParseError, ReaderError, Ref, Store, StoreLinker, TrapReason, ValType,
    ValidationError, Value, WasmMemoryAllocator, WasmRef, WasmStream,
};
use std::alloc::Layout;
use std::cell::RefCell;
use std::ops::ControlFlow;
use std::panic::catch_unwind;
use std::ptr::NonNull;
use std::rc::Rc;
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
        unsafe { Ok(NonNull::new(std::alloc::alloc(layout)).ok_or(AllocError::AllocationFailed)?) }
    }

    fn reallocate(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        layout: Layout,
    ) -> Result<NonNull<u8>, AllocError> {
        unsafe {
            Ok(
                NonNull::new(std::alloc::realloc(ptr.as_ptr(), old_layout, layout.size()))
                    .ok_or(AllocError::AllocationFailed)?,
            )
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

impl WasmStream for ByteStream {
    fn read(&mut self) -> Result<Option<InnerVec<u8>>, ReaderError> {
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

struct TestContext {
    store: Store,
    module_index: usize,
    code_builder: CodeBuilder<256>,
}

impl TestContext {
    fn new() -> Self {
        TestContext {
            store: Store::default(),
            module_index: 0,
            code_builder: CodeBuilder::<256>::default(),
        }
    }

    fn current_module_index(&self) -> usize {
        if self.store.modules.len() == 0 {
            0
        } else {
            self.store.modules.len() - 1
        }
    }

    fn get_module(&self, index: usize) -> &Module {
        self.store
            .modules
            .get(index)
            .map(|b| &**b)
            .unwrap_or_else(|| panic!("Module at index {index} not found"))
    }

    fn find_module_by_name(&self, name: &str) -> Option<usize> {
        self.store.modules.iter().position(|m| m.name == name)
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
enum ModuleLoadError {
    DecodeError(ParseError),
    AllocationError(MemoryError),
    InitializeError(InitializeError),
}

impl From<ParseError> for ModuleLoadError {
    fn from(e: ParseError) -> Self {
        ModuleLoadError::DecodeError(e)
    }
}

impl From<InitializeError> for ModuleLoadError {
    fn from(value: InitializeError) -> Self {
        ModuleLoadError::InitializeError(value)
    }
}

impl From<MemoryError> for ModuleLoadError {
    fn from(value: MemoryError) -> Self {
        ModuleLoadError::AllocationError(value)
    }
}

fn load_module(
    ctx: &mut TestContext,
    module_name: Option<String>,
    wasm_bytes: &[u8],
) -> Result<(), ModuleLoadError> {
    // Create a ByteStream
    let mut stream = ByteStream::new(wasm_bytes);

    // We are loading a new module. We need to construct a new store using the old modules
    // Move modules back from store to a new linker

    // Create new linker with the preserved modules
    let mut new_linker = StoreLinker::new(254, []).unwrap();
    if ctx.store.modules.capacity() > 0 {
        new_linker.modules = ctx.store.modules.clone();
    }

    new_linker.host_modules = test_host_module();

    // If the last module that got loaded has an empty name "", this means that it should not be
    // kept around in the store since it cannot actually be referenced. It was only the "active"
    // module and probably ran some invoke/assert_return stuff but should not remain in the store.
    if let Some(module) = new_linker.modules.pop() {
        if module.name != "" {
            // It is possible to reference this module in the future
            // Add it back to the list
            new_linker.modules.push(module);
        }
    }

    // Parse and validate the module
    let module = Module::new::<256>(
        module_name.as_ref().map(|f| f.as_ref()).unwrap_or(""),
        &mut stream,
        &new_linker,
        &mut ctx.code_builder,
        CompilerOptions {
            allow_memory_grow: true,
        },
    )?;

    // Push module to linker
    let module_index = new_linker.modules.len();
    new_linker
        .modules
        .push(spacewasm::Box::new(module).unwrap());

    // Finish the code builder to get the text
    let (text, _final_page_offset) = ctx.code_builder.clone().finish().unwrap();

    // Initialize the linker into a store
    let mut store = new_linker.allocate(&RustSystemAllocator)?;
    let mut state = InterpreterState::new(1024);
    ctx.store = loop {
        store = match store.initialize(&text, &mut state, usize::MAX)? {
            InitializeResult::Finished(store) => break store,
            InitializeResult::Continue(c) => c,
        }
    };

    // Update module index - don't finish the linker yet, as more modules might be loaded
    ctx.module_index = module_index;
    Ok(())
}

fn invoke_function(
    ctx: &mut TestContext,
    module_name: &Option<String>,
    func_name: &str,
    args: &[ValueSpec],
    test_log: Rc<RefCell<LimitedVec<String>>>,
) -> Result<Option<Value>, InterpreterBreak> {
    // Resolve module index by name lookup or use current (last) module
    let module_index = if let Some(name) = module_name {
        ctx.find_module_by_name(name)
            .unwrap_or_else(|| panic!("Module '{name}' not found"))
    } else {
        ctx.current_module_index()
    };

    // Scope the immutable borrow of ctx
    let (func_index, return_types, params) = {
        let module = ctx.get_module(module_index);

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
            .expect(&format!("Function {} not found in exports", func_name));

        let func_index = match func_ref {
            Ref::Module(idx) => idx as usize,
            _ => panic!("Function {} is not a function export", func_name),
        };

        // Get all the immutable data we need
        let func = &module.functions[func_index];
        let func_type = &module.types[func.ty.0 as usize];
        let return_types = func_type.returns.clone();

        // Convert arguments
        let params: Vec<Value> = args.iter().map(parse_value).collect();

        (func_index, return_types, params)
    };

    // Create a temporary interpreter by moving the store
    // We'll move it back after the function completes
    let interpreter = Interpreter::new(core::mem::take(&mut ctx.store));

    let mut state = InterpreterState::new(1024);
    state.invoke(
        &interpreter.store,
        WasmRef {
            module: ModuleRef(module_index as u8),
            index: func_index as u16,
        },
        &params,
    )?;

    let test_runner = Inspector {
        v: &interpreter,
        out: test_log,
    };

    test_runner
        .out
        .borrow_mut()
        .push(format!("invoke {}({:?})", func_name, params));

    // Run until completion
    // Run up to 1-million instructions to catch infinite loops
    let (text, _final_page_offset) = ctx.code_builder.clone().finish().unwrap();
    let result = test_runner.run(&text, &mut state, 10000000);

    // Move the store back to the context
    ctx.store = interpreter.store;
    ctx.get_module(module_index)
        .return_memory(&ctx.store, state.memory);

    // Check the result
    match result {
        InterpreterResult::Instruction(InterpreterBreak::Finished) => {
            if return_types.is_empty() {
                Ok(None)
            } else if return_types.len() == 1 {
                Ok(Some(state.result.unwrap().to_value(return_types[0])))
            } else {
                panic!("Multi-value returns not supported");
            }
        }
        InterpreterResult::Instruction(err) => Err(err),
        InterpreterResult::ReaderError(err) => panic!("Reader error: {err:?}"),
        InterpreterResult::OutOfFuel => panic!("Infinite loop detected"),
    }
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
        err => {
            assert!(
                false,
                "Could not match expected trap text '{text}' with error {err:?}"
            )
        }
    }
}

fn check_decode_error(err: Error, text: String) {
    match err {
        Error::Parse(err) => match (err.err.err, text.as_str()) {
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
            (
                ValidationError::AlignmentLargerThanType,
                "alignment must not be larger than natural",
            ) => {}
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
            (ValidationError::BrTableHasTooManyCases, "br.table has too many cases") => {}
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
            (ValidationError::InvalidConstantExpr(ConstantExprError::NoValue), "type mismatch") => {
            }
            (
                ValidationError::InvalidConstantExpr(ConstantExprError::InvalidGlobal),
                "unknown global",
            ) => {}
            (ValidationError::ExpectedConstOrVar(_), "malformed mutability") => {}
            (ValidationError::MemoryNotDefined, "unknown memory") => {}
            (ValidationError::InvalidMaxLimit, "size minimum must not be greater than maximum") => {
            }
            (ValidationError::MemoryTooLarge, "memory size must be at most 65536 pages (4GiB)") => {
            }
            (ValidationError::InvalidNegativeMemOffset, "data segment does not fit") => {}
            (ValidationError::InvalidMemOffsetType, "type mismatch") => {}
            (ValidationError::InvalidStartFunctionSignature, "start function") => {}
            (ValidationError::DuplicateExportName, "duplicate export name") => {}
            (ValidationError::InvalidTableIndex, "unknown table") => {}
            (ValidationError::MemoryIdxTooLarge, "unknown memory") => {}
            err => {
                assert!(
                    false,
                    "Could not match validation error text '{text}' with error {err:?}"
                )
            }
        },
        Error::Memory(err) => match (err, text.as_str()) {
            (MemoryError::OutOfBounds, "data segment does not fit") => {}
            err => {
                assert!(
                    false,
                    "Could not match validation error text '{text}' with error {err:?}"
                )
            }
        },
    }
}

fn check_initialization_error(err: InitializeError, text: &str) {
    match (err, text) {
        (InitializeError::Trap(TrapReason::Unreachable), "unreachable") => {}
        err => {
            assert!(
                false,
                "Could not match initialization error text '{text}' with error {err:?}"
            )
        }
    }
}

fn test_host_module() -> spacewasm::Vec<HostModule> {
    vec![HostModule {
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
        memory: Some(spacewasm::MemType::from(1, Some(2))),
    }]
}

fn run_wast_command(
    command: Command,
    test_dir: &str,
    ctx: &mut TestContext,
    log: Rc<RefCell<LimitedVec<String>>>,
) {
    match command {
        Command::Module { name, filename, .. } => {
            let wasm_path = format!("{test_dir}/{filename}");
            let wasm_bytes =
                std::fs::read(&wasm_path).unwrap_or_else(|e| panic!("Failed to read module: {e}"));
            load_module(ctx, name, &wasm_bytes).unwrap();
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
                let wasm_path = format!("{test_dir}/{filename}");
                let wasm_bytes = std::fs::read(&wasm_path)
                    .unwrap_or_else(|e| panic!("Failed to read module: {e}"));

                // Clone the old store to restore it after we lose this one
                // The problem is we need to do a "partial" clone since the host_modules are not clonable.
                let prev_store = Store {
                    modules: ctx.store.modules.clone(),
                    host_modules: test_host_module(),
                    memory: ctx.store.memory.clone(),
                };

                match load_module(ctx, None, &wasm_bytes) {
                    Ok(_) => {
                        panic!("Expected error when linking/initializing module");
                    }
                    Err(ModuleLoadError::InitializeError(err)) => {
                        check_initialization_error(err, &text);

                        // Restore the previous store since this one is now wiped out.
                        ctx.store = prev_store;
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
                Err(InterpreterBreak::Trap(reason)) => check_trap_reason(reason, &text),
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
            // Skip text format tests as we only handle binary WASM
            if module_type != "text" {
                let wasm_path = format!("{test_dir}/{filename}");
                let wasm_bytes = std::fs::read(&wasm_path).unwrap();

                match load_module(ctx, None, &wasm_bytes) {
                    Err(ModuleLoadError::DecodeError(err)) => check_decode_error(err.into(), text),
                    _ => {
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
                let wasm_path = format!("{test_dir}/{filename}");
                let wasm_bytes = std::fs::read(&wasm_path)
                    .unwrap_or_else(|e| panic!("Failed to read {wasm_path}: {e}"));

                match load_module(ctx, None, &wasm_bytes) {
                    Err(ModuleLoadError::DecodeError(err)) => check_decode_error(err.into(), text),
                    Err(ModuleLoadError::AllocationError(err)) => {
                        check_decode_error(err.into(), text)
                    }
                    _ => {
                        panic!("Expected error when decoding module");
                    }
                }
            }
        }
        Command::AssertExhaustion { .. } => {
            // todo!()
        }
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
            let module = ctx.store.modules.get_mut(module_index).unwrap();
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
                // todo!()
            }
        },
    }
}

fn run_wast_test_file_inner(
    test_dir: &str,
    test_name: &str,
    wast_line: Arc<Mutex<Option<u32>>>,
    subtest_log: Arc<Mutex<Option<Rc<RefCell<LimitedVec<String>>>>>>,
) {
    let json_path = format!("{}/{}.json", test_dir, test_name);

    let json_content = std::fs::read_to_string(&json_path)
        .unwrap_or_else(|e| panic!("Failed to read JSON file: {}: {e}", json_path));

    let test_file: TestFile = serde_json::from_str(&json_content)
        .unwrap_or_else(|e| panic!("Failed to parse JSON file {}: {}", json_path, e));

    let mut ctx = TestContext::new();
    ctx.store.host_modules = test_host_module();

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
    let test_dir = format!("{}/tests/spectest/wasm-1.0/{}", manifest_dir, test_name);
    let wast_path = format!("{test_dir}/{test_name}.wast");

    let wast_line = Arc::new(Mutex::new(None));
    let subtest_log = Arc::new(Mutex::new(None));

    match catch_unwind(|| {
        run_wast_test_file_inner(&test_dir, test_name, wast_line.clone(), subtest_log.clone())
    }) {
        Ok(_) => {}
        Err(err) => {
            if let Some(log) = &*subtest_log.lock().unwrap() {
                let log_lines: Vec<String> = log.borrow().clone().into();
                if log_lines.len() > 0 {
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
                panic!("{wast_path}:{line_no}: {msg}")
            } else {
                panic!("{wast_path}: {msg}")
            }
        }
    }
}
