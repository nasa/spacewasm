/// An implementation of a WasmMemoryAllocator using the embedded_alloc crate
/// for using SpaceWasm in embedded applications.
///
/// Copyright 2026 California Institute of Technology
///
/// Licensed under the Apache License, Version 2.0 (the "License");
/// you may not use this file except in compliance with the License.
/// You may obtain a copy of the License at
///
/// <http://www.apache.org/licenses/LICENSE-2.0>
///
/// ---
/// Portions of this file are derived from <https://github.com/rust-embedded/embedded-alloc>
/// and the embedded-alloc crate developed by the Rust Embedded community.
use core::{
    alloc::{GlobalAlloc, Layout},
    ptr::NonNull,
};

use embedded_alloc::LlffHeap as Heap;
use spacewasm::{AllocError, Allocator, WasmMemoryAllocator};

#[global_allocator]
static HEAP: Heap = Heap::empty();

const HEAP_SIZE: usize = 100000;

/// Backing memory for the heap. This is placed in `.uninit` (a `NOLOAD`
/// section provided by the riscv-rt and cortex-m-rt linker scripts) rather
/// than `.bss` so that the fixed-size memory handed to the wasm module is not
/// counted against the size of SpaceWasm itself in the benchmark report.
#[unsafe(link_section = ".uninit.HEAP_MEM")]
static mut HEAP_MEM: [core::mem::MaybeUninit<u8>; HEAP_SIZE] =
    [core::mem::MaybeUninit::uninit(); HEAP_SIZE];

pub fn init() {
    unsafe {
        HEAP.init(&raw mut HEAP_MEM as usize, HEAP_SIZE);
    }
}

pub struct BareMetalAllocator;
unsafe impl Allocator for BareMetalAllocator {
    unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
        unsafe { Ok(HEAP.alloc(layout)) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { HEAP.dealloc(ptr, layout) }
    }
}

impl WasmMemoryAllocator for BareMetalAllocator {
    fn allocate(&self, layout: Layout) -> Result<NonNull<u8>, AllocError> {
        unsafe { NonNull::new(HEAP.alloc(layout)).ok_or(AllocError::AllocationFailed) }
    }

    fn reallocate(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        layout: Layout,
    ) -> Result<NonNull<u8>, AllocError> {
        unsafe {
            NonNull::new(HEAP.realloc(ptr.as_ptr(), old_layout, layout.size()))
                .ok_or(AllocError::AllocationFailed)
        }
    }

    fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        unsafe { HEAP.dealloc(ptr.as_ptr(), layout) }
    }
}
