use spacewasm::{
    AllocError, Allocator, InnerVec, MemoryStatistics, PageAllocator, ReaderError, SectionKind,
    Stream,
};
use std::alloc::Layout;
use std::collections::HashMap;
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
    pen: HashMap<*mut u8, Vec<u8>>,
}

impl FileStream {
    fn new(file: std::fs::File) -> FileStream {
        FileStream {
            file,
            pen: Default::default(),
        }
    }
}

impl Stream for FileStream {
    fn read(&mut self) -> Result<Option<InnerVec<u8>>, ReaderError> {
        let mut v = Vec::with_capacity(4096);
        let n = self.file.read(&mut v).map_err(|err| {
            eprintln!("Failed to read file: {}", err);
            ReaderError
        })?;

        if n == 0 {
            Ok(None)
        } else {
            let m = InnerVec {
                ptr: v.as_mut_ptr(),
                capacity: 4096,
                len: n as u32,
            };

            self.pen.insert(m.ptr, v);
            Ok(Some(m))
        }
    }

    fn return_(&mut self, chunk: InnerVec<u8>) {
        self.pen.remove(&chunk.ptr);
    }
}

fn main() {
    let allocator: PageAllocator<2> = PageAllocator::new(&RustSystemAllocator {}, 4096);
    spacewasm::alloc::run(&allocator, || {
        std::env::args().skip(1).for_each(|path| {
            let file = std::fs::File::open(path).expect("failed to open file");
            match spacewasm::Module::new(&mut FileStream::new(file)) {
                Ok(module) => {
                    eprintln!(
                        "{:#?}",
                        module
                            .memory_usage
                            .iter()
                            .enumerate()
                            .map(|(i, v)| { (SectionKind::convert(i as u8).unwrap(), v) })
                            .collect::<Vec<_>>()
                    );
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
