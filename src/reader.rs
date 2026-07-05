/// WASM Reader
/// This file implements some basic WASM reading capabilities such
/// as LEB128 (variable width integer encoding).
use crate::{
    Allocator, Chunk, CircularBuffer, GlobalAllocator, StaticVec, ValidationError, Vec, WasmStream,
};


/// Wasm encodes integers according to the LEB128 format, which specifies that
/// only 7 bits of every byte are used to store the integer's bits. The 8th bit
/// is always used as a bitflag for whether the next byte shall also be read as
/// part of the current integer. Therefore, it can be called a continuation bit,
/// which is stored here as a global constant to improve code readability.
const CONTINUATION_BIT: u8 = 0b10000000;

const INTEGER_BIT_FLAG: u8 = !CONTINUATION_BIT;

/// A struct for managing and reading WASM bytecode
/// Its purpose is to abstract parsing basic WASM values from the bytecode
/// and managing the chunks from a stream as they are read.
///
/// This reader cannot backtrack. The code that calls into the reader must
/// allocate and copy data that should be retained as it is read.
pub struct Reader<'wasm> {
    stream: &'wasm mut dyn WasmStream,
    /// Number of bytes we've already extracted from the next chunk and
    /// placed in the circular buffer
    chunk_used: usize,
    /// A holding pen for the next chunk given to us by the streamer.
    /// We use this to feed the buffer
    next: Option<Chunk>,

    /// A fixed size circular buffer meant to hold as much WASM data as it can.
    /// WASM chunks may be of variable length, we may need to span multiple which is
    /// Data will be copied into this circular buffer and the processing will be done here.
    buffer: CircularBuffer<u8, 64>,

    /// A counter keeping track of the total number of bytes we've processed in the WASM binary
    /// This is useful for generating error messages with an absolute location in the binary.
    full_offset: usize,
}

impl<'wasm> Reader<'wasm> {
    pub fn new(stream: &'wasm mut dyn WasmStream) -> Self {
        Self {
            stream,
            chunk_used: 0,
            next: None,
            buffer: CircularBuffer::new(),
            full_offset: 0,
        }
    }

    pub fn offset(&self) -> usize {
        self.full_offset
    }

    /// Fills the circular buffer from the stream chunks.
    /// This method tries to fill the buffer as much as possible from the current chunk,
    /// and fetches a new chunk from the stream if the current one is exhausted.
    fn fill_buffer(&mut self) -> Result<(), ValidationError> {
        // If buffer already has data, we're done
        if !self.buffer.is_empty() {
            return Ok(());
        }

        // Try to fill from current chunk if it has remaining bytes
        if let Some(ref chunk) = self.next {
            let remaining = chunk.len() - self.chunk_used;
            if remaining > 0 {
                // Copy bytes from chunk into buffer
                let to_copy = remaining.min(self.buffer.capacity());
                for i in 0..to_copy {
                    self.buffer.push(chunk[self.chunk_used + i]);
                }
                self.chunk_used += to_copy;
                return Ok(());
            }
        }

        // Current chunk is exhausted or None, return it and get next chunk
        if let Some(mut chunk) = self.next.take() {
            chunk.return_(self.stream);
        }

        // Fetch next chunk from stream
        self.next = self
            .stream
            .read()
            .map_err(ValidationError::ReaderError)?
            .map(|inner| inner.into());
        self.chunk_used = 0;

        // Try to fill buffer from new chunk
        if let Some(ref chunk) = self.next {
            if chunk.is_empty() {
                // Empty chunk means EOF
                return Err(ValidationError::Eof);
            }
            let to_copy = chunk.len().min(self.buffer.capacity());
            for i in 0..to_copy {
                self.buffer.push(chunk[i]);
            }
            self.chunk_used = to_copy;
            Ok(())
        } else {
            // No more chunks, EOF
            Err(ValidationError::Eof)
        }
    }

    fn peek_u8(&mut self) -> Result<u8, ValidationError> {
        // Try to get a byte from the buffer
        if let Some(&byte) = self.buffer.front() {
            return Ok(byte);
        }

        // Buffer is empty, need to fill it
        self.fill_buffer()?;

        // Try again
        self.buffer.front().copied().ok_or(ValidationError::Eof)
    }

