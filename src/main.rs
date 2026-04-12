use spacewasm::PageAllocator;
use std::alloc::Layout;

struct GlobalAllocator;
unsafe impl core::alloc::GlobalAlloc for GlobalAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { std::alloc::alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { std::alloc::dealloc(ptr, layout) }
    }
}

fn main() {
    let allocator: PageAllocator<4096, 2> = PageAllocator::new(&GlobalAllocator {});
    spacewasm::alloc::run(&allocator, || {
        std::env::args().skip(1).for_each(|path| {
            let data = std::fs::read(&path).expect("failed to read file");

            match spacewasm::Module::new(&data) {
                Ok(module) => {
                    for item in module.functions {
                        println!("{:?}", item);
                    }

                    println!("Found {} imports", module.imports.len());
                }
                Err(err) => {
                    eprintln!("Failed to parse: {:?}", err)
                }
            }
        });
    });

    let stats = allocator.finish();
    println!("{:#?}", stats);
}
