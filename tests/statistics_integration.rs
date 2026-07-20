//! Integration test for [`spacewasm::Module::new_with_statistics`].
//!
//! `new_with_statistics` decodes a module while sampling the global allocator's
//! live-byte total before and after each section, attributing the delta to that
//! section. This test installs a tracking global allocator, decodes a
//! hard-coded module, and asserts that the reported per-section usage is
//! self-consistent and lands in the sections we expect to allocate.

use core::alloc::Layout;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicI32, Ordering};

use spacewasm::{
    AllocError, Allocator, CodeBuilder, CompilerOptions, Engine, InnerVec, MemoryStatistics,
    Module, SectionKind, WasmMemoryAllocator, WasmStream, global_allocator, vec,
};

extern crate std;

// ---------------------------------------------------------------------------
// Tracking global allocator
//
// The statistics machinery reads `GlobalAllocator::memory_statistics()`, which
// dispatches to the `__spacewasm_*` symbols defined by `global_allocator!`.
// We back it with the system heap and keep a running tally of live bytes so
// the reported deltas are non-zero and meaningful.
// ---------------------------------------------------------------------------

static LIVE_BYTES: AtomicI32 = AtomicI32::new(0);

struct TrackingAllocator;

unsafe impl Allocator for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
        let ptr = unsafe { std::alloc::alloc(layout) };
        if ptr.is_null() {
            Err(AllocError::AllocationFailed)
        } else {
            LIVE_BYTES.fetch_add(layout.size() as i32, Ordering::SeqCst);
            Ok(ptr)
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE_BYTES.fetch_sub(layout.size() as i32, Ordering::SeqCst);
        unsafe { std::alloc::dealloc(ptr, layout) }
    }

    fn memory_statistics(&self) -> MemoryStatistics {
        MemoryStatistics {
            total_bytes: LIVE_BYTES.load(Ordering::SeqCst),
            pad_bytes: 0,
        }
    }
}

// Also usable as the linear-memory allocator argument. The module under test
// declares no memory, so these are never exercised, but the argument still
// needs a concrete `WasmMemoryAllocator`.
impl WasmMemoryAllocator for TrackingAllocator {
    fn allocate(&self, layout: Layout) -> Result<NonNull<u8>, AllocError> {
        unsafe { NonNull::new(std::alloc::alloc(layout)).ok_or(AllocError::AllocationFailed) }
    }

    fn reallocate(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        layout: Layout,
    ) -> Result<NonNull<u8>, AllocError> {
        unsafe {
            NonNull::new(std::alloc::realloc(ptr.as_ptr(), old_layout, layout.size()))
                .ok_or(AllocError::AllocationFailed)
        }
    }

    fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        unsafe { std::alloc::dealloc(ptr.as_ptr(), layout) }
    }
}

global_allocator!(TrackingAllocator, TrackingAllocator);

// ---------------------------------------------------------------------------
// A single-shot stream over an in-memory byte buffer.
// ---------------------------------------------------------------------------

struct ByteStream {
    buffer: std::vec::Vec<u8>,
    consumed: bool,
}

impl ByteStream {
    fn new(data: &[u8]) -> Self {
        ByteStream {
            buffer: data.to_vec(),
            consumed: false,
        }
    }
}

impl WasmStream for ByteStream {
    fn read(&mut self) -> Result<Option<InnerVec<u8>>, u8> {
        if self.consumed {
            return Ok(None);
        }
        self.consumed = true;
        Ok(Some(InnerVec {
            ptr: self.buffer.as_mut_ptr(),
            capacity: self.buffer.len() as u32,
            len: self.buffer.len() as u32,
        }))
    }

    fn return_(&mut self, _chunk: InnerVec<u8>) {
        // The buffer is owned by `self`; nothing to reclaim.
    }
}

/// A hand-assembled module with two function types, two functions, a code
/// section, and two exports:
///
/// ```wat
/// (module
///   (type (func (param i32 i32) (result i32)))
///   (type (func (result i32)))
///   (func (type 0) (local.get 0) (local.get 1) (i32.add))
///   (func (type 1) (i32.const 42))
///   (export "add" (func 0))
///   (export "answer" (func 1)))
/// ```
#[rustfmt::skip]
static STAT_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00,
    // type section (id 1)
    0x01, 0x0b, 0x02, 0x60, 0x02, 0x7f, 0x7f, 0x01, 0x7f, 0x60, 0x00, 0x01, 0x7f,
    // function section (id 3)
    0x03, 0x03, 0x02, 0x00, 0x01,
    // export section (id 7)
    0x07, 0x10, 0x02, 0x03, 0x61, 0x64, 0x64, 0x00, 0x00, 0x06, 0x61, 0x6e, 0x73,
    0x77, 0x65, 0x72, 0x00, 0x01,
    // code section (id 10)
    0x0a, 0x0e, 0x02, 0x07, 0x00, 0x20, 0x00, 0x20, 0x01, 0x6a, 0x0b, 0x04, 0x00,
    0x41, 0x2a, 0x0b,
];

