// Wasm Reader
// This file implements some basic Wasm reading capabilities such
// as LEB128 (variable width integer encoding).
//
// Copyright 2026 California Institute of Technology
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// ---
// Portions of this file are derived from https://github.com/DLR-FT/wasm-interpreter:
// Copyright © 2024-2026 Deutsches Zentrum für Luft- und Raumfahrt e.V.
// (DLR).
// Copyright © 2024-2025 OxidOS Automotive SRL.
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

/// Cap on the in-memory size of a single heap vector decoded from module bytes.
/// `Vec::new_in` reserves `len * size_of::<T>()` up front, so without this a
/// few-byte module can drive a multi-gigabyte allocation (or overflow the
/// layout outright on 32-bit targets). A byte budget rather than an element
/// count so wide element types are bounded the same way. Sized to match the
/// `MAX_TABLE_ELEMENTS` bound in `types.rs`.
const MAX_DECODE_BYTES: u64 = 10 * 1024 * 1024;

/// A struct for managing and reading Wasm bytecode
/// Its purpose is to abstract parsing basic Wasm values from the bytecode
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

    /// A fixed size circular buffer meant to hold as much Wasm data as it can.
    /// Wasm chunks may be of variable length, so a single value may span multiple
    /// chunks. Data is copied into this circular buffer and processing is done here.
    buffer: CircularBuffer<u8, 64>,

    /// A counter keeping track of the total number of bytes we've processed in the Wasm binary
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
    ///
    /// The `is_empty()` early-return below is load-bearing for the copy loops:
    /// because we only ever refill an empty buffer, the free space always equals
    /// `capacity()`, so clamping `to_copy` to `capacity()` never overfills and
    /// `CircularBuffer::push` (which overwrites the oldest element when full)
    /// never actually overwrites live data.
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
    /// This implementation is heavily based off of DLR's Wasm interpreter:
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

    /// Read a little-endian `f64` and return its raw bit pattern.
    pub fn read_f64_bits(&mut self) -> Result<u64, ValidationError> {
        let bytes = self.strip_bytes::<8>()?;
        Ok(u64::from_le_bytes(bytes))
    }

    /// This implementation is heavily based off of DLR's Wasm interpreter:
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

    /// Read a little-endian `f32` and return its raw bit pattern.
    pub fn read_f32_bits(&mut self) -> Result<u32, ValidationError> {
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
        if u64::from(len) * size_of::<T>() as u64 > MAX_DECODE_BYTES {
            return Err(ValidationError::VecTooLong);
        }
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
    use super::*;
    use crate::InnerVec;

    extern crate std;
    use std::vec::Vec as StdVec;

    /// A [`WasmStream`] that hands out its backing bytes in fixed-size chunks,
    /// pointing each [`InnerVec`] directly into the borrowed slice (mirroring
    /// the C API `Cursor` streaming test). A `chunk_size` of 1 forces a stream
    /// round-trip for essentially every byte, and any size smaller than a
    /// multi-byte value makes that value straddle a chunk boundary — exactly
    /// the cross-chunk decode path that [`Reader::fill_buffer`] must handle.
    struct ChunkStream<'a> {
        data: &'a [u8],
        pos: usize,
        chunk_size: usize,
    }

    impl<'a> ChunkStream<'a> {
        fn new(data: &'a [u8], chunk_size: usize) -> Self {
            assert!(chunk_size > 0);
            Self {
                data,
                pos: 0,
                chunk_size,
            }
        }
    }

    impl WasmStream for ChunkStream<'_> {
        fn read(&mut self) -> Result<Option<InnerVec<u8>>, u8> {
            let remaining = self.data.len() - self.pos;
            if remaining == 0 {
                // No more chunks: signals EOF to the reader.
                return Ok(None);
            }
            let n = remaining.min(self.chunk_size);
            // SAFETY: `data` outlives every chunk we hand out; the reader only
            // reads the bytes (never writes through the pointer) and copies
            // them into its own buffer before requesting the next chunk.
            let ptr = unsafe { self.data.as_ptr().add(self.pos) as *mut u8 };
            self.pos += n;
            Ok(Some(InnerVec {
                ptr,
                capacity: n as u32,
                len: n as u32,
            }))
        }

        // The backing slice is owned by the caller and stays alive for the
        // whole test, so there is nothing to reclaim.
        fn return_(&mut self, _chunk: InnerVec<u8>) {}
    }

    /// Decode a single value from `data` under several chunk sizes (1, 7, and
    /// a single whole-buffer chunk) and assert it always equals `expected`,
    /// with the reader landing exactly at end-of-input.
    fn for_chunks<T: PartialEq + core::fmt::Debug>(
        data: &[u8],
        expected: T,
        mut f: impl FnMut(&mut Reader) -> Result<T, ValidationError>,
    ) {
        for &sz in &[1usize, 7, data.len().max(1)] {
            let mut stream = ChunkStream::new(data, sz);
            let mut reader = Reader::new(&mut stream);
            assert_eq!(f(&mut reader).unwrap(), expected, "value (chunk_size={sz})");
            assert_eq!(reader.offset(), data.len(), "offset (chunk_size={sz})");
        }
    }

    #[test]
    fn read_u8_across_chunks() {
        for_chunks(&[0xAB], 0xABu8, |r| r.read_u8());
    }

    #[test]
    fn read_u32_leb128_multibyte() {
        // 624485 == [0xE5, 0x8E, 0x26] (3-byte unsigned LEB128).
        for_chunks(&[0xE5, 0x8E, 0x26], 624485u32, |r| r.read_u32());
        // Single-byte and boundary values.
        for_chunks(&[0x00], 0u32, |r| r.read_u32());
        for_chunks(&[0x7F], 127u32, |r| r.read_u32());
        // Full 5-byte encoding of u32::MAX.
        for_chunks(&[0xFF, 0xFF, 0xFF, 0xFF, 0x0F], u32::MAX, |r| r.read_u32());
    }

    #[test]
    fn read_i32_leb128_negative() {
        for_chunks(&[0x80, 0x7F], -128i32, |r| r.read_i32()); // 2-byte
        for_chunks(&[0xC0, 0xBB, 0x78], -123456i32, |r| r.read_i32()); // 3-byte
        for_chunks(&[0x7F], -1i32, |r| r.read_i32()); // 1-byte sign-extended
    }

    #[test]
    fn read_i64_leb128_full_width() {
        for_chunks(&[0xD2, 0x09], 1234i64, |r| r.read_i64());
        for_chunks(&[0xAE, 0x76], -1234i64, |r| r.read_i64());
        // i64::MIN uses the full 10-byte continuation path.
        let min = [0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x7F];
        for_chunks(&min, i64::MIN, |r| r.read_i64());
    }

    #[test]
    fn read_floats_little_endian() {
        for_chunks(&3.5f32.to_le_bytes(), 3.5f32.to_bits(), |r| {
            r.read_f32_bits()
        });
        for_chunks(&(-2.25f64).to_le_bytes(), (-2.25f64).to_bits(), |r| {
            r.read_f64_bits()
        });
    }

    #[test]
    fn strip_bytes_skip_and_expect_across_chunks() {
        let data = [1u8, 2, 3, 4, 5, 6];
        // Feed one byte at a time so each read crosses a chunk boundary.
        let mut stream = ChunkStream::new(&data, 1);
        let mut reader = Reader::new(&mut stream);
        assert_eq!(reader.strip_bytes::<3>().unwrap(), [1, 2, 3]);
        reader.skip(1).unwrap(); // skips the 4
        assert_eq!(reader.read_u8().unwrap(), 5);
        reader.expect_u8(6).unwrap();
        assert_eq!(reader.offset(), data.len());
    }

    #[test]
    fn expect_u8_mismatch_errors_without_consuming() {
        let data = [0x09u8];
        let mut stream = ChunkStream::new(&data, 1);
        let mut reader = Reader::new(&mut stream);
        assert_eq!(
            reader.expect_u8(0x00),
            Err(ValidationError::ExpectedTerminal(0x00))
        );
        // The mismatched byte is not consumed and can still be read.
        assert_eq!(reader.read_u8().unwrap(), 0x09);
    }

    #[test]
    fn eof_is_reported_on_empty_and_exhausted_stream() {
        // Empty stream.
        let mut stream = ChunkStream::new(&[], 1);
        let mut reader = Reader::new(&mut stream);
        assert_eq!(reader.read_u8(), Err(ValidationError::Eof));
        drop(reader);

        // Exhausted after reading the only byte.
        let data = [0x42u8];
        let mut stream = ChunkStream::new(&data, 1);
        let mut reader = Reader::new(&mut stream);
        assert_eq!(reader.read_u8().unwrap(), 0x42);
        assert_eq!(reader.read_u8(), Err(ValidationError::Eof));
    }

    #[test]
    fn read_vec_across_chunks() {
        // A LEB128 length (3) followed by three u8 elements, one byte per chunk.
        let data = [0x03u8, 0x0A, 0x0B, 0x0C];
        let mut stream = ChunkStream::new(&data, 1);
        let mut reader = Reader::new(&mut stream);
        // `read_vec` allocates through the `GlobalAllocator`, which the crate
        // test binary backs via the `__spacewasm_alloc` shim in `lib.rs`.
        let v = reader.read_vec(|r| r.read_u8()).unwrap();
        assert_eq!(&*v, &[0x0A, 0x0B, 0x0C]);
        assert_eq!(reader.offset(), data.len());
    }

    #[test]
    fn read_vec_in_rejects_oversized_byte_budget() {
        // 1_000_000 entries is far below any plausible element-count cap, but
        // at 16 bytes each that is a 16 MiB allocation - the byte budget has
        // to reject it before `Vec::new_in` reserves the space. The length
        // prefix is enough; the check returns before any element is read.
        let data = [0xC0u8, 0x84, 0x3D]; // LEB128 1_000_000
        let mut stream = ChunkStream::new(&data, 3);
        let mut reader = Reader::new(&mut stream);
        assert!(matches!(
            reader.read_vec(|r| r.strip_bytes::<16>()),
            Err(ValidationError::VecTooLong)
        ));
    }

    #[test]
    fn single_large_chunk_refills_circular_buffer_repeatedly() {
        // A payload larger than the 64-byte circular buffer, delivered as a
        // SINGLE chunk, forces `fill_buffer` to re-enter its "copy more from
        // the still-held chunk" branch each time the buffer drains (three times
        // for 200 bytes). Reading every byte back byte-exact across those
        // refills proves that path independently of `mixed_sequence_*`.
        let mut data = StdVec::new();
        for i in 0..200u32 {
            data.push((i.wrapping_mul(7).wrapping_add(3)) as u8);
        }
        let mut stream = ChunkStream::new(&data, data.len()); // one chunk
        let mut reader = Reader::new(&mut stream);
        for (i, &expected) in data.iter().enumerate() {
            assert_eq!(reader.read_u8().unwrap(), expected, "byte {i}");
        }
        assert_eq!(reader.offset(), data.len());
        assert_eq!(reader.read_u8(), Err(ValidationError::Eof));
    }

    #[test]
    fn mixed_sequence_across_chunks() {
        // A heterogeneous value stream decoded back under several chunk sizes.
        // chunk_size 1 forces a stream round-trip per byte; 3 and 7 make the
        // multi-byte values straddle boundaries at varying offsets; and the
        // whole-buffer case (> 64 bytes) exercises refilling the 64-byte
        // circular buffer repeatedly from a single large chunk.
        let mut data = StdVec::new();
        data.push(0x2Au8); // u8 = 42
        data.extend_from_slice(&[0xE5, 0x8E, 0x26]); // u32 LEB = 624485
        data.extend_from_slice(&[0xC0, 0xBB, 0x78]); // i32 LEB = -123456
        data.extend_from_slice(&[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x7F]); // i64 = i64::MIN
        data.extend_from_slice(&3.5f32.to_le_bytes()); // f32 bits
        data.extend_from_slice(&(-2.25f64).to_le_bytes()); // f64 bits
        data.extend_from_slice(&[0x11, 0x22, 0x33, 0x44]); // strip_bytes::<4>
        data.extend_from_slice(&[0x55; 40]); // skip(40) of filler
        data.push(0x77); // final u8
        let total = data.len();
        assert!(total > 64, "buffer must exceed the 64-byte circular buffer");

        for &sz in &[1usize, 3, 7, total] {
            let mut stream = ChunkStream::new(&data, sz);
            let mut reader = Reader::new(&mut stream);
            assert_eq!(reader.read_u8().unwrap(), 0x2A, "u8 (sz={sz})");
            assert_eq!(reader.read_u32().unwrap(), 624485, "u32 (sz={sz})");
            assert_eq!(reader.read_i32().unwrap(), -123456, "i32 (sz={sz})");
            assert_eq!(reader.read_i64().unwrap(), i64::MIN, "i64 (sz={sz})");
            assert_eq!(
                reader.read_f32_bits().unwrap(),
                3.5f32.to_bits(),
                "f32 (sz={sz})"
            );
            assert_eq!(
                reader.read_f64_bits().unwrap(),
                (-2.25f64).to_bits(),
                "f64 (sz={sz})"
            );
            assert_eq!(
                reader.strip_bytes::<4>().unwrap(),
                [0x11, 0x22, 0x33, 0x44],
                "strip (sz={sz})"
            );
            reader.skip(40).unwrap();
            assert_eq!(reader.read_u8().unwrap(), 0x77, "final (sz={sz})");
            assert_eq!(reader.offset(), total, "offset (sz={sz})");
            assert_eq!(reader.read_u8(), Err(ValidationError::Eof), "eof (sz={sz})");
        }
    }
}
