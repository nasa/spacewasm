use spacewasm::{
    AllocError, Allocator, Code, ExportDesc, FuncRef, GlobalValue, GlobalValueError, HostFunction,
    HostGlobal, HostModule, InnerVec, Interpreter, InterpreterResult, InterpreterRunner,
    InterpreterState, Memory, MemoryStatistics, Module, ReaderError, Store, Stream, ValType, Value,
    global_allocator, vec,
};
use std::alloc::Layout;
use std::collections::HashMap;
use std::ops::ControlFlow;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use wast::core::{WastArgCore, WastRetCore};
use wast::parser::{self, ParseBuffer};
use wast::{QuoteWat, Wast, WastArg, WastDirective, WastExecute, WastRet};

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

// Wrapper type for implementing Stream
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

impl Stream for ByteStream {
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
}

impl TestContext {
    fn new(store: Store) -> Self {
        TestContext {
            store,
            instances: HashMap::new(),
            current_instance: None,
        }
    }

    #[allow(unused)]
    fn get_current_instance(&mut self) -> Result<&mut TestInstance, String> {
        let instance_name = &self
            .current_instance
            .clone()
            .ok_or_else(|| "No current module instance".to_string())?;
        self.instances
            .get_mut(&Some(instance_name.clone()))
            .ok_or_else(|| format!("Instance {:?} not found", instance_name))
    }

    fn get_module(&self, index: usize) -> Result<&Module, String> {
        self.store
            .modules
            .get(index)
            .map(|b| &**b)
            .ok_or_else(|| format!("Module at index {} not found", index))
    }
}

fn wast_arg_to_value(arg: &WastArg) -> Result<Value, String> {
    match arg {
        WastArg::Core(core) => match core {
            WastArgCore::I32(v) => Ok(Value::I32(*v)),
            WastArgCore::I64(v) => Ok(Value::I64(*v)),
            WastArgCore::F32(f) => Ok(Value::F32(f32::from_bits(f.bits))),
            WastArgCore::F64(f) => Ok(Value::F64(f64::from_bits(f.bits))),
            _ => Err(format!("Unsupported wast arg type: {:?}", core)),
        },
        _ => Err(format!("Unsupported wast arg: {:?}", arg)),
    }
}

#[allow(unused)]
fn wast_ret_to_value(ret: &WastRet) -> Result<Value, String> {
    match ret {
        WastRet::Core(core) => match core {
            WastRetCore::I32(v) => Ok(Value::I32(*v)),
            WastRetCore::I64(v) => Ok(Value::I64(*v)),
            WastRetCore::F32(f) => match f {
                wast::core::NanPattern::CanonicalNan => {
                    Err("Cannot convert canonical NaN pattern to value".to_string())
                }
                wast::core::NanPattern::ArithmeticNan => {
                    Err("Cannot convert arithmetic NaN pattern to value".to_string())
                }
                wast::core::NanPattern::Value(bits) => Ok(Value::F32(f32::from_bits(bits.bits))),
            },
            WastRetCore::F64(f) => match f {
                wast::core::NanPattern::CanonicalNan => {
                    Err("Cannot convert canonical NaN pattern to value".to_string())
                }
                wast::core::NanPattern::ArithmeticNan => {
                    Err("Cannot convert arithmetic NaN pattern to value".to_string())
                }
                wast::core::NanPattern::Value(bits) => Ok(Value::F64(f64::from_bits(bits.bits))),
            },
            _ => Err(format!("Unsupported wast ret type: {:?}", core)),
        },
        _ => Err(format!("Unsupported wast ret: {:?}", ret)),
    }
}