const MAX_CONTROL_FRAMES: usize = 128;
const MAX_STACK_DEPTH: usize = 256;
const MAX_CODE_PAGES: u32 = 256;

#[test]
fn new_with_statistics_reports_per_section_usage() {
    let mut engine = Engine::new(1024, 8, vec![]).unwrap();
    let mut code_builder = CodeBuilder::new(MAX_CODE_PAGES).unwrap();
    let mut stream = ByteStream::new(STAT_WASM);

    let allocator = spacewasm::Rc::new(TrackingAllocator)
        .unwrap()
        .into_wasm_memory_allocator();

    let (module, stats) = Module::new_with_statistics::<MAX_CONTROL_FRAMES, MAX_STACK_DEPTH>(
        "stats",
        &mut stream,
        &mut engine.store,
        &mut code_builder,
        allocator,
        CompilerOptions {
            allow_memory_grow: false,
        },
    )
    .expect("module should decode");

    // The module decoded to the expected shape.
    assert_eq!(module.functions.len(), 2);
    assert_eq!(module.types.len(), 2);
    assert_eq!(module.exports.len(), 2);

    // The statistics array is indexed by `SectionKind`.
    assert_eq!(stats.len(), SectionKind::N as usize);

    // The type section decodes into a `Vec` of function types, so it must
    // register a positive live-byte delta.
    let type_bytes = stats[SectionKind::Type as usize].total_bytes;
    assert!(
        type_bytes > 0,
        "type section should allocate, got {type_bytes}"
    );

    // Likewise the function and export sections build owned collections.
    let function_bytes = stats[SectionKind::Function as usize].total_bytes;
    assert!(
        function_bytes > 0,
        "function section should allocate, got {function_bytes}"
    );
    let export_bytes = stats[SectionKind::Export as usize].total_bytes;
    assert!(
        export_bytes > 0,
        "export section should allocate, got {export_bytes}"
    );

    // Sections absent from the module were never sampled and stay at zero.
    assert_eq!(stats[SectionKind::Import as usize].total_bytes, 0);
    assert_eq!(stats[SectionKind::Memory as usize].total_bytes, 0);
    assert_eq!(stats[SectionKind::Global as usize].total_bytes, 0);
    assert_eq!(stats[SectionKind::Table as usize].total_bytes, 0);
    assert_eq!(stats[SectionKind::Data as usize].total_bytes, 0);
    assert_eq!(stats[SectionKind::Element as usize].total_bytes, 0);
    assert_eq!(stats[SectionKind::Start as usize].total_bytes, 0);
}

#[test]
fn new_with_statistics_matches_plain_new() {
    // Decoding the same bytes with and without statistics must produce an
    // equivalent module; the statistics variant only adds sampling.
    let allocator = || {
        spacewasm::Rc::new(TrackingAllocator)
            .unwrap()
            .into_wasm_memory_allocator()
    };
    let options = || CompilerOptions {
        allow_memory_grow: false,
    };

    let mut engine_a = Engine::new(1024, 8, vec![]).unwrap();
    let mut cb_a = CodeBuilder::new(MAX_CODE_PAGES).unwrap();
    let mut stream_a = ByteStream::new(STAT_WASM);
    let plain = Module::new::<MAX_CONTROL_FRAMES, MAX_STACK_DEPTH>(
        "plain",
        &mut stream_a,
        &mut engine_a.store,
        &mut cb_a,
        allocator(),
        options(),
    )
    .unwrap();

    let mut engine_b = Engine::new(1024, 8, vec![]).unwrap();
    let mut cb_b = CodeBuilder::new(MAX_CODE_PAGES).unwrap();
    let mut stream_b = ByteStream::new(STAT_WASM);
    let (with_stats, _stats) = Module::new_with_statistics::<MAX_CONTROL_FRAMES, MAX_STACK_DEPTH>(
        "with-stats",
        &mut stream_b,
        &mut engine_b.store,
        &mut cb_b,
        allocator(),
        options(),
    )
    .unwrap();

    assert_eq!(plain.functions.len(), with_stats.functions.len());
    assert_eq!(plain.types.len(), with_stats.types.len());
    assert_eq!(plain.exports.len(), with_stats.exports.len());
}