    /// Tries to read one byte and fails if the end of file is reached.
    pub fn read_u8(&mut self) -> Result<u8, ValidationError> {
        let byte = self.peek_u8()?;
        self.buffer.pop_front();
        self.full_offset += 1;
        Ok(byte)
    }

    pub fn expect_u8(&mut self, expected: u8) -> Result<(), ValidationError> {
        let byte = self.peek_u8()?;
        if byte == expected {
            self.read_u8()?;
            Ok(())
        } else {
            Err(ValidationError::ExpectedTerminal(expected))
        }
    }

    /// Read a constant number of bytes into an array
    pub fn strip_bytes<const N: usize>(&mut self) -> Result<[u8; N], ValidationError> {
        let mut result = [0u8; N];
        for item in result.iter_mut().take(N) {
            *item = self.read_u8()?;
        }
        Ok(result)
    }

    /// Parses a variable-length `u32` as specified by [LEB128](https://en.wikipedia.org/wiki/LEB128#Unsigned_LEB128).
    /// Note: If `Err`, the [Reader] object is no longer guaranteed to be in a valid state
    /// This implementation is heavily based off of DLR's WASM interpreter:
    /// <https://github.com/DLR-FT/wasm-interpreter>
    pub fn read_u32(&mut self) -> Result<u32, ValidationError> {
        /// Because up to 5 bytes (each storing 7 bits) may be used to store 32 bits,
        /// some bits in the last byte will be left unused. This is a bitmask for
        /// exactly these bits in the last byte.
        const PADDING_IN_LAST_BYTE_BIT_MASK: u8 = 0b01110000;

        let mut result: u32 = 0;

        let byte = self.read_u8()?;
        result |= u32::from(byte & INTEGER_BIT_FLAG);
        if byte & CONTINUATION_BIT == 0 {
            return Ok(result);
        }

        let byte = self.read_u8()?;
        result |= u32::from(byte & INTEGER_BIT_FLAG) << 7;
        if byte & CONTINUATION_BIT == 0 {
            return Ok(result);
        }

        let byte = self.read_u8()?;
        result |= u32::from(byte & INTEGER_BIT_FLAG) << 14;
        if byte & CONTINUATION_BIT == 0 {
            return Ok(result);
        }

        let byte = self.read_u8()?;
        result |= u32::from(byte & INTEGER_BIT_FLAG) << 21;
        if byte & CONTINUATION_BIT == 0 {
            return Ok(result);
        }

        let byte = self.read_u8()?;
        result |= u32::from(byte & INTEGER_BIT_FLAG) << 28;

        // there can only be a maximum number of 5 bytes for a 32-bit integer
        let has_next_byte = byte & CONTINUATION_BIT > 0;
        let padding_bits_are_not_zero = byte & PADDING_IN_LAST_BYTE_BIT_MASK > 0;
        if has_next_byte || padding_bits_are_not_zero {
            return Err(ValidationError::MalformedInteger);
        }

        Ok(result)
    }

    pub fn read_f64(&mut self) -> Result<u64, ValidationError> {
        let bytes = self.strip_bytes::<8>()?;
        Ok(u64::from_le_bytes(bytes))
    }

