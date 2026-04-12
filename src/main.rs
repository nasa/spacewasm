use spacewasm::alloc::{AllocError, Allocator};
use spacewasm::PageAllocator;
use std::alloc::Layout;

struct RustSystemAllocator;
unsafe impl Allocator for RustSystemAllocator {
    unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
        unsafe { Ok(std::alloc::alloc(layout)) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { std::alloc::dealloc(ptr, layout) }
    }
}

fn main() {
    let allocator: PageAllocator<2> = PageAllocator::new(&RustSystemAllocator {}, 4096);
    spacewasm::alloc::run(&allocator, || {
        std::env::args().skip(1).for_each(|path| {
            let data = std::fs::read(&path).expect("failed to read file");

            match spacewasm::Module::new(&data) {
                Ok(module) => {
                    eprintln!("{:#?}", allocator.stats());
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
