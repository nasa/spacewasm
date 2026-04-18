use core::ops::Deref;

use crate::util::InnerVec;

// FIXME(tumbar) Do we need context driven errors here or is there another
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReaderError;

pub struct WasmChunk<'wasm> {
    inner: InnerVec<u8>,
    stream: &'wasm dyn WasmStreamer,
}

impl<'wasm> WasmChunk<'wasm> {
    pub(crate) fn new(inner: InnerVec<u8>, stream: &'wasm dyn WasmStreamer) -> Self {
        Self { inner, stream }
    }
}

impl<'wasm> Drop for WasmChunk<'wasm> {
    fn drop(&mut self) {
        let to_drop = core::mem::replace(&mut self.inner, InnerVec::zero());
        self.stream.return_(to_drop)
    }
}

impl Deref for WasmChunk<'_> {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.inner
    }
}

pub trait WasmStreamer {
    /// Read the next chunk of data from the data 'feeder'
    /// Returns Ok(None) when we are finished reading
    fn read(&self) -> Result<Option<InnerVec<u8>>, ReaderError>;

    /// Returns a buffer back to the stream so that the memory may be reused.
    fn return_(&self, chunk: InnerVec<u8>);
}
