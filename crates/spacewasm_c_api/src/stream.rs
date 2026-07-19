//! Streaming Wasm input bridged from a C pull callback into a
//! [`spacewasm::WasmStream`], so the whole module never has to be resident in
//! memory.

use core::ffi::c_void;

use spacewasm::{InnerVec, WasmStream};

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

/// C callback that supplies the next chunk of a Wasm module. The callback owns
/// the buffer: it sets `*out_buf` to point at the next chunk and `*out_len` to
/// its length (0 == EOF). The chunk must stay valid until the next call to the
/// callback (or until loading completes).
pub type spacewasm_read_fn_t = Option<
    unsafe extern "C" fn(
        userdata: *mut c_void,
        out_buf: *mut *const u8,
        out_len: *mut usize,
    ) -> spacewasm_read_result_t,
>;

/// A [`WasmStream`] that pulls chunks from a C callback. The callback owns the
/// buffer backing each chunk and hands us a borrowed pointer, so the stream
/// allocates nothing of its own — important under a strict-LIFO page allocator,
/// where a scratch buffer freed mid-load would strand its page.
pub struct CallbackStream {
    read: unsafe extern "C" fn(*mut c_void, *mut *const u8, *mut usize) -> spacewasm_read_result_t,
    userdata: *mut c_void,
    errored: bool,
}

impl CallbackStream {
    /// Create a stream backed by a C callback.
    pub fn new(
        read: spacewasm_read_fn_t,
        userdata: *mut c_void,
    ) -> Result<CallbackStream, spacewasm_status_t> {
        let read = read.ok_or(status::SPACEWASM_ERR_NULL_ARG)?;
        Ok(CallbackStream {
            read,
            userdata,
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
        let mut out_buf: *const u8 = core::ptr::null();
        let mut out_len: usize = 0;
        // SAFETY: `read` is a valid C function pointer; `out_buf`/`out_len` are
        // valid locals the callback writes.
        let result = unsafe { (self.read)(self.userdata, &mut out_buf, &mut out_len) };

        match result {
            spacewasm_read_result_t::SPACEWASM_READ_ERROR => {
                self.errored = true;
                // Non-zero code signals a reader error to the interpreter.
                Err(1)
            }
            spacewasm_read_result_t::SPACEWASM_READ_EOF => Ok(None),
            spacewasm_read_result_t::SPACEWASM_READ_OK => {
                if out_len == 0 || out_buf.is_null() {
                    return Ok(None);
                }
                // Hand the interpreter a borrowed view of the callback's buffer.
                // `capacity: 0` ensures the `Chunk` drop assertion holds and no
                // deallocation of borrowed memory is attempted.
                Ok(Some(InnerVec {
                    ptr: out_buf as *mut u8,
                    capacity: 0,
                    len: out_len as u32,
                }))
            }
        }
    }

    fn return_(&mut self, _chunk: InnerVec<u8>) {
        // No-op: the chunk borrows the callback's buffer; nothing to free.
    }
}
