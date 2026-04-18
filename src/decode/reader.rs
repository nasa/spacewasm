/// WASM Reader
/// This file implements some basic WASM reading capabilities such
/// as LEB128 (variable width integer encoding).
use crate::{AllocError, Allocator, GlobalAllocator, ReaderError, StaticVec, ValidationError, Vec, WasmChunk, WasmStreamer};


/// Wasm encodes integers according to the LEB128 format, which specifies that
/// only 7 bits of every byte are used to store the integer's bits. The 8th bit
/// is always used as a bitflag for whether the next byte shall also be read as
/// part of the current integer. Therefore, it can be called a continuation bit,
/// which is stored here as a global constant to improve code readability.
const CONTINUATION_BIT: u8 = 0b10000000;

const INTEGER_BIT_FLAG: u8 = !CONTINUATION_BIT;

/// A struct for managing and reading WASM bytecode
/// Its purpose is to abstract parsing basic WASM values from the bytecode.
pub(crate) struct WasmReader<'wasm> {
    stream: &'wasm dyn WasmStreamer,

    /// All the currently loaded chunks of the WASM binary
    frames: Vec<WasmChunk<'wasm>>,

    /// Read offset pointer in the current frame
    offset: usize,

    /// A counter keeping track of the byte offset of
    /// the current chunk of the WASM binary. This will be incremented
    /// whenever a new chunk is received
    frame_offset: usize,
}

impl<'wasm> WasmReader<'wasm> {
    pub fn new(stream: &'wasm dyn WasmStreamer, max_concurrent_chunks: u32) -> Result<Self, AllocError> {
        Ok(Self {
            stream,
            frames: Vec::new(max_concurrent_chunks)?,
            offset: 0,
            frame_offset: 0,
        })
    }

    pub fn offset(&self) -> usize {
        self.offset + self.frame_offset
    }

    fn peek_u8(&mut self) -> Result<u8, ValidationError> {
        self.ensure(1).map_err(|_| ValidationError::Eof)?;
        let frame = self.frames.last().ok_or(ValidationError::Eof)?;
        frame.get(self.offset).copied().ok_or(ValidationError::Eof)
    }

    /// Tries to read one byte and fails if the end of file is reached.
    pub fn read_u8(&mut self) -> Result<u8, ValidationError> {
        let byte = self.peek_u8()?;
        self.offset += 1;
        Ok(byte)
    }

    pub fn expect_u8(&mut self, expected: u8) -> Result<(), ValidationError> {
        let byte = self.peek_u8()?;
        if byte == expected {
            self.offset += 1;
            Ok(())
        } else {
            Err(ValidationError::ExpectedTerminal(expected))
        }
    }

    pub fn strip_bytes<const N: usize>(&mut self) -> Result<[u8; N], ValidationError> {
        self.ensure(N).map_err(|_| ValidationError::Eof)?;
        let frame = self.frames.last().ok_or(ValidationError::Eof)?;
        let bytes = &frame[self.offset..self.offset + N];
        self.offset += N;
        Ok(bytes.try_into().unwrap())
    }

