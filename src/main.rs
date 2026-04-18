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
    let allocator: PageAllocator<2> = PageAllocator::new(&RustSystemAllocator {}, 8192);
    spacewasm::alloc::run(&allocator, || {
        std::env::args().skip(1).for_each(|path| {
            let file = std::fs::File::open(path).expect("failed to open file");
            match spacewasm::Module::new(&mut FileStream::new(file)) {
                Ok(module) => {
                    for (i, section) in module.memory_usage.iter().enumerate() {
                        let section_kind = SectionKind::convert(i as u8).unwrap();
                        eprintln!("{:?} {} bytes", section_kind, section.total_bytes);
                    }

                    eprintln!("{:?}", module.functions);

                    println!("Found {} imports", module.imports.len());
                }
                Err(err) => {
                    eprintln!("{:#?}", allocator.stats());
                    eprintln!("Failed to parse: {:?}", err)
                }
            }
        });
    });
}
