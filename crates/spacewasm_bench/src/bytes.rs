use spacewasm::{InnerVec, Vec, WasmStream};
pub struct ByteStream {
    data: Vec<u8>
}

impl ByteStream {
    pub fn new(bytes: &[u8]) -> ByteStream {
        let vector: Vec<u8> = Vec::from_exact_iter(bytes.iter().copied());

        ByteStream {
            data: vector
        }
    }
}

impl WasmStream for ByteStream {
    fn read(&mut self) -> Result<Option<InnerVec<u8>>, u8> {
        let m = InnerVec {
                ptr: self.data.as_mut_ptr(),
                capacity: self.data.capacity() as u32,
                len: self.data.len() as u32,
            };

        Ok(Some(m))
    }

    fn return_(&mut self, _: InnerVec<u8>) {
        // pass
    }
}