fn compare_values(actual: Value, expected: &WastRet) -> Result<(), String> {
    match expected {
        WastRet::Core(core) => match core {
            WastRetCore::I32(v) => {
                if let Value::I32(a) = actual {
                    if a == *v {
                        Ok(())
                    } else {
                        Err(format!("Expected i32 {}, got {}", v, a))
                    }
                } else {
                    Err(format!("Expected i32, got {:?}", actual))
                }
            }
            WastRetCore::I64(v) => {
                if let Value::I64(a) = actual {
                    if a == *v {
                        Ok(())
                    } else {
                        Err(format!("Expected i64 {}, got {}", v, a))
                    }
                } else {
                    Err(format!("Expected i64, got {:?}", actual))
                }
            }
            WastRetCore::F32(f) => {
                if let Value::F32(a) = actual {
                    match f {
                        wast::core::NanPattern::CanonicalNan => {
                            if a.is_nan() && (a.to_bits() & 0x7FC00000) == 0x7FC00000 {
                                Ok(())
                            } else {
                                Err(format!("Expected canonical NaN, got {}", a))
                            }
                        }
                        wast::core::NanPattern::ArithmeticNan => {
                            if a.is_nan() {
                                Ok(())
                            } else {
                                Err(format!("Expected arithmetic NaN, got {}", a))
                            }
                        }
                        wast::core::NanPattern::Value(bits) => {
                            let expected_f32 = f32::from_bits(bits.bits);
                            if a.to_bits() == expected_f32.to_bits() {
                                Ok(())
                            } else {
                                Err(format!("Expected f32 {}, got {}", expected_f32, a))
                            }
                        }
                    }
                } else {
                    Err(format!("Expected f32, got {:?}", actual))
                }
            }
            WastRetCore::F64(f) => {
                if let Value::F64(a) = actual {
                    match f {
                        wast::core::NanPattern::CanonicalNan => {
                            if a.is_nan()
                                && (a.to_bits() & 0x7FF8000000000000) == 0x7FF8000000000000
                            {
                                Ok(())
                            } else {
                                Err(format!("Expected canonical NaN, got {}", a))
                            }
                        }
                        wast::core::NanPattern::ArithmeticNan => {
                            if a.is_nan() {
                                Ok(())
                            } else {
                                Err(format!("Expected arithmetic NaN, got {}", a))
                            }
                        }
                        wast::core::NanPattern::Value(bits) => {
                            let expected_f64 = f64::from_bits(bits.bits);
                            if a.to_bits() == expected_f64.to_bits() {
                                Ok(())
                            } else {
                                Err(format!("Expected f64 {}, got {}", expected_f64, a))
                            }
                        }
                    }
                } else {
                    Err(format!("Expected f64, got {:?}", actual))
                }
            }
            _ => Err(format!(
                "Unsupported wast ret type for comparison: {:?}",
                core
            )),
        },
        _ => Err(format!(
            "Unsupported wast ret for comparison: {:?}",
            expected
        )),
    }
}

fn load_module(
    ctx: &mut TestContext,
    module_name: Option<String>,
    wat: &mut QuoteWat,
) -> Result<(), String> {
    // Encode WAT to WASM bytes
    let wasm_bytes = wat
        .encode()
        .map_err(|e| format!("Failed to encode WAT: {}", e))?;

    // Create a ByteStream
    let mut stream = ByteStream::new(&wasm_bytes);

    // Generate a unique module name for the WASM module itself
    let internal_name = format!("module_{}", ctx.store.modules.len());

    // Parse and validate the module
    let module = Module::new::<256>(&internal_name, &mut stream, &ctx.store)
        .map_err(|e| format!("Failed to parse module: {:?}", e))?;

    // Get memory size
    let heap_size = if module.memories.is_empty() {
        // Default small memory if no memory section
        65536
    } else {
        (module.memories[0].0.min as usize) * 65536
    };

    // Allocate memory
    let memory = Memory::new(heap_size);

    // Create interpreter state
    let mut state = InterpreterState::new(1024, memory);

    // Push module to store
    let module_index = ctx.store.modules.len();
    ctx.store.modules.push(spacewasm::Box::new(module).unwrap());
    let module = ctx.get_module(module_index)?;

    // Initialize the state
    state
        .initialize(module)
        .map_err(|e| format!("Failed to initialize module: {:?}", e))?;

    // Use a string name for the instance key (never None)
    let instance_key = module_name.clone().unwrap_or_else(|| internal_name.clone());

    // Store the instance (Code is created on-demand during invoke)
    ctx.instances.insert(
        Some(instance_key.clone()),
        TestInstance {
            module_index,
            state,
        },
    );

    // Set as current instance
    ctx.current_instance = Some(instance_key);

    Ok(())
}

