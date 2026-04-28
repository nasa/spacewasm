use spacewasm::{
    global_allocator, ExportDesc, FunctionImport, Memory, ModuleImports, ValType, Value,
};
use spacewasm::{
    AllocError, Allocator, InnerVec, MemoryStatistics, PageAllocator, ReaderError, SectionKind,
    Stream,
};
use std::alloc::Layout;
use std::collections::{HashMap, VecDeque};
use std::io::Read;

struct RustSystemAllocator;
unsafe impl Allocator for RustSystemAllocator {
    unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
        unsafe { Ok(std::alloc::alloc(layout)) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { std::alloc::dealloc(ptr, layout) }
    }

    fn memory_statistics(&self) -> MemoryStatistics {
        panic!("The page allocator should be tracking it's own memory statistics.")
    }
}

global_allocator!(
    PageAllocator<16>,
    PageAllocator::new(&RustSystemAllocator {}, 8192)
);

struct FileStream {
    file: std::fs::File,
    ready: VecDeque<Vec<u8>>,
    used: HashMap<*mut u8, Vec<u8>>,
}

impl FileStream {
    fn new(file: std::fs::File) -> FileStream {
        let mut ready = VecDeque::new();
        for _ in 0..8 {
            ready.push_back(vec![0u8; 1024]);
        }

        FileStream {
            file,
            ready,
            used: Default::default(),
        }
    }
}

impl Stream for FileStream {
    fn read(&mut self) -> Result<Option<InnerVec<u8>>, ReaderError> {
        let mut buf = self.ready.pop_front().expect("no more buffers");

        let n = self.file.read(&mut buf).map_err(|err| {
            eprintln!("Failed to read file: {}", err);
            ReaderError
        })?;

        if n == 0 {
            Ok(None)
        } else {
            let m = InnerVec {
                ptr: buf.as_mut_ptr(),
                capacity: 4096,
                len: n as u32,
            };

            self.used.insert(buf.as_mut_ptr(), buf);
            Ok(Some(m))
        }
    }

    fn return_(&mut self, chunk: InnerVec<u8>) {
        let buf = self.used.remove(&chunk.ptr).unwrap();
        self.ready.push_back(buf);
    }
}

fn main() {
    std::env::args().skip(1).for_each(|path| {
        let file = std::fs::File::open(path).expect("failed to open file");
        match spacewasm::Module::new::<256>(
            &mut FileStream::new(file),
            ModuleImports {
                globals: &[],
                functions: &[
                    FunctionImport {
                        module: "fprime_core",
                        name: "panic",
                        params: &[ValType::I32, ValType::I32],
                        returns: &[],
                        f: |a| {
                            eprintln!("PANIC {:?} {:?}", a.get(0), a.get(1));
                            None
                        },
                    },
                    FunctionImport {
                        module: "fprime_core",
                        name: "rsleep",
                        params: &[ValType::I64],
                        returns: &[],
                        f: |a| {
                            eprintln!("RSLEEP {:?}", a.get(0));
                            None
                        },
                    },
                    FunctionImport {
                        module: "fprime_core",
                        name: "command",
                        params: &[ValType::I32, ValType::I32],
                        returns: &[ValType::I32],
                        f: |a| {
                            eprintln!("COMMAND {:?} {:?}", a.get(0), a.get(1));
                            Some(spacewasm::Value::I32(0))
                        },
                    },
                    FunctionImport {
                        module: "fprime_core",
                        name: "telemetry",
                        params: &[
                            ValType::I32,
                            ValType::I32,
                            ValType::I32,
                            ValType::I32,
                            ValType::I32,
                        ],
                        returns: &[ValType::I32],
                        f: |a| {
                            eprintln!(
                                "TELEMETRY {:?} {:?} {:?} {:?} {:?}",
                                a.get(0),
                                a.get(1),
                                a.get(2),
                                a.get(3),
                                a.get(4),
                            );
                            Some(spacewasm::Value::I32(0))
                        },
                    },
                ],
                memories: &[],
            },
        ) {
            Ok(module) => {
                let mut total: usize = 0;
                for (i, section) in module.memory_usage.iter().enumerate() {
                    let section_kind = SectionKind::convert(i as u8).unwrap();
                    eprintln!("{:?}: {} bytes", section_kind, section.total_bytes);
                    total += section.total_bytes as usize;
                }

                eprintln!("Total: {}", total);
                eprintln!(
                    "Compilation Ratio: {:.2}x",
                    (total as f64) / (module.wasm_size as f64)
                );

                let full_page_usage = if module.text.len() > 1 {
                    (module.text.len() - 1) * 256
                } else {
                    0
                };

                eprintln!("Code pages: {}", module.text.len());
                eprintln!(
                    "Code word usage (16-bits): {} / {} ({:.2}%)",
                    full_page_usage + module.final_page_offset,
                    module.text.len() * 256,
                    100.0 * ((full_page_usage + module.final_page_offset) as f64)
                        / (module.text.len() * 256) as f64
                );
                eprintln!(
                    "Final page: {} / 256 ({:.2}%)",
                    module.final_page_offset,
                    100.0 * (module.final_page_offset as f64 / 256.0)
                );

                let mut state = spacewasm::InterpreterState::new(
                    1024,
                    Memory::from(
                        unsafe { std::alloc::alloc(Layout::from_size_align(65536, 64).unwrap()) },
                        65536,
                    ),
                );

                match module.start {
                    None => {
                        let f = module.exports.iter().find(|f| &f.name == "main").unwrap();
                        match f.desc {
                            ExportDesc::Func(fi) => {
                                let f = module.functions.get(fi.0 as usize).unwrap();
                                eprintln!(
                                    "fn main => {:?}",
                                    module.types.get(f.ty.0 as usize).unwrap()
                                );
                                state.invoke(f, &[Value::I32(0)]);
                            }
                            ExportDesc::Table(_) => {}
                            ExportDesc::Mem(_) => {}
                            ExportDesc::Global(_) => {}
                        }
                    }
                    Some(fi) => {
                        let f = module.functions.get(fi.0 as usize).unwrap();
                        state.invoke(f, &[Value::I32(0)]);
                    }
                }

                let interpreter =
                    spacewasm::Interpreter::new(module.functions, module.module_imports);
                let code = spacewasm::Code::new(module.text);

                let r = interpreter.run(&code, &mut state, 10);

                eprintln!("Interpreter finished {:?}", r)
            }
            Err(err) => {
                eprintln!("Failed to parse: {:?}", err)
            }
        }
    });
}
