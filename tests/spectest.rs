use spacewasm::{
    global_allocator, AllocError, Allocator, Code, ExportDesc, InnerVec, InterpreterResult, InterpreterState,
    Memory, MemoryStatistics, Module, ModuleImports, ReaderError, Stream, Value,
};
use std::alloc::Layout;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use wast::core::{NanPattern, WastArgCore, WastRetCore};
use wast::parser::{self, ParseBuffer};
use wast::{QuoteWat, Wast, WastArg, WastDirective, WastExecute, WastInvoke, WastRet, Wat};

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

pub struct TestContext {
    modules: HashMap<Option<String>, TestModule>,
    current_module: Option<String>,
    imports: ModuleImports<'static>,
}

pub struct TestModule {
    pub module: spacewasm::Box<Module<'static>>,
    pub state: InterpreterState,
    // Keep the wasm bytes alive for the lifetime of the module
    _wasm_bytes: Vec<u8>,
}

impl TestContext {
    pub fn new(imports: ModuleImports<'static>) -> Self {
        Self {
            modules: HashMap::new(),
            current_module: None,
            imports,
        }
    }

    pub fn load_module_from_bytes(
        &mut self,
        name: Option<String>,
        wasm_bytes: &[u8],
    ) -> Result<(), String> {
        // Keep a copy of the wasm bytes to ensure they outlive the module
        let wasm_bytes_copy = wasm_bytes.to_vec();

        let mut stream = ByteStream::new(&wasm_bytes_copy);
        let module = spacewasm::Box::new(
            Module::new::<256>(&mut stream, self.imports.clone())
                .map_err(|e| format!("failed to parse module: {:?}", e))?,
        )
            .unwrap();

        let heap_size = if module.memories.is_empty() {
            65536
        } else {
            let mem = &module.memories[0];
            (mem.0.min as usize) * 65536
        };

        let mut state = InterpreterState::new(
            1024,
            Memory::from(
                unsafe { std::alloc::alloc(Layout::from_size_align(heap_size, 64).unwrap()) },
                heap_size,
            ),
        );

        state
            .initialize(&module.globals, &module.data)
            .map_err(|e| format!("failed to initialize state: {:?}", e))?;

        let test_module = TestModule {
            module,
            state,
            _wasm_bytes: wasm_bytes_copy,
        };

        // If we're replacing an existing module, forget it to avoid deallocation
        // The GlobalAllocator doesn't support dealloc/alloc cycles
        if let Some(old_module) = self.modules.insert(name.clone(), test_module) {
            std::mem::forget(old_module);
        }
        self.current_module = name;
        Ok(())
    }

    pub fn invoke(
        &mut self,
        module_name: Option<&str>,
        func_name: &str,
        args: &[Value],
    ) -> Result<Vec<Value>, String> {
        let module_name = module_name
            .map(|s| s.to_string())
            .or_else(|| self.current_module.clone());
        let test_module = self
            .modules
            .get_mut(&module_name)
            .ok_or_else(|| "module not found".to_string())?;

        let export = test_module
            .module
            .exports
            .iter()
            .find(|e| &e.name == func_name)
            .ok_or_else(|| format!("function {} not found", func_name))?;

        let ExportDesc::Func(fi) = export.desc else {
            return Err(format!("{} is not a function", func_name));
        };

        let interpreter = spacewasm::Interpreter::new(
            &test_module.module.functions,
            self.imports.globals,
            self.imports.functions,
            self.imports.memories,
            &test_module.module.table,
            &test_module.module.types,
        );

        let func_idx = if (fi.0 as usize) < self.imports.functions.len() {
            return Err("host function invocation not supported".to_string());
        } else {
            (fi.0 - self.imports.functions.len() as u32) as usize
        };

        let func = &test_module.module.functions[func_idx];

        interpreter.invoke(&mut test_module.state, func, args);

        // Create Code wrapper from a reference to the module's text pages
        let code = Code::new(&test_module.module.text);

        let mut result = InterpreterResult::OutOfFuel;
        while result == InterpreterResult::OutOfFuel {
            result = interpreter.run(&code, &mut test_module.state, usize::MAX);
        }

        match result {
            InterpreterResult::Finished => Ok(Vec::new()),
            InterpreterResult::Instruction(spacewasm::InstructionError::Finished(raw_value)) => {
                // Extract return value using function's return type
                let func_type = &test_module.module.types[func.ty.0 as usize];
                let return_values: Vec<Value> = match func_type.returns.len() {
                    0 => Vec::new(),
                    1 => vec![raw_value.to_value(func_type.returns[0])],
                    _ => {
                        // Multiple return values not yet supported
                        return Err("multiple return values not supported".to_string());
                    }
                };
                Ok(return_values)
            }
            InterpreterResult::Instruction(_) => Err("trap".to_string()),
            InterpreterResult::ReaderError(_) => Err("reader error".to_string()),
            InterpreterResult::OutOfFuel => unreachable!(),
        }
    }
}

