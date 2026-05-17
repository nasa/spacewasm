use serde::{Deserialize, Serialize};
use spacewasm::{
    global_allocator, vec, AllocError, Allocator, CompilerOptions, ExportDesc, FuncRef,
    GlobalValue, GlobalValueError, HostFunction, HostGlobal, HostModule, InnerVec,
    Interpreter, InterpreterBreak, InterpreterResult, InterpreterRunner, InterpreterState, Memory,
    MemoryStatistics, Module, ParseError, ReaderError, Store, TrapReason, ValType, ValidationError,
    Value, WasmStream,
};
use std::alloc::Layout;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::ControlFlow;
use std::panic::catch_unwind;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use super::inspector::Inspector;

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

struct RustSystemAllocator {
    total: AtomicUsize,
}

impl RustSystemAllocator {
    const fn new() -> Self {
        Self {
            total: AtomicUsize::new(0),
        }
    }
}

unsafe impl Allocator for RustSystemAllocator {
    unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
        self.total.store(layout.size(), Ordering::Relaxed);
        unsafe { Ok(std::alloc::alloc(layout)) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { std::alloc::dealloc(ptr, layout) }
    }

    fn memory_statistics(&self) -> MemoryStatistics {
        MemoryStatistics {
            total_bytes: self.total.load(Ordering::Relaxed) as i32,
            pad_bytes: 0,
        }
    }
}

global_allocator!(RustSystemAllocator, RustSystemAllocator::new());

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

struct TestInstance {
    module_index: usize,
    state: InterpreterState,
}

struct TestContext {
    store: Store,
    instances: HashMap<Option<String>, TestInstance>,
    current_instance: Option<String>,
    registry: HashMap<String, Option<String>>,
}

impl TestContext {
    fn new(store: Store) -> Self {
        TestContext {
            store,
            instances: HashMap::new(),
            current_instance: None,
            registry: HashMap::new(),
        }
    }

    fn get_module(&self, index: usize) -> &Module {
        self.store
            .modules
            .get(index)
            .map(|b| &**b)
            .unwrap_or_else(|| panic!("Module at index {index} not found"))
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

            let (e, arithmetic_nan) = match value_str.as_str() {
                "nan:arithmetic" => (f32::NAN, true),
                "nan:canonical" => (f32::NAN, false),
                _ => (
                    f32::from_bits(value_str.parse::<u32>().expect("failed to parse f32 bits")),
                    false,
                ),
            };

            if arithmetic_nan {
                assert!(a.is_nan(), "Expected NaN f32, got {actual:?}");
            } else {
                // Handle exact comparisons
                assert_eq!(
                    a.to_bits(),
                    e.to_bits(),
                    "Expected f32 {} ({:08x}), got {} ({:08x})",
                    e,
                    e.to_bits(),
                    a,
                    a.to_bits()
                );
            }
        }
        "f64" => {
            let Value::F64(a) = actual else {
                panic!("Expected f64, got {actual:?}");
            };

            let (e, arithmetic_nan) = match value_str.as_str() {
                "nan:arithmetic" => (f64::NAN, true),
                "nan:canonical" => (f64::NAN, false),
                _ => (
                    f64::from_bits(value_str.parse::<u64>().expect("failed to parse f64 bits")),
                    false,
                ),
            };

            if arithmetic_nan {
                assert!(a.is_nan(), "Expected NaN f64, got {actual:?}");
            } else {
                // Handle exact comparisons
                assert_eq!(
                    a.to_bits(),
                    e.to_bits(),
                    "Expected f64 {} ({:08x}), got {} ({:08x})",
                    e,
                    e.to_bits(),
                    a,
                    a.to_bits()
                );
            }
        }
        _ => panic!("Unsupported expected value type: {}", expected.ty),
    }
}

