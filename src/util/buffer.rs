use core::str::Utf8Error;

pub enum ReaderError {
    ReadOverflow { offset: usize },
    ReadBitOverflow { offset: usize, max_bits: usize },
    BadUtf8 { offset: usize, err: Utf8Error },
}

pub type ReaderResult<T> = core::result::Result<T, ReaderError>;

pub struct Reader<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &[u8]) -> Reader<'_> {
        Reader { data, offset: 0 }
    }

    pub fn read_u7(&mut self) -> ReaderResult<u8> {
        Ok(self.read_leb128_unsigned::<7>()? as u8)
    }

    pub fn read_i7(&mut self) -> ReaderResult<i8> {
        Ok(self.read_leb128_signed::<7>()? as i8)
    }

    pub fn read_u32(&mut self) -> ReaderResult<u32> {
        Ok(self.read_leb128_unsigned::<32>()? as u32)
    }

    pub fn read_i32(&mut self) -> ReaderResult<i32> {
        Ok(self.read_leb128_signed::<32>()? as i32)
    }

    pub fn read_u64(&mut self) -> ReaderResult<u64> {
        self.read_leb128_unsigned::<64>()
    }

    pub fn read_i64(&mut self) -> ReaderResult<i64> {
        self.read_leb128_signed::<64>()
    }

    pub fn read_utf8(&mut self) -> ReaderResult<&str> {
        let length = self.read_u32()? as usize;
        if self.offset + length >= self.data.len() {
            Err(ReaderError::ReadOverflow {
                offset: self.offset,
            })
        } else {
            let out = &self.data[self.offset..self.offset + length];
            self.offset += length;

            match core::str::from_utf8(out) {
                Ok(s) => Ok(s),
                Err(err) => Err(ReaderError::BadUtf8 {
                    offset: self.offset - length,
                    err,
                }),
            }
        }
    }

    fn read_leb128_unsigned<const MAX_BITS: usize>(&mut self) -> ReaderResult<u64> {
        let mut value = 0u64;
        let mut shift = 0;

        loop {
            let Some(byte) = self.data.get(self.offset) else {
                return Err(ReaderError::ReadOverflow {
                    offset: self.offset,
                });
            };

            self.offset += 1;

            value |= ((byte & 0x7f) as u64) << shift;
            shift += 7;

            if (byte & 0x80) == 0 {
                return Ok(value);
            }

            if shift > MAX_BITS {
                return Err(ReaderError::ReadBitOverflow {
                    offset: self.offset,
                    max_bits: MAX_BITS,
                });
            }
        }
    }

    fn read_leb128_signed<const MAX_BITS: usize>(&mut self) -> ReaderResult<i64> {
        let mut value = 0i64;
        let mut shift = 0;

        loop {
            let Some(byte) = self.data.get(self.offset) else {
                return Err(ReaderError::ReadOverflow {
                    offset: self.offset,
                });
            };

            self.offset += 1;

            value |= ((byte & 0x7f) as i64) << shift;
            shift += 7;

            if (byte & 0x80) == 0 {
                // Perform sign extension if negative
                if (byte & 0x40) != 0 && shift < 64 {
                    value |= (-1i64) << shift;
                }

                return Ok(value);
            }

            if shift > MAX_BITS {
                return Err(ReaderError::ReadBitOverflow {
                    offset: self.offset,
                    max_bits: MAX_BITS,
                });
            }
        }
    }
}