impl Drop for TestContext {
    fn drop(&mut self) {
        // Prevent dropping modules to avoid GlobalAllocator deallocation assertions
        // The GlobalAllocator doesn't support dealloc/alloc cycles
        // Modules contain Vecs with GlobalAllocator, and InterpreterState contains Stack with GlobalAllocator
        // Read and forget the entire HashMap without dropping it
        unsafe {
            let modules = std::ptr::read(&self.modules);
            std::mem::forget(modules);
        }
    }
}

fn wast_arg_to_value(arg: &WastArg) -> Value {
    match arg {
        WastArg::Core(core) => match core {
            WastArgCore::I32(v) => Value::I32(*v),
            WastArgCore::I64(v) => Value::I64(*v),
            WastArgCore::F32(f) => Value::F32(f32::from_bits(f.bits)),
            WastArgCore::F64(f) => Value::F64(f64::from_bits(f.bits)),
            WastArgCore::V128(_) => panic!("V128 not supported"),
            WastArgCore::RefNull(_) => panic!("RefNull not supported"),
            WastArgCore::RefExtern(_) => panic!("RefExtern not supported"),
            WastArgCore::RefHost(_) => panic!("RefHost not supported"),
        },
        _ => panic!("Component model args not supported"),
    }
}

fn values_match(actual: &Value, expected: &WastRet) -> bool {
    let core = match expected {
        WastRet::Core(c) => c,
        _ => return false,
    };

    match (actual, core) {
        (Value::I32(a), WastRetCore::I32(e)) => a == &(*e as i32),
        (Value::I64(a), WastRetCore::I64(e)) => a == &(*e as i64),
        (Value::F32(a), WastRetCore::F32(pattern)) => match pattern {
            NanPattern::CanonicalNan | NanPattern::ArithmeticNan => a.is_nan(),
            NanPattern::Value(f) => {
                if a.is_nan() && f.bits == f32::NAN.to_bits() {
                    true
                } else {
                    a.to_bits() == f.bits
                }
            }
        },
        (Value::F64(a), WastRetCore::F64(pattern)) => match pattern {
            NanPattern::CanonicalNan | NanPattern::ArithmeticNan => a.is_nan(),
            NanPattern::Value(f) => {
                if a.is_nan() && f.bits == f64::NAN.to_bits() {
                    true
                } else {
                    a.to_bits() == f.bits
                }
            }
        },
        _ => false,
    }
}

#[derive(Debug, Default)]
pub struct TestResult {
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub failures: Vec<String>,
}

fn create_spectest_imports() -> ModuleImports<'static> {
    use spacewasm::{
        GlobalImport, GlobalValue, GlobalValueError, HostFunction, MemoryImport, ValType,
    };
    use std::ops::ControlFlow;
    use std::sync::Mutex;

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

    // These globals/functions/memories are used by the spec test suite
    // See: https://github.com/WebAssembly/spec/tree/main/interpreter#spectest-host-module
    let globals = vec![
        GlobalImport {
            module: "spectest",
            name: "global_i32",
            value: spacewasm::Box::new(StaticGlobal {
                value: Mutex::new(Value::I32(666)),
                ty: ValType::I32,
            })
                .unwrap()
                .into_global_value_dyn(),
        },
        GlobalImport {
            module: "spectest",
            name: "global_i64",
            value: spacewasm::Box::new(StaticGlobal {
                value: Mutex::new(Value::I64(666)),
                ty: ValType::I64,
            })
                .unwrap()
                .into_global_value_dyn(),
        },
        GlobalImport {
            module: "spectest",
            name: "global_f32",
            value: spacewasm::Box::new(StaticGlobal {
                value: Mutex::new(Value::F32(666.6)),
                ty: ValType::F32,
            })
                .unwrap()
                .into_global_value_dyn(),
        },
        GlobalImport {
            module: "spectest",
            name: "global_f64",
            value: spacewasm::Box::new(StaticGlobal {
                value: Mutex::new(Value::F64(666.6)),
                ty: ValType::F64,
            })
                .unwrap()
                .into_global_value_dyn(),
        },
    ];

    let functions = vec![
        HostFunction::new("spectest", "print", &[], &[], |_, _| {
            ControlFlow::Continue(None)
        }),
        HostFunction::new("spectest", "print_i32", &[ValType::I32], &[], |_, _| {
            ControlFlow::Continue(None)
        }),
        HostFunction::new("spectest", "print_i64", &[ValType::I64], &[], |_, _| {
            ControlFlow::Continue(None)
        }),
        HostFunction::new("spectest", "print_f32", &[ValType::F32], &[], |_, _| {
            ControlFlow::Continue(None)
        }),
        HostFunction::new("spectest", "print_f64", &[ValType::F64], &[], |_, _| {
            ControlFlow::Continue(None)
        }),
        HostFunction::new(
            "spectest",
            "print_i32_f32",
            &[ValType::I32, ValType::F32],
            &[],
            |_, _| ControlFlow::Continue(None),
        ),
        HostFunction::new(
            "spectest",
            "print_f64_f64",
            &[ValType::F64, ValType::F64],
            &[],
            |_, _| ControlFlow::Continue(None),
        ),
    ];

    // Note: Not providing memory import - let each test define its own memory
    let memories = vec![];

    let globals_static: &'static [GlobalImport] = Box::leak(globals.into_boxed_slice());
    let functions_static: &'static [HostFunction] = Box::leak(functions.into_boxed_slice());
    let memories_static: &'static [MemoryImport] = Box::leak(memories.into_boxed_slice());

    ModuleImports {
        globals: globals_static,
        functions: functions_static,
        memories: memories_static,
    }
}

