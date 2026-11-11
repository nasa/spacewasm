//! The IR map against `wasmparser`'s own reading of the same module.

use spacewasm::{CompilerOptions, PageAllocator};
use spacewasm_tools::{DecoderLimits, host_modules_for, ir_map};

spacewasm::global_allocator!(
    PageAllocator<System, 64>,
    PageAllocator::new(System, 64 * 1024)
);

struct System;

// SAFETY: forwards to the Rust global allocator.
unsafe impl spacewasm::Allocator for System {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> Result<*mut u8, spacewasm::AllocError> {
        // SAFETY: the caller guarantees a non-zero size.
        let ptr = unsafe { std::alloc::alloc(layout) };
        match ptr.is_null() {
            true => Err(spacewasm::AllocError::AllocationFailed),
            false => Ok(ptr),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        // SAFETY: `ptr` came from `alloc` with `layout`.
        unsafe { std::alloc::dealloc(ptr, layout) }
    }
}

/// Every compiled instruction maps to its opcode's offset, even across a code page.
#[test]
fn every_instruction_maps_to_its_opcode_across_pages() {
    let body = "i32.const 300 drop ".repeat(200);
    let bytes = wat::parse_str(format!(
        r#"(module
             (memory 512 (pagesize 1))
             (func (export "main") {body}))"#
    ))
    .unwrap();
    // The opcode offsets `wasmparser` sees, `end` included.
    let mut expected = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(&bytes) {
        if let wasmparser::Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                expected.push(reader.read_with_offset().unwrap().1 as u32);
            }
        }
    }

    let hosts = host_modules_for(&bytes).unwrap();
    let options = CompilerOptions::new(true, None, 16);
    let map = ir_map(&bytes, options, hosts, DecoderLimits::default()).unwrap();

    // `i32.const 300` is 3 IR words (operand > 254), `drop` 1: 4 words per pair, so 800
    // words over 4 pages, and the last `end` emits the return.
    for (pair, offsets) in expected.chunks(2).take(200).enumerate() {
        let ir = pair as u32 * 4;
        assert_eq!(map.wasm_offset(ir), Some(offsets[0]), "pair {pair}");
        assert_eq!(map.wasm_offset(ir + 2), Some(offsets[0]), "an operand word");
        assert_eq!(map.wasm_offset(ir + 3), Some(offsets[1]), "pair {pair}");
    }
    assert_eq!(map.wasm_offset(800), expected.last().copied(), "the return");
}
