use spacewasm::{InnerVec, ReaderError, WasmStream};
use std::collections::{HashMap, VecDeque};
use std::io::Read;

pub struct FileStream {
    file: std::fs::File,
    ready: VecDeque<Vec<u8>>,
    used: HashMap<*mut u8, Vec<u8>>,
    n: usize,
}

impl FileStream {
    pub fn new(file: std::fs::File) -> FileStream {
        let mut ready = VecDeque::new();
        for _ in 0..8 {
            ready.push_back(vec![0u8; 1024]);
        }

        FileStream {
            file,
            ready,
            used: Default::default(),
            n: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.n
    }
}

impl WasmStream for FileStream {
    fn read(&mut self) -> Result<Option<InnerVec<u8>>, ReaderError> {
        let mut buf = self.ready.pop_front().expect("no more buffers");

        let n = self.file.read(&mut buf).map_err(|err| {
            eprintln!("Failed to read file: {}", err);
            ReaderError
        })?;

        if n == 0 {
            Ok(None)
        } else {
            let m = InnerVec {
                ptr: buf.as_mut_ptr(),
                capacity: 4096,
                len: n as u32,
            };

            self.n += n;
            self.used.insert(buf.as_mut_ptr(), buf);
            Ok(Some(m))
        }
    }

    fn return_(&mut self, chunk: InnerVec<u8>) {
        let buf = self.used.remove(&chunk.ptr).unwrap();
        self.ready.push_back(buf);
    }
}