fn invoke_function(
    ctx: &mut TestContext,
    module_name: &Option<String>,
    func_name: &str,
    args: &[WastArg],
) -> Result<Option<Value>, String> {
    // Get the instance key
    let instance_key = if module_name.is_some() {
        module_name.clone()
    } else {
        Some(
            ctx.current_instance
                .clone()
                .ok_or_else(|| "No module instance specified for invoke".to_string())?,
        )
    };

    // Get module index first
    let module_index = ctx
        .instances
        .get(&instance_key)
        .ok_or_else(|| format!("Instance {:?} not found", instance_key))?
        .module_index;

    // Scope the immutable borrow of ctx
    let (func_index, return_types, params) = {
        let module = ctx.get_module(module_index)?;

        // Find the exported function
        let export = module
            .exports
            .iter()
            .find(|e| e.name == func_name)
            .ok_or_else(|| format!("Function {} not found in exports", func_name))?;

        let func_idx = match &export.desc {
            ExportDesc::Func(idx) => *idx,
            _ => return Err(format!("{} is not a function export", func_name)),
        };

        // Get the function reference
        let func_ref = module
            .get_func_ref(func_idx)
            .map_err(|e| format!("Failed to get function reference: {:?}", e))?;

        let func_index = match func_ref {
            FuncRef::Func(idx) => idx as usize,
            _ => return Err(format!("Cannot invoke host function {}", func_name)),
        };

        // Get all the immutable data we need
        let func = &module.functions[func_index];
        let func_type = &module.types[func.ty.0 as usize];
        let return_types = func_type.returns.clone();

        // Convert arguments
        let params: Result<Vec<Value>, String> = args.iter().map(wast_arg_to_value).collect();
        let params = params?;

        (func_index, return_types, params)
    }; // immutable borrow of ctx ends here

    // Now create interpreter with fresh borrows
    let module = ctx.store.modules.get(module_index).unwrap();
    let func = &module.functions[func_index];
    let interpreter = Interpreter::new(&ctx.store, module);
    let code = Code::new(&module.text);
    let instance = ctx.instances.get_mut(&instance_key).unwrap();
    interpreter.invoke(&mut instance.state, func, &params);

    // Run until completion
    let mut result = InterpreterResult::OutOfFuel;
    while result == InterpreterResult::OutOfFuel {
        result = interpreter.run(&code, &mut instance.state, usize::MAX);
    }

    // Check the result
    match result {
        InterpreterResult::Finished => {
            if return_types.is_empty() {
                Ok(None)
            } else {
                Err("Function finished but return value extraction not yet implemented".to_string())
            }
        }
        InterpreterResult::Instruction(spacewasm::InstructionError::Finished(raw)) => {
            if return_types.is_empty() {
                Ok(None)
            } else if return_types.len() == 1 {
                Ok(Some(raw.to_value(return_types[0])))
            } else {
                Err("Multi-value returns not yet supported".to_string())
            }
        }
        InterpreterResult::Instruction(err) => Err(format!("Execution failed: {:?}", err)),
        InterpreterResult::ReaderError(err) => Err(format!("Reader error: {:?}", err)),
        InterpreterResult::OutOfFuel => unreachable!(),
    }
}