    /// This implementation is heavily based off of DLR's WASM interpreter:
    /// <https://github.com/DLR-FT/wasm-interpreter>
    pub fn read_i32(&mut self) -> Result<i32, ValidationError> {
        /// Because up to 5 bytes (each storing 7 bits) may be used to store 32 bits,
        /// some bits in the last byte will be left unused. This is a bitmask for
        /// exactly these bits in the last byte.
        const PADDING_IN_LAST_BYTE_BITMASK: u8 = 0b01110000;

        /// This bitflag defines the position of the sign bit in the last byte.
        const SIGN_IN_LAST_BYTE_BITFLAG: u8 = 0b00001000;

        /// Number of bits in this number type
        const NUM_BITS: u32 = 32;

        let mut result: i32 = 0;

        let byte = self.read_u8()?;
        result |= i32::from(byte & INTEGER_BIT_FLAG);
        if byte & CONTINUATION_BIT == 0 {
            /// before returning the result, we need to sign extend the unspecified bits
            const NUM_UNSPECIFIED_BITS: u32 = NUM_BITS - 7;
            let sign_extended_result = (result << NUM_UNSPECIFIED_BITS) >> NUM_UNSPECIFIED_BITS;
            return Ok(sign_extended_result);
        }

        let byte = self.read_u8()?;
        result |= i32::from(byte & INTEGER_BIT_FLAG) << 7;
        if byte & CONTINUATION_BIT == 0 {
            const NUM_UNSPECIFIED_BITS: u32 = NUM_BITS - 14;
            let sign_extended_result = (result << NUM_UNSPECIFIED_BITS) >> NUM_UNSPECIFIED_BITS;
            return Ok(sign_extended_result);
        }

        let byte = self.read_u8()?;
        result |= i32::from(byte & INTEGER_BIT_FLAG) << 14;
        if byte & CONTINUATION_BIT == 0 {
            const NUM_UNSPECIFIED_BITS: u32 = NUM_BITS - 21;
            let sign_extended_result = (result << NUM_UNSPECIFIED_BITS) >> NUM_UNSPECIFIED_BITS;
            return Ok(sign_extended_result);
        }

        let byte = self.read_u8()?;
        result |= i32::from(byte & INTEGER_BIT_FLAG) << 21;
        if byte & CONTINUATION_BIT == 0 {
            const NUM_UNSPECIFIED_BITS: u32 = NUM_BITS - 28;
            let sign_extended_result = (result << NUM_UNSPECIFIED_BITS) >> NUM_UNSPECIFIED_BITS;
            return Ok(sign_extended_result);
        }

        let byte = self.read_u8()?;
        result |= i32::from(byte & INTEGER_BIT_FLAG) << 28;

        // there can only be a maximum number of 5 bytes for a 32-bit integer
        let has_next_byte = byte & CONTINUATION_BIT > 0;
        if has_next_byte {
            return Err(ValidationError::MalformedInteger);
        }

        // Verify that the padding and sign bits are either all ones or all
        // zeros. To do this we count the ones and check if that number is zero
        // or equal to the number of ones in both bitmasks combined.
        const PADDING_AND_SIGN_BITMASK: u8 =
            PADDING_IN_LAST_BYTE_BITMASK | SIGN_IN_LAST_BYTE_BITFLAG;
        let number_of_ones_in_padding_and_sign_bits =
            (byte & PADDING_AND_SIGN_BITMASK).count_ones();
        let padding_bits_match_sign_bit = number_of_ones_in_padding_and_sign_bits
            == PADDING_AND_SIGN_BITMASK.count_ones()
            || number_of_ones_in_padding_and_sign_bits == 0;
        if !padding_bits_match_sign_bit {
            return Err(ValidationError::MalformedInteger);
        }

        Ok(result)
    }

    pub fn read_f32(&mut self) -> Result<u32, ValidationError> {
        let bytes = self.strip_bytes::<4>()?;
        Ok(u32::from_le_bytes(bytes))
    }