fn load_module(ctx: &mut TestContext, module_name: Option<String>, wasm_bytes: &[u8]) {
    // Create a ByteStream
    let mut stream = ByteStream::new(wasm_bytes);

    // Generate a unique module name for the WASM module itself
    let internal_name = format!("module_{}", ctx.store.modules.len());

    // Parse and validate the module
    let module = Module::new::<256>(
        &internal_name,
        &mut stream,
        &ctx.store,
        CompilerOptions {
            allow_memory_grow: true,
        },
    )
    .unwrap_or_else(|e| panic!("Failed to parse module: {e:?}"));

    // Get memory size
    let heap_size = if let Some(m_ty) = &module.memory {
        (m_ty.0.min as usize) * 65536
    } else {
        // Default small memory if no memory section
        65536
    };

    // Allocate memory
    let memory = Memory::new(heap_size);

    // Create interpreter state
    let mut state = InterpreterState::new(1024, memory);

    // Push module to store
    let module_index = ctx.store.modules.len();
    ctx.store.modules.push(spacewasm::Box::new(module).unwrap());
    let module = ctx.get_module(module_index);

    // Initialize the state
    state
        .initialize(module)
        .unwrap_or_else(|e| panic!("Failed to initialize module: {e:?}"));

    // Use a string name for the instance key (never None)
    let instance_key = module_name.unwrap_or_else(|| internal_name.clone());

    // Store the instance
    ctx.instances.insert(
        Some(instance_key.clone()),
        TestInstance {
            module_index,
            state,
        },
    );

    // Set as current instance
    ctx.current_instance = Some(instance_key);
}

fn invoke_function(
    ctx: &mut TestContext,
    module_name: &Option<String>,
    func_name: &str,
    args: &[ValueSpec],
    test_log: Rc<RefCell<Vec<String>>>,
) -> Result<Option<Value>, InterpreterBreak> {
    // Resolve module name through registry if needed
    let resolved_module = if let Some(name) = module_name {
        ctx.registry
            .get(name)
            .cloned()
            .unwrap_or_else(|| Some(name.clone()))
    } else {
        None
    };

    // Get the instance key
    let instance_key = if resolved_module.is_some() {
        resolved_module.clone()
    } else {
        Some(
            ctx.current_instance
                .clone()
                .expect("No module instance specified for invoke"),
        )
    };

    // Get module index first
    let module_index = ctx
        .instances
        .get(&instance_key)
        .expect(&format!("Instance {instance_key:?} not found"))
        .module_index;

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
            FuncRef::Func(idx) => idx as usize,
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

    // Now create interpreter with fresh borrows
    let module = ctx.store.modules.get(module_index).unwrap();
    let func = &module.functions[func_index];
    let interpreter = Interpreter::new(&ctx.store, module);
    let instance = ctx.instances.get_mut(&instance_key).unwrap();
    interpreter.invoke(&mut instance.state, func, &params)?;

    let test_runner = Inspector {
        v: &interpreter,
        out: test_log,
    };

    test_runner
        .out
        .borrow_mut()
        .push(format!("invoke {}({:?})", func_name, params));

    // Run until completion
    let mut result = InterpreterResult::OutOfFuel;
    while result == InterpreterResult::OutOfFuel {
        result = test_runner.run(&module.text, &mut instance.state, usize::MAX);
    }

    // Check the result
    match result {
        InterpreterResult::Instruction(InterpreterBreak::Finished(raw)) => {
            if return_types.is_empty() {
                Ok(None)
            } else if return_types.len() == 1 {
                Ok(Some(raw.to_value(return_types[0])))
            } else {
                panic!("Multi-value returns not supported");
            }
        }
        InterpreterResult::Instruction(err) => Err(err),
        InterpreterResult::ReaderError(err) => panic!("Reader error: {err:?}"),
        InterpreterResult::OutOfFuel => unreachable!(),
    }
}

fn trap_reason_to_string(reason: TrapReason) -> &'static str {
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
    match reason {
        TrapReason::Unreachable => "unreachable",
        TrapReason::Host => unreachable!(),
        TrapReason::DivideByZero => "integer divide by zero",
        TrapReason::InvalidTableIndex => "out of bounds table access",
        TrapReason::InvalidTableFunctionType => "indirect call type mismatch",
        TrapReason::BrTableLookupFailed => "out of bounds table access",
        TrapReason::GlobalGetFailed => unreachable!(),
        TrapReason::GlobalSetFailed => unreachable!(),
        TrapReason::MemoryOutOfBounds => "out of bounds memory access",
        TrapReason::StackOverflow => "stack overflow",
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
        (ValidationError::MemoryImportsNotSupportedYet, "multiple memories") => {}
        (ValidationError::AlignmentLargerThanType, "alignment must not be larger than natural") => {
        }
        (ValidationError::TypeMismatch, "type mismatch") => {}
        err => {
            assert!(
                false,
                "Could not match expected error text '{text}' with error {err:?}"
            )
        }
    }
}

