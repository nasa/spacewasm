/// An implementation of a WasmStream for directly embedding WASM modules
/// into code using things like include_bytes!().
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
use spacewasm::{InnerVec, Vec, WasmStream};
pub struct ByteStream {
    index: usize,
    chunks: Vec<Vec<u8>>,
}

impl ByteStream {
    pub fn new(bytes: &[u8]) -> ByteStream {
        // Avoid temporary allocations here: the PageAllocator poisons the heap
        // on any deallocation, so nothing allocated here may be dropped.
        let mut chunks: Vec<Vec<u8>> =
            Vec::new(bytes.len().div_ceil(1024)).expect("could not allocate vector");

        for chunk in bytes.chunks(1024) {
            chunks.push(Vec::from_exact_iter(chunk.iter().copied()).unwrap());
        }

        ByteStream { index: 0, chunks }
    }
}

impl WasmStream for ByteStream {
    fn read(&mut self) -> Result<Option<InnerVec<u8>>, u8> {
        if self.index == self.chunks.len() {
            Ok(None)
        } else {
            let m = unsafe {
                InnerVec::from_raw_parts(
                    self.chunks[self.index].as_mut_ptr(),
                    1024,
                    self.chunks[self.index].len(),
                )
            };

            self.index += 1;

            Ok(Some(m))
        }
    }

    fn return_(&mut self, _: InnerVec<u8>) {
        // intentionally left empty
    }
}