    /// Parses a variable-length `u32` as specified by [LEB128](https://en.wikipedia.org/wiki/LEB128#Unsigned_LEB128).
    /// Note: If `Err`, the [WasmReader] object is no longer guaranteed to be in a valid state
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
            return Err(ValidationError::MalformedVariableLengthInteger);
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
            return Err(ValidationError::MalformedVariableLengthInteger);
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
            return Err(ValidationError::MalformedVariableLengthInteger);
        }

        Ok(result)
    }

    pub fn read_var_i33_as_u32(&mut self) -> Result<u32, ValidationError> {
        /// Because up to 5 bytes (each storing 7 bits) may be used to store 32 bits,
        /// some bits in the last byte will be left unused. This is a bitmask for
        /// exactly these bits in the last byte.
        const PADDING_IN_LAST_BYTE_BITMASK: u8 = 0b01100000;

        /// This bitflag defines the position of the sign bit in the last byte.
        const SIGN_IN_LAST_BYTE_BITFLAG: u8 = 0b00010000;

        /// Number of bits in this number type
        const NUM_BITS: u32 = 33;

        let mut result: i64 = 0;

        let byte = self.read_u8()?;
        result |= i64::from(byte & INTEGER_BIT_FLAG);
        if byte & CONTINUATION_BIT == 0 {
            /// before returning the result, we need to sign extend the unspecified bits
            const NUM_UNSPECIFIED_BITS: u32 = NUM_BITS - 7;
            let sign_extended_result = (result << NUM_UNSPECIFIED_BITS) >> NUM_UNSPECIFIED_BITS;
            return u32::try_from(sign_extended_result).map_err(|_| ValidationError::I33IsNegative);
        }

        let byte = self.read_u8()?;
        result |= i64::from(byte & INTEGER_BIT_FLAG) << 7;
        if byte & CONTINUATION_BIT == 0 {
            const NUM_UNSPECIFIED_BITS: u32 = NUM_BITS - 14;
            let sign_extended_result = (result << NUM_UNSPECIFIED_BITS) >> NUM_UNSPECIFIED_BITS;
            return u32::try_from(sign_extended_result).map_err(|_| ValidationError::I33IsNegative);
        }

        let byte = self.read_u8()?;
        result |= i64::from(byte & INTEGER_BIT_FLAG) << 14;
        if byte & CONTINUATION_BIT == 0 {
            const NUM_UNSPECIFIED_BITS: u32 = NUM_BITS - 21;
            let sign_extended_result = (result << NUM_UNSPECIFIED_BITS) >> NUM_UNSPECIFIED_BITS;
            return u32::try_from(sign_extended_result).map_err(|_| ValidationError::I33IsNegative);
        }

        let byte = self.read_u8()?;
        result |= i64::from(byte & INTEGER_BIT_FLAG) << 21;
        if byte & CONTINUATION_BIT == 0 {
            const NUM_UNSPECIFIED_BITS: u32 = NUM_BITS - 28;
            let sign_extended_result = (result << NUM_UNSPECIFIED_BITS) >> NUM_UNSPECIFIED_BITS;
            return u32::try_from(sign_extended_result).map_err(|_| ValidationError::I33IsNegative);
        }

        let byte = self.read_u8()?;
        result |= i64::from(byte & INTEGER_BIT_FLAG) << 28;

        // there can only be a maximum number of 5 bytes for a 33-bit integer
        let has_next_byte = byte & CONTINUATION_BIT > 0;
        if has_next_byte {
            return Err(ValidationError::MalformedVariableLengthInteger);
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
            return Err(ValidationError::MalformedVariableLengthInteger);
        }

        u32::try_from(result).map_err(|_| ValidationError::I33IsNegative)
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
            return Err(ValidationError::MalformedVariableLengthInteger);
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
            return Err(ValidationError::MalformedVariableLengthInteger);
        }

        Ok(result)
    }

    pub fn skip(&mut self, len: usize) -> Result<(), ValidationError> {
        self.ensure(len).map_err(|_| ValidationError::Eof)?;
        self.offset += len;
        Ok(())
    }

    /// Note: If `Err`, the [WasmReader] object is no longer guaranteed to be in a valid state
    pub fn read_vec<T, F>(&mut self, read_element: F) -> Result<Vec<T>, ValidationError>
    where
        T: 'wasm,
        F: FnMut(&mut Self) -> Result<T, ValidationError>,
    {
        self.read_vec_in(GlobalAllocator, read_element)
    }

    pub fn read_vec_stack<T, F, const SIZE: usize>(
        &mut self,
        mut read_element: F,
    ) -> Result<StaticVec<T, SIZE>, ValidationError>
    where
        T: 'wasm,
        F: FnMut(&mut Self) -> Result<T, ValidationError>,
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

#[cfg(test)]
mod tests {
    use crate::{InnerVec, WasmReader, WasmStreamer, ReaderError, Vec};
    use core::cell::RefCell;

    struct TestStreamer {
        data: RefCell<Option<InnerVec<u8>>>,
    }

    impl TestStreamer {
        fn new(bytes: &[u8]) -> Self {
            // Create a Vec and extract its InnerVec
            let mut vec = Vec::new(bytes.len() as u32).unwrap();
            for &byte in bytes {
                vec.push(byte);
            }
            let inner = unsafe { vec.take_inner() };
            Self {
                data: RefCell::new(Some(inner)),
            }
        }
    }

    impl WasmStreamer for TestStreamer {
        fn read(&self) -> Result<Option<InnerVec<u8>>, ReaderError> {
            Ok(self.data.borrow_mut().take())
        }

        fn return_(&self, _chunk: InnerVec<u8>) {
            // For tests, we don't need to reuse buffers
        }
    }

    #[test]
    fn test_var_i32() {
        let bytes = [0xC0, 0xBB, 0x78];
        let streamer = TestStreamer::new(&bytes);
        let mut wasm = WasmReader::new(&streamer, 4).unwrap();

        assert_eq!(wasm.read_i32(), Ok(-123456));
    }
}
