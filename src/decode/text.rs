use crate::util::StaticVec;
use crate::{AllocError, Box, MemArg, ValidationError};

pub struct TextPage(pub [u16; 256]);
impl Default for TextPage {
    fn default() -> TextPage {
        TextPage([0; 256])
    }
}

pub struct JumpTarget(u32);

impl JumpTarget {
    pub fn page(&self) -> usize {
        (self.0 >> 8) as usize
    }

    pub fn offset(&self) -> usize {
        (self.0 & 0xFF) as usize
    }
}

pub struct TextBuilder<const N: usize> {
    pages: StaticVec<Box<TextPage>, N>,
    offset: usize,
}

impl<const N: usize> TextBuilder<N> {
    pub fn new() -> Result<TextBuilder<N>, AllocError> {
        let mut o = TextBuilder {
            pages: Default::default(),
            offset: 0,
        };

        o.add_page()?;
        Ok(o)
    }

    pub fn pc(&self) -> JumpTarget {
        let (page_index, offset) = {
            if self.offset == 256 {
                // We are at the start of the next page, we have not allocated it yet
                // The address is still valid so we can manually convert it
                (self.pages.len() + 1, 0)
            } else {
                (self.pages.len(), self.offset)
            }
        };

        // We support up to 24-bit page addresses
        assert!(page_index < (2 << 24));
        assert!(offset < 256);

        JumpTarget(((page_index as u32) << 8) | (offset as u32))
    }

    fn add_page(&mut self) -> Result<(), AllocError> {
        self.pages.push(Box::new(TextPage::default())?)?;
        self.offset = 0;
        Ok(())
    }

    fn push(&mut self, c: u16) -> Result<(), AllocError> {
        if self.offset >= 256 {
            self.add_page()?;
        }

        let idx = self.pages.len() - 1;
        self.pages[idx].0[self.offset] = c;
        self.offset += 1;

        Ok(())
    }

    pub(crate) fn push_no_operand(&mut self, op: u8) -> Result<(), AllocError> {
        self.push((op as u16) << 8)
    }

    /// Push an opcode with 7 or 23 bits
    pub(crate) fn push_23(&mut self, op: u8, idx: u32) -> Result<(), ValidationError> {
        // We support up to 7 + 16 bits
        if idx >= 1 << 23 {
            Err(ValidationError::IdxTooLarge)
        } else {
            let mut idx_first = (idx & 0x7F) as u16;
            if idx > 1 << 7 {
                idx_first |= 0x80;
            }

            self.push((op as u16) << 8 | idx_first)?;

            if idx > 1 << 7 {
                self.push((idx - (idx_first as u32)) as u16)?;
            }

            Ok(())
        }
    }

    /// Push an operation with a MemArg operand.
    pub(crate) fn push_mem(&mut self, op: u8, m: MemArg) -> Result<(), ValidationError> {
        // Check if the alignment exponent can fit into 8-bits
        if m.align >= 0xFF {
            Err(ValidationError::MemAlignTooLarge)
        } else {
            self.push((op as u16) << 8 | (m.align as u16))?;
            self.push((m.offset & 0xFFFF) as u16)?;
            self.push(((m.offset >> 16) & 0xFFFF) as u16)?;
            Ok(())
        }
    }

    /// Push an operation with a 7-bit or 32-bit (fixed size) operand
    pub(crate) fn push_32(&mut self, op: u8, i: u32) -> Result<(), AllocError> {
        if i <= 0x7F {
            let first = ((op as u16) << 8) | i as u16;
            self.push(first)?;
        } else {
            let first = ((op as u16) << 8) | 0xFF;
            let lo = (i & 0xFFFF) as u16;
            let hi = (i >> 16) as u16;

            self.push(first)?;
            self.push(lo)?;
            self.push(hi)?;
        }

        Ok(())
    }

    /// Push an operation with a 7-bit or 64-bit operand
    pub(crate) fn push_64(&mut self, op: u8, i: u64) -> Result<(), AllocError> {
        if i <= 0x7F {
            let first = ((op as u16) << 8) | i as u16;
            self.push(first)?;
        } else {
            let first = ((op as u16) << 8) | 0xFF;
            let w1 = i as u16;
            let w2 = (i >> 16) as u16;
            let w3 = (i >> 32) as u16;
            let w4 = (i >> 48) as u16;

            self.push(first)?;
            self.push(w1)?;
            self.push(w2)?;
            self.push(w3)?;
            self.push(w4)?;
        }

        Ok(())
    }
}
