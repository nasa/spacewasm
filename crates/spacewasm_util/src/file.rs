use spacewasm::{InnerVec, WasmStream};
use std::collections::{HashMap, VecDeque};
use std::io::Read;

/// Number of reusable read buffers held by a [`FileStream`]'s pool.
const BUFFER_POOL_SIZE: usize = 8;

/// Size, in bytes, of each buffer in a [`FileStream`]'s pool.
const BUFFER_SIZE: usize = 1024;

/// Error code reported by [`FileStream::read`] when the underlying I/O error has
/// no OS errno that can be represented in the stream's single-`u8` error
/// channel.
const UNKNOWN_IO_ERROR: u8 = 0xFF;

/// Map an OS errno (as returned by [`std::io::Error::raw_os_error`]) into the
/// stream's single-`u8` error channel, applying the [`UNKNOWN_IO_ERROR`]
/// encoding documented on that constant. Only errnos in `1..=254` are passed
/// through verbatim; everything else (missing errno, `0`, or a value that would
/// truncate or collide with the sentinel) is reported as [`UNKNOWN_IO_ERROR`].
fn errno_to_code(errno: Option<i32>) -> u8 {
    match errno {
        Some(e) if (1..UNKNOWN_IO_ERROR as i32).contains(&e) => e as u8,
        _ => UNKNOWN_IO_ERROR,
    }
}

pub struct FileStream {
    file: std::fs::File,
    ready: VecDeque<Vec<u8>>,
    used: HashMap<*mut u8, Vec<u8>>,
    n: usize,
}

impl FileStream {
    pub fn new(file: std::fs::File) -> FileStream {
        let mut ready = VecDeque::new();
        for _ in 0..BUFFER_POOL_SIZE {
            ready.push_back(vec![0u8; BUFFER_SIZE]);
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

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }
}

impl WasmStream for FileStream {
    fn read(&mut self) -> Result<Option<InnerVec<u8>>, u8> {
        let mut buf = self.ready.pop_front().unwrap_or_else(|| {
            panic!(
                "FileStream buffer pool exhausted: all {BUFFER_POOL_SIZE} buffers \
                 are checked out; return chunks with `return_` before reading again"
            )
        });

        let n = self.file.read(&mut buf).map_err(|err| {
            eprintln!("Failed to read file: {}", err);
            errno_to_code(err.raw_os_error())
        })?;

        if n == 0 {
            Ok(None)
        } else {
            let m = unsafe {
                InnerVec::from_raw_parts(buf.as_mut_ptr(), buf.capacity() as u32, n as u32)
            };

            self.n += n;
            self.used.insert(buf.as_mut_ptr(), buf);
            Ok(Some(m))
        }
    }

    fn return_(&mut self, chunk: InnerVec<u8>) {
        let buf = self.used.remove(&chunk.ptr()).unwrap();
        self.ready.push_back(buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errno_in_range_is_passed_through() {
        assert_eq!(errno_to_code(Some(1)), 1);
        assert_eq!(errno_to_code(Some(2)), 2);
        assert_eq!(errno_to_code(Some(13)), 13);
        assert_eq!(errno_to_code(Some(254)), 254);
    }

    #[test]
    fn missing_errno_maps_to_sentinel() {
        assert_eq!(errno_to_code(None), UNKNOWN_IO_ERROR);
    }

    #[test]
    fn zero_errno_maps_to_sentinel() {
        // `0` is reserved to mean "no error"; it must never be emitted as an
        // error code.
        assert_eq!(errno_to_code(Some(0)), UNKNOWN_IO_ERROR);
    }

    #[test]
    fn out_of_range_errno_maps_to_sentinel_without_colliding() {
        // Values that would truncate into a colliding byte are remapped to the
        // sentinel rather than silently wrapping.
        assert_eq!(errno_to_code(Some(255)), UNKNOWN_IO_ERROR); // equals sentinel
        assert_eq!(errno_to_code(Some(256)), UNKNOWN_IO_ERROR); // would truncate to 0
        assert_eq!(errno_to_code(Some(511)), UNKNOWN_IO_ERROR); // would truncate to 255
        assert_eq!(errno_to_code(Some(-1)), UNKNOWN_IO_ERROR);
    }

    #[test]
    fn sentinel_is_nonzero() {
        // A nonzero sentinel keeps `0` free for "no error" and guarantees the
        // passthrough range never produces it.
        assert_ne!(UNKNOWN_IO_ERROR, 0);
    }
}