    pub fn read_i64(&mut self) -> Result<i64, ValidationError> {
        /// Because up to 10 bytes (each storing 7 bits) may be used to store 64 bits,
        /// some bits in the last byte will be left unused. This is a bitmask for
        /// exactly these bits in the last byte.
        const PADDING_IN_LAST_BYTE_BITMASK: u8 = 0b01111110;

        /// This bitflag defines the position of the sign bit in the last byte.
        const SIGN_IN_LAST_BYTE_BITFLAG: u8 = 0b00000001;

        /// Number of bits in this number type
        const NUM_BITS: u32 = 64;

        let mut result: i64 = 0;

        let byte = self.read_u8()?;
        result |= i64::from(byte & INTEGER_BIT_FLAG);
        if byte & CONTINUATION_BIT == 0 {
            /// before returning the result, we need to sign extend the unspecified bits
            const NUM_UNSPECIFIED_BITS: u32 = NUM_BITS - 7;
            let sign_extended_result = (result << NUM_UNSPECIFIED_BITS) >> NUM_UNSPECIFIED_BITS;
            return Ok(sign_extended_result);
        }

        let byte = self.read_u8()?;
        result |= i64::from(byte & INTEGER_BIT_FLAG) << 7;
        if byte & CONTINUATION_BIT == 0 {
            const NUM_UNSPECIFIED_BITS: u32 = NUM_BITS - 14;
            let sign_extended_result = (result << NUM_UNSPECIFIED_BITS) >> NUM_UNSPECIFIED_BITS;
            return Ok(sign_extended_result);
        }

        let byte = self.read_u8()?;
        result |= i64::from(byte & INTEGER_BIT_FLAG) << 14;
        if byte & CONTINUATION_BIT == 0 {
            const NUM_UNSPECIFIED_BITS: u32 = NUM_BITS - 21;
            let sign_extended_result = (result << NUM_UNSPECIFIED_BITS) >> NUM_UNSPECIFIED_BITS;
            return Ok(sign_extended_result);
        }

        let byte = self.read_u8()?;
        result |= i64::from(byte & INTEGER_BIT_FLAG) << 21;
        if byte & CONTINUATION_BIT == 0 {
            const NUM_UNSPECIFIED_BITS: u32 = NUM_BITS - 28;
            let sign_extended_result = (result << NUM_UNSPECIFIED_BITS) >> NUM_UNSPECIFIED_BITS;
            return Ok(sign_extended_result);
        }

        let byte = self.read_u8()?;
        result |= i64::from(byte & INTEGER_BIT_FLAG) << 28;
        if byte & CONTINUATION_BIT == 0 {
            const NUM_UNSPECIFIED_BITS: u32 = NUM_BITS - 35;
            let sign_extended_result = (result << NUM_UNSPECIFIED_BITS) >> NUM_UNSPECIFIED_BITS;
            return Ok(sign_extended_result);
        }

        let byte = self.read_u8()?;
        result |= i64::from(byte & INTEGER_BIT_FLAG) << 35;
        if byte & CONTINUATION_BIT == 0 {
            const NUM_UNSPECIFIED_BITS: u32 = NUM_BITS - 42;
            let sign_extended_result = (result << NUM_UNSPECIFIED_BITS) >> NUM_UNSPECIFIED_BITS;
            return Ok(sign_extended_result);
        }

        let byte = self.read_u8()?;
        result |= i64::from(byte & INTEGER_BIT_FLAG) << 42;
        if byte & CONTINUATION_BIT == 0 {
            const NUM_UNSPECIFIED_BITS: u32 = NUM_BITS - 49;
            let sign_extended_result = (result << NUM_UNSPECIFIED_BITS) >> NUM_UNSPECIFIED_BITS;
            return Ok(sign_extended_result);
        }

        let byte = self.read_u8()?;
        result |= i64::from(byte & INTEGER_BIT_FLAG) << 49;
        if byte & CONTINUATION_BIT == 0 {
            const NUM_UNSPECIFIED_BITS: u32 = NUM_BITS - 56;
            let sign_extended_result = (result << NUM_UNSPECIFIED_BITS) >> NUM_UNSPECIFIED_BITS;
            return Ok(sign_extended_result);
        }

        let byte = self.read_u8()?;
        result |= i64::from(byte & INTEGER_BIT_FLAG) << 56;
        if byte & CONTINUATION_BIT == 0 {
            const NUM_UNSPECIFIED_BITS: u32 = NUM_BITS - 63;
            let sign_extended_result = (result << NUM_UNSPECIFIED_BITS) >> NUM_UNSPECIFIED_BITS;
            return Ok(sign_extended_result);
        }

        let byte = self.read_u8()?;
        result |= i64::from(byte & INTEGER_BIT_FLAG) << 63;

        // there can only be a maximum number of 10 bytes for a 64-bit integer
        let has_next_byte = byte & CONTINUATION_BIT > 0;
        if has_next_byte {
            return Err(ValidationError::MalformedInteger);
        }

        // Verify that the padding and sign bits are either all ones or all
        // zeros. To do this we count the ones and check if that number is zero
        // or equal to the number of ones in both bitmasks combined.
        const PADDING_AND_SIGN_BITMASK: u8 =
            PADDING_IN_LAST_BYTE_BITMASK | SIGN_IN_LAST_BYTE_BITFLAG;
        let number_of_ones_in_padding_and_sign_bits =
            (byte & PADDING_AND_SIGN_BITMASK).count_ones();
        let padding_bits_match_sign_bit = number_of_ones_in_padding_and_sign_bits
            == PADDING_AND_SIGN_BITMASK.count_ones()
            || number_of_ones_in_padding_and_sign_bits == 0;
        if !padding_bits_match_sign_bit {
            return Err(ValidationError::MalformedInteger);
        }

        Ok(result)
    }

