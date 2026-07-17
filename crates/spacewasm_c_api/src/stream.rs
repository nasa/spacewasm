//! Streaming Wasm input bridged from a C pull callback into a
//! [`spacewasm::WasmStream`], so the whole module never has to be resident in
//! memory.

use core::ffi::c_void;

use spacewasm::{InnerVec, Vec, WasmStream};

use crate::status::{self, spacewasm_status_t};

/// Outcome of a [`spacewasm_read_fn_t`] call, written by the callback.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum spacewasm_read_result_t {
    /// A chunk of `*out_len` bytes was written to the buffer. `out_len == 0`
    /// also signals end-of-stream.
    SPACEWASM_READ_OK = 0,
    /// End of stream; no more bytes.
    SPACEWASM_READ_EOF = 1,
    /// An I/O error occurred; loading fails with `SPACEWASM_ERR_STREAM`.
    SPACEWASM_READ_ERROR = 2,
}

/// C callback that supplies the next chunk of a Wasm module, writing up to
/// `cap` bytes into `buf` and setting `*out_len` (0 == EOF).
pub type spacewasm_read_fn_t = Option<
    unsafe extern "C" fn(
        userdata: *mut c_void,
        buf: *mut u8,
        cap: usize,
        out_len: *mut usize,
    ) -> spacewasm_read_result_t,
>;

/// A [`WasmStream`] that pulls chunks from a C callback into a single owned
/// scratch buffer. The reader holds at most one outstanding chunk and returns
/// it (via `return_`) before requesting the next, so one buffer suffices.
pub struct CallbackStream {
    read: unsafe extern "C" fn(*mut c_void, *mut u8, usize, *mut usize) -> spacewasm_read_result_t,
    userdata: *mut c_void,
    buffer: Vec<u8>,
    errored: bool,
}

impl CallbackStream {
    /// Create a stream backed by a C callback, using an owned scratch buffer of
    /// `chunk_size` bytes (a minimum is enforced).
    pub fn new(
        read: spacewasm_read_fn_t,
        userdata: *mut c_void,
        chunk_size: usize,
    ) -> Result<CallbackStream, spacewasm_status_t> {
        let read = read.ok_or(status::SPACEWASM_ERR_NULL_ARG)?;
        let cap = if chunk_size == 0 { 4096 } else { chunk_size };
        let mut buffer = Vec::<u8>::new(cap as u32).map_err(status::alloc_status)?;
        // Fill so the backing allocation is `cap` bytes and `len()` reports it.
        for _ in 0..cap {
            buffer.push(0);
        }
        Ok(CallbackStream {
            read,
            userdata,
            buffer,
            errored: false,
        })
    }

    /// Whether the callback reported an I/O error during reading.
    pub fn errored(&self) -> bool {
        self.errored
    }
}

impl WasmStream for CallbackStream {
    fn read(&mut self) -> Result<Option<InnerVec<u8>>, u8> {
        let cap = self.buffer.len();
        let mut out_len: usize = 0;
        // SAFETY: `read` is a valid C function pointer; `buffer` is valid for
        // `cap` bytes; `out_len` is a valid local.
        let result =
            unsafe { (self.read)(self.userdata, self.buffer.as_mut_ptr(), cap, &mut out_len) };

        match result {
            spacewasm_read_result_t::SPACEWASM_READ_ERROR => {
                self.errored = true;
                // Non-zero code signals a reader error to the interpreter.
                Err(1)
            }
            spacewasm_read_result_t::SPACEWASM_READ_EOF => Ok(None),
            spacewasm_read_result_t::SPACEWASM_READ_OK => {
                if out_len == 0 {
                    return Ok(None);
                }
                let n = out_len.min(cap);
                // Hand the interpreter a borrowed view of our scratch buffer.
                // `capacity: 0` ensures the `Chunk` drop assertion holds and no
                // deallocation of borrowed memory is attempted.
                Ok(Some(InnerVec {
                    ptr: self.buffer.as_mut_ptr(),
                    capacity: 0,
                    len: n as u32,
                }))
            }
        }
    }

    fn return_(&mut self, _chunk: InnerVec<u8>) {
        // No-op: the chunk borrows our owned scratch buffer, which we reuse for
        // the next `read`.
    }
}
