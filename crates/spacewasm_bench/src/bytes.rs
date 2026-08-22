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
        let bytes_vec: Vec<u8> = Vec::from_exact_iter(bytes.iter().copied());
        let mut chunks: Vec<Vec<u8>> = Vec::new(10).expect("could not allocate vector");

        let mut i = 0;
        while i < bytes_vec.len() {
            let n = core::cmp::min(1024, bytes_vec.len() - i);

            chunks.push(Vec::from_exact_iter(bytes_vec[i..(i + n)].iter().copied()));

            i += n;
        }

        ByteStream { index: 0, chunks }
    }
}

impl WasmStream for ByteStream {
    fn read(&mut self) -> Result<Option<InnerVec<u8>>, u8> {
        if self.index == self.chunks.len() {
            Ok(None)
        } else {
            let m = InnerVec {
                ptr: self.chunks[self.index].as_mut_ptr(),
                capacity: 1024,
                len: self.chunks[self.index].len() as u32,
            };

            self.index += 1;

            Ok(Some(m))
        }
    }

    fn return_(&mut self, _: InnerVec<u8>) {
        // intentionally left empty
    }
}