fn run_wast_test_file_internal(file_name: &str) -> Result<TestResult, String> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let wast_path = format!("{}/tests/spectest/{}.wast", manifest_dir, file_name);

    let wast_content = std::fs::read_to_string(&wast_path)
        .map_err(|e| format!("failed to read wast file: {}", e))?;

    let buf = ParseBuffer::new(&wast_content)
        .map_err(|e| format!("failed to create parse buffer: {}", e))?;

    let wast: Wast = parser::parse(&buf).map_err(|e| format!("failed to parse wast: {}", e))?;

    let imports = create_spectest_imports();

    let mut context = TestContext::new(imports);
    let mut stats = TestResult::default();

    for (idx, directive) in wast.directives.into_iter().enumerate() {
        match directive {
            WastDirective::Module(mut quote_wat) => {
                // Extract module name before encoding
                let module_name = match &quote_wat {
                    QuoteWat::Wat(Wat::Module(module)) => module.id.map(|id| id.name().to_string()),
                    _ => {
                        stats.skipped += 1;
                        continue;
                    }
                };

                // Encode the wast module to binary
                let wasm_bytes = quote_wat
                    .encode()
                    .map_err(|e| format!("Command {}: failed to encode module: {}", idx, e))?;

                match context.load_module_from_bytes(module_name, &wasm_bytes) {
                    Ok(_) => stats.passed += 1,
                    Err(e) => {
                        stats.failed += 1;
                        stats.failures.push(format!("Command {}: {}", idx, e));
                    }
                }
            }
            WastDirective::AssertReturn { exec, results, .. } => {
                let WastExecute::Invoke(WastInvoke {
                                            module, name, args, ..
                                        }) = &exec
                else {
                    stats.skipped += 1;
                    continue;
                };

                let arg_values: Vec<Value> = args.iter().map(wast_arg_to_value).collect();

                match context.invoke(module.as_ref().map(|m| m.name()), name, &arg_values) {
                    Ok(return_values) => {
                        if return_values.len() != results.len() {
                            stats.failed += 1;
                            stats.failures.push(format!(
                                "Command {}: expected {} return values, got {}",
                                idx,
                                results.len(),
                                return_values.len()
                            ));
                        } else {
                            let mut matched = true;
                            for (i, (actual, expected)) in
                                return_values.iter().zip(results.iter()).enumerate()
                            {
                                if !values_match(actual, expected) {
                                    matched = false;
                                    stats.failed += 1;
                                    stats.failures.push(format!(
                                        "Command {}: return value {} mismatch: expected {:?}, got {:?}",
                                        idx, i, expected, actual
                                    ));
                                    break;
                                }
                            }
                            if matched {
                                stats.passed += 1;
                            }
                        }
                    }
                    Err(e) => {
                        stats.failed += 1;
                        stats.failures.push(format!("Command {}: {}", idx, e));
                    }
                }
            }
            WastDirective::AssertTrap { exec, message, .. } => {
                let WastExecute::Invoke(WastInvoke {
                                            module, name, args, ..
                                        }) = &exec
                else {
                    stats.skipped += 1;
                    continue;
                };

                let arg_values: Vec<Value> = args.iter().map(wast_arg_to_value).collect();

                match context.invoke(module.as_ref().map(|m| m.name()), name, &arg_values) {
                    Ok(_) => {
                        stats.failed += 1;
                        stats
                            .failures
                            .push(format!("Command {}: expected trap '{}'", idx, message));
                    }
                    Err(e) if e.contains("trap") => {
                        stats.passed += 1;
                    }
                    Err(e) => {
                        stats.failed += 1;
                        stats
                            .failures
                            .push(format!("Command {}: unexpected error: {}", idx, e));
                    }
                }
            }
            _ => {
                stats.skipped += 1;
            }
        }
    }

    Ok(stats)
}

pub fn run_wast_test_file(file_name: &str) {
    match run_wast_test_file_internal(file_name) {
        Ok(result) => {
            if result.failed > 0 {
                eprintln!("\nTest failures for {}:", file_name);
                for failure in &result.failures {
                    eprintln!("  {}", failure);
                }
                panic!(
                    "{} tests passed, {} failed, {} skipped",
                    result.passed, result.failed, result.skipped
                );
            } else {
                println!(
                    "{}: {} passed, {} skipped",
                    file_name, result.passed, result.skipped
                );
            }
        }
        Err(e) => panic!("Failed to run test: {}", e),
    }
}