pub fn run_wast_test_file(file_name: &str) {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let wast_path = format!("{}/tests/spec/test/core/{}.wast", manifest_dir, file_name);

    let wast_content = std::fs::read_to_string(&wast_path)
        .expect(&format!("failed to read wast file: {}", wast_path));

    let buf = ParseBuffer::new(&wast_content).expect("failed to create parse buffer");

    let wast: Wast = parser::parse(&buf).expect("failed to parse wast");

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

    for dir in wast.directives {
        let error_opt = match dir {
            WastDirective::Module(mut module) => {
                let span = module.span();
                match load_module(&mut ctx, None, &mut module) {
                    Ok(()) => None,
                    Err(e) => Some((span, format!("Module: {}", e))),
                }
            }
            WastDirective::ModuleDefinition(mut module) => {
                let span = module.span();
                match load_module(&mut ctx, None, &mut module) {
                    Ok(()) => None,
                    Err(e) => Some((span, format!("ModuleDefinition: {}", e))),
                }
            }
            WastDirective::ModuleInstance { span, .. } => {
                Some((span, "ModuleInstance directive not yet implemented".to_string()))
            }
            WastDirective::AssertMalformed {
                span,
                mut module,
                message: _,
            } => {
                match module.encode() {
                    Ok(_) => Some((span, "Expected malformed module to fail encoding".to_string())),
                    Err(_) => None, // Expected
                }
            }
            WastDirective::AssertInvalid {
                span,
                mut module,
                message: _,
            } => {
                match module.encode() {
                    Ok(bytes) => {
                        let mut stream = ByteStream::new(&bytes);
                        match Module::new::<256>("invalid_test", &mut stream, &ctx.store) {
                            Ok(_) => Some((span, "Expected invalid module to fail validation".to_string())),
                            Err(_) => None, // Expected
                        }
                    }
                    Err(e) => Some((span, format!("Module encoding failed: {}", e))),
                }
            }
            WastDirective::AssertInvalidCustom { span, .. } => {
                Some((span, "AssertInvalidCustom directive not yet implemented".to_string()))
            }
            WastDirective::Register { span, .. } => {
                Some((span, "Register directive not yet fully implemented".to_string()))
            }
            WastDirective::Invoke(invoke) => {
                let span = invoke.span;
                match invoke_function(&mut ctx, &invoke.module.map(|m| m.name().to_string()), invoke.name, &invoke.args) {
                    Ok(_) => None,
                    Err(e) => Some((span, format!("Invoke '{}': {}", invoke.name, e))),
                }
            }
            WastDirective::AssertTrap { span, exec, message: _ } => {
                match exec {
                    WastExecute::Invoke(invoke) => {
                        let result = invoke_function(&mut ctx, &invoke.module.map(|m| m.name().to_string()), invoke.name, &invoke.args);
                        match result {
                            // Any execution error is considered a trap
                            Err(msg) if msg.contains("Trap") || msg.contains("MemoryOutOfBounds") || msg.contains("Execution failed") => None,
                            Err(msg) => Some((span, format!("AssertTrap '{}': Expected trap, got error: {}", invoke.name, msg))),
                            Ok(_) => Some((span, format!("AssertTrap '{}': Expected trap, but execution succeeded", invoke.name))),
                        }
                    }
                    WastExecute::Wat(_) => {
                        Some((span, "AssertTrap with Wat not yet implemented".to_string()))
                    }
                    WastExecute::Get { .. } => {
                        Some((span, "AssertTrap with Get not yet implemented".to_string()))
                    }
                }
            }
            WastDirective::AssertReturn { span, exec, results } => {
                match exec {
                    WastExecute::Invoke(invoke) => {
                        match invoke_function(&mut ctx, &invoke.module.map(|m| m.name().to_string()), invoke.name, &invoke.args) {
                            Ok(result) => {
                                if results.is_empty() {
                                    if result.is_some() {
                                        Some((span, format!("AssertReturn '{}': Expected no return value, got {:?}", invoke.name, result)))
                                    } else {
                                        None
                                    }
                                } else if results.len() == 1 {
                                    match result {
                                        Some(actual) => match compare_values(actual, &results[0]) {
                                            Ok(()) => None,
                                            Err(e) => Some((span, format!("AssertReturn '{}': {}", invoke.name, e))),
                                        },
                                        None => Some((span, format!("AssertReturn '{}': Expected return value, got none", invoke.name))),
                                    }
                                } else {
                                    Some((span, format!("AssertReturn '{}': Multi-value returns not yet supported", invoke.name)))
                                }
                            }
                            Err(e) => Some((span, format!("AssertReturn '{}': {}", invoke.name, e))),
                        }
                    }
                    WastExecute::Wat(_) => {
                        Some((span, "AssertReturn with Wat not yet implemented".to_string()))
                    }
                    WastExecute::Get { .. } => {
                        Some((span, "AssertReturn with Get not yet implemented".to_string()))
                    }
                }
            }
            WastDirective::AssertExhaustion { span, .. } => {
                Some((span, "AssertExhaustion not supported: stack overflow detection not yet implemented in spacewasm".to_string()))
            }
            WastDirective::AssertUnlinkable { span, .. } => {
                Some((span, "AssertUnlinkable directive not yet implemented".to_string()))
            }
            WastDirective::AssertException { span, .. } => {
                Some((span, "AssertException not supported: exceptions are not implemented in spacewasm".to_string()))
            }
            WastDirective::AssertSuspension { span, .. } => {
                Some((span, "AssertSuspension not supported: threading is not implemented in spacewasm".to_string()))
            }
            WastDirective::Thread(t) => {
                Some((t.span, "Thread directive not supported: threading is not implemented in spacewasm".to_string()))
            }
            WastDirective::Wait { span, .. } => {
                Some((span, "Wait directive not supported: threading is not implemented in spacewasm".to_string()))
            }
            WastDirective::AssertMalformedCustom { span, .. } => {
                Some((span, "AssertMalformedCustom directive not yet implemented".to_string()))
            }
        };

        if let Some((span, msg)) = error_opt {
            let (line, col) = span.linecol_in(&wast_content);
            panic!("{}:{}:{}: {}", wast_path, line + 1, col + 1, msg);
        }
    }
}