    /// Skip over a fixed set of bytes and ignore them
    pub fn skip(&mut self, len: usize) -> Result<(), ValidationError> {
        for _ in 0..len {
            self.read_u8()?;
        }
        Ok(())
    }

    /// Note: If `Err`, the [Reader] object is no longer guaranteed to be in a valid state
    pub fn read_vec<T, F>(&mut self, read_element: F) -> Result<Vec<T>, ValidationError>
    where
        T: 'wasm,
        F: FnMut(&mut Self) -> Result<T, ValidationError>,
    {
        self.read_vec_in(GlobalAllocator, read_element)
    }

    pub fn read_vec_stack<const SIZE: usize, T>(
        &mut self,
        mut read_element: impl FnMut(&mut Self) -> Result<T, ValidationError>,
    ) -> Result<StaticVec<T, SIZE>, ValidationError>
    where
        T: 'wasm,
    {
        let len = self.read_u32()?;
        if len as usize > SIZE {
            return Err(ValidationError::VecTooLong);
        }

        let mut out = StaticVec::new();
        for _ in 0..len {
            out.push(read_element(self)?)?;
        }

        Ok(out)
    }

    pub fn read_vec_in<T, F, VA>(
        &mut self,
        alloc: VA,
        mut read_element: F,
    ) -> Result<Vec<T, VA>, ValidationError>
    where
        T: 'wasm,
        F: FnMut(&mut Self) -> Result<T, ValidationError>,
        VA: Allocator,
    {
        let len = self.read_u32()?;
        let mut out = Vec::new_in(alloc, len)?;
        for _ in 0..len {
            out.push(read_element(self)?);
        }

        Ok(out)
    }
}

impl<'wasm> Drop for Reader<'wasm> {
    fn drop(&mut self) {
        // Return the current chunk to the stream if one exists
        if let Some(mut chunk) = self.next.take() {
            chunk.return_(self.stream);
        }
    }
}

#[cfg(test)]
mod tests {
    // use crate::{alloc::run, InnerVec, ReaderError, StackAllocator, Vec, Reader, Stream};

    // struct TestStreamer {
    //     data: Option<InnerVec<u8>>,
    // }
    //
    // impl TestStreamer {
    //     fn new(bytes: &[u8]) -> Self {
    //         // Create a Vec and extract its InnerVec
    //         let mut vec = Vec::new(bytes.len() as u32).unwrap();
    //         for &byte in bytes {
    //             vec.push(byte);
    //         }
    //         let inner = unsafe { vec.take_inner() };
    //         Self {
    //             data: Some(inner),
    //         }
    //     }
    // }

    // impl Stream for TestStreamer {
    //     fn read(&mut self) -> Result<Option<InnerVec<u8>>, ReaderError> {
    //         Ok(self.data.take())
    //     }
    //
    //     fn return_(&mut self, _chunk: InnerVec<u8>) {
    //         // For tests, we don't need to reuse buffers
    //     }
    // }

    // #[test]
    // fn test_var_i32() {
    //     let alloc = StackAllocator::<1024, 8>::new();
    //     run(&alloc, || {
    //         let bytes = [0xC0, 0xBB, 0x78];
    //         let mut streamer = TestStreamer::new(&bytes);
    //         let mut wasm = Reader::new(&mut streamer);
    //
    //         assert_eq!(wasm.read_i32(), Ok(-123456));
    //     });
    // }
}