fn run_wast_command(
    command: Command,
    test_dir: &str,
    ctx: &mut TestContext,
    log: Rc<RefCell<Vec<String>>>,
) {
    match command {
        Command::Module { name, filename, .. } => {
            let wasm_path = format!("{test_dir}/{filename}");
            let wasm_bytes =
                std::fs::read(&wasm_path).unwrap_or_else(|e| panic!("Failed to read module: {e}"));
            load_module(ctx, name, &wasm_bytes);
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
        Command::AssertTrap { action, text, .. } => match action {
            Action::Invoke {
                module,
                field,
                args,
            } => match invoke_function(ctx, &module, &field, &args, log) {
                Err(InterpreterBreak::Trap(reason)) if text == trap_reason_to_string(reason) => {}
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
                let mut stream = ByteStream::new(&wasm_bytes);

                let err = Module::new::<256>(
                    "malformed_test",
                    &mut stream,
                    &ctx.store,
                    CompilerOptions {
                        allow_memory_grow: true,
                    },
                )
                .err()
                .expect(format!("Expected malformed module to fail with '{text}'").as_str());

                check_decode_error(err, text);
            }
        }
        Command::AssertInvalid {
            filename,
            module_type,
            text,
            ..
        } => {
            if module_type != "text" {
                let wasm_path = format!("{test_dir}/{filename}");
                let wasm_bytes = std::fs::read(&wasm_path)
                    .unwrap_or_else(|e| panic!("Failed to read {wasm_path}: {e}"));
                let mut stream = ByteStream::new(&wasm_bytes);
                let err = Module::new::<256>(
                    "malformed_test",
                    &mut stream,
                    &ctx.store,
                    CompilerOptions {
                        allow_memory_grow: true,
                    },
                )
                .err()
                .expect(format!("Expected invalid module to fail with '{text}'").as_str());

                check_decode_error(err, text);
            }
        }
        Command::AssertExhaustion { .. } => {
            // Skip exhaustion tests as stack overflow detection is not implemented
        }
        Command::Register { name, as_name, .. } => {
            // Register maps an alias to a module instance
            let instance_key = name
                .map(Some)
                .unwrap_or_else(|| ctx.current_instance.clone());
            assert!(instance_key.is_some(), "No instance to register");
            ctx.registry.insert(as_name, instance_key);
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
                // Skip Get actions for now
            }
        },
    }
}

fn run_wast_test_file_inner(
    test_dir: &str,
    test_name: &str,
    wast_line: Arc<Mutex<Option<u32>>>,
    subtest_log: Arc<Mutex<Option<Rc<RefCell<Vec<String>>>>>>,
) {
    let json_path = format!("{}/{}.json", test_dir, test_name);

    let json_content = std::fs::read_to_string(&json_path)
        .unwrap_or_else(|e| panic!("Failed to read JSON file: {}: {e}", json_path));

    let test_file: TestFile = serde_json::from_str(&json_content)
        .unwrap_or_else(|e| panic!("Failed to parse JSON file {}: {}", json_path, e));

    let store = Store::new(
        500,
        [HostModule {
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
        }],
    )
    .unwrap();

    let mut ctx = TestContext::new(store);

    for command in test_file.commands {
        let test_log = Rc::new(RefCell::new(Vec::<String>::new()));
        *subtest_log.lock().unwrap() = Some(test_log.clone());
        *wast_line.lock().unwrap() = match &command {
            Command::Module { line, .. }
            | Command::AssertReturn { line, .. }
            | Command::AssertTrap { line, .. }
            | Command::AssertMalformed { line, .. }
            | Command::AssertInvalid { line, .. }
            | Command::AssertExhaustion { line, .. }
            | Command::Register { line, .. }
            | Command::Action { line, .. } => Some(*line),
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
                let log_lines = log.borrow();
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
