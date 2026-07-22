use spacewasm::{InnerVec, Vec, WasmStream};
pub struct ByteStream {
    index: usize,
    chunks: Vec<Vec<u8>>,
}

impl ByteStream {
    pub fn new(bytes: &[u8]) -> ByteStream {
        let bytes_vec: Vec<u8> = Vec::from_exact_iter(bytes.iter().copied());
        let mut chunks: Vec<Vec<u8>> = Vec::new(1).expect("could not allocate vector");

        let mut i = 0;
        while i < bytes_vec.len() {
            let n = core::cmp::min(1024, bytes_vec.len() - i);

            chunks.push(Vec::from_exact_iter(bytes_vec[i..(i + n)].iter().copied()));

            i += n;
        }

        ByteStream { index: 0, chunks: chunks }
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

            // let m = self.ready.pop().expect("expected value");
            // println!("read({}) --> {:?}", m.len(), m);

            Ok(Some(m))
        }
    }

    fn return_(&mut self, _: InnerVec<u8>) {
        // println!("return({})", to_add.len());

        // self.ready.push(to_add);
    }
}
