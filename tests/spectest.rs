use spacewasm::{
    global_allocator, vec, AllocError, Allocator, GlobalValue, GlobalValueError, HostFunction,
    HostGlobal, HostModule, InnerVec, MemoryStatistics, ReaderError, Store, Stream, ValType, Value,
};
use std::alloc::Layout;
use std::ops::ControlFlow;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use wast::parser::{self, ParseBuffer};
use wast::{Wast, WastDirective};

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

fn run_wast_test_file_inner(file_name: &str) -> Result<(), String> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let wast_path = format!("{}/tests/spectest/{}.wast", manifest_dir, file_name);

    let wast_content = std::fs::read_to_string(&wast_path)
        .map_err(|e| format!("failed to read wast file: {}", e))?;

    let buf = ParseBuffer::new(&wast_content)
        .map_err(|e| format!("failed to create parse buffer: {}", e))?;

    let wast: Wast = parser::parse(&buf).map_err(|e| format!("failed to parse wast: {}", e))?;

    let mut store = Store::new(
        2,
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

    for dir in wast.directives {
        match dir {
            WastDirective::Module(m) => {}
            WastDirective::ModuleDefinition(d) => {}
            WastDirective::ModuleInstance {
                span,
                instance,
                module,
            } => {}
            WastDirective::AssertMalformed { .. } => {}
            WastDirective::AssertInvalid { .. } => {}
            WastDirective::AssertInvalidCustom { .. } => {}
            WastDirective::Register { .. } => {}
            WastDirective::Invoke(_) => {}
            WastDirective::AssertTrap { .. } => {}
            WastDirective::AssertReturn { .. } => {}
            WastDirective::AssertExhaustion { .. } => {}
            WastDirective::AssertUnlinkable { .. } => {}
            WastDirective::AssertException { .. } => {}
            WastDirective::AssertSuspension { .. } => {}
            WastDirective::Thread(_) => {}
            WastDirective::Wait { .. } => {}
            WastDirective::AssertMalformedCustom { .. } => {}
        }
    }

    Ok(())
}

pub fn run_wast_test_file(file_name: &str) {
    match run_wast_test_file_inner(file_name) {
        Ok(_) => {}
        Err(_) => {}
    }
}
