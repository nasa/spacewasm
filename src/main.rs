use spacewasm::alloc::AllocError;
use std::alloc::Layout;

struct StdAllocator;

unsafe impl spacewasm::alloc::Allocator for StdAllocator {
    unsafe fn allocate(&self, layout: Layout) -> Result<*mut u8, AllocError> {
        eprintln!("Allocating {:?}", layout);

        unsafe { Ok(std::alloc::alloc(layout)) }
    }

    unsafe fn deallocate(&self, ptr: *mut u8, layout: Layout) {
        eprintln!("Deallocating {:?} {:?}", ptr, layout);

        unsafe { std::alloc::dealloc(ptr, layout) }
    }
}

static ALLOCATOR: StdAllocator = StdAllocator;

fn main() {
    unsafe {
        spacewasm::alloc::init(&raw const ALLOCATOR);
    }

    std::env::args().skip(1).for_each(|path| {
        let data = std::fs::read(&path).expect("failed to read file");
        let module = spacewasm::Module::new(&data).expect("failed to parse file");

        for item in module.functions {
            println!("{:?}", item);
        }

        println!("Found {} imports", module.imports.len());
    })
}
