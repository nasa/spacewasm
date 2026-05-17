use core::ops::Deref;

use crate::util::InnerVec;

// FIXME(tumbar) Do we need context driven errors here or is there another
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReaderError;

pub struct Chunk(InnerVec<u8>);

impl From<InnerVec<u8>> for Chunk {
    fn from(value: InnerVec<u8>) -> Self {
        Chunk(value)
    }
}

impl Chunk {
    pub(crate) fn return_(&mut self, stream: &mut dyn WasmStream) {
        let to_drop = core::mem::replace(&mut self.0, InnerVec::zero());
        stream.return_(to_drop)
    }
}

impl Drop for Chunk {
    fn drop(&mut self) {
        assert_eq!(self.0.capacity, 0);
    }
}

impl Deref for Chunk {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.0
    }
}

pub trait WasmStream {
    /// Read the next chunk of data from the data 'feeder'
    /// Returns Ok(None) when we are finished reading
    fn read(&mut self) -> Result<Option<InnerVec<u8>>, ReaderError>;

    /// Returns a buffer back to the stream so that the memory may be reused.
    fn return_(&mut self, chunk: InnerVec<u8>);
}
