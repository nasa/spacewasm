use crate::{AllocError, Box, LabelIdx, MemArg, StaticVec, ValidationError};

pub struct TextPage(pub [u16; 256]);
impl Default for TextPage {
    fn default() -> TextPage {
        TextPage([0; 256])
    }
}

#[derive(PartialEq, Eq, Clone, Copy)]
pub struct JumpTarget(u32);

impl core::ops::Add<u32> for JumpTarget {
    type Output = JumpTarget;
    fn add(self, rhs: u32) -> JumpTarget {
        JumpTarget(self.0 + rhs)
    }
}

impl JumpTarget {
    const SENTINEL: JumpTarget = JumpTarget(0x7FFF_FFFFu32);

    pub fn page(&self) -> usize {
        (self.0 >> 8) as usize
    }
    pub fn offset(&self) -> usize {
        (self.0 & 0xFF) as usize
    }
}

#[derive(Clone, Copy)]
struct ControlFrame(JumpTarget);

impl ControlFrame {
    /// Jump to the end of the block.
    /// This is one of two values:
    ///   The sentinel value [0x7FFF_FFFF] which indicates no more back patching is needed
    ///   An address in the IR that needs back patching
    fn forward(jt: JumpTarget) -> ControlFrame {
        assert!(jt.0 < 0x8000_0000);
        ControlFrame(jt)
    }

    fn backward(jt: JumpTarget) -> ControlFrame {
        assert!(jt.0 < 0x8000_0000);
        ControlFrame(JumpTarget(jt.0 | 0x8000_0000))
    }

    fn is_forward(&self) -> bool {
        (self.0.0 & 0x8000_0000) == 0
    }

    fn address(&self) -> JumpTarget {
        JumpTarget(self.0.0 & 0x7FFF_FFFF)
    }

    fn set_address(&mut self, addr: JumpTarget) {
        assert!(self.0.0 < 0x8000_0000);
        self.0.0 = addr.0 | (self.0.0 & 0x8000_0000);
    }
}

struct CodeBuilder<const N: usize> {
    pages: StaticVec<Box<TextPage>, N>,
    offset: usize,
}

impl<const N: usize> CodeBuilder<N> {
    fn pc(&self) -> JumpTarget {
        let (page_index, offset) = {
            if self.offset == 256 {
                // We are at the start of the next page, we have not allocated it yet
                // The address is still valid so we can manually convert it
                (self.pages.len(), 0)
            } else if self.pages.len() == 0 {
                // We haven't started writing instructions yet
                (0, 0)
            } else {
                (self.pages.len() - 1, self.offset)
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

    fn write(&mut self, address: JumpTarget, value: u16) -> Result<(), ValidationError> {
        let page_index = address.page();
        let offset = address.offset();

        if page_index >= self.pages.len() || offset >= 256 {
            Err(ValidationError::PageFault)
        } else {
            self.pages[page_index].0[offset] = value;
            Ok(())
        }
    }

    fn read(&mut self, address: JumpTarget) -> Result<u16, ValidationError> {
        let page_index = address.page();
        let offset = address.offset();

        if page_index >= self.pages.len() || offset >= 256 {
            Err(ValidationError::PageFault)
        } else {
            Ok(self.pages[page_index].0[offset])
        }
    }
}

pub struct TextBuilder<const N: usize> {
    code: CodeBuilder<N>,
    control_frames: StaticVec<ControlFrame, 64>,
    else_frames: StaticVec<JumpTarget, 64>,
}

impl<const N: usize> TextBuilder<N> {
    pub fn new() -> Result<TextBuilder<N>, AllocError> {
        let mut o = TextBuilder {
            code: CodeBuilder {
                pages: Default::default(),
                offset: 0,
            },
            control_frames: Default::default(),
            else_frames: Default::default(),
        };

        o.code.add_page()?;
        Ok(o)
    }

    pub fn pc(&self) -> JumpTarget {
        self.code.pc()
    }

    pub(crate) fn enter_forward_block(&mut self) -> Result<(), AllocError> {
        self.control_frames
            .push(ControlFrame::forward(JumpTarget::SENTINEL).into())
    }

    pub(crate) fn enter_backward_block(&mut self) -> Result<(), AllocError> {
        self.control_frames
            .push(ControlFrame::backward(self.code.pc()).into())
    }

    pub(crate) fn exit_block(&mut self) -> Result<(), ValidationError> {
        let Some(last) = self.control_frames.pop() else {
            return Err(ValidationError::InvalidEndBlock);
        };

        if last.is_forward() {
            let pc = self.code.pc();
            let mut next = last.address();

            // Back-patch all the jump targets in the code
            // We know the first address since it is written in the control frame
            // All the other addresses are written as placeholders in the code text
            let mut n = 0;
            while next != JumpTarget::SENTINEL {

                // Bound our loops to not go into an infinite cycle
                n += 1;
                if n > 100 {
                    return Err(ValidationError::PossibleBackpatchCycle);
                }

                let address = next;

                // Read the next address before we overwrite it
                let w1 = self.code.read(address)?;
                let w2 = self.code.read(address + 1)?;
                next = JumpTarget((w1 as u32) | ((w2 as u32) << 16));

                // Overwrite the next address with our program counter
                self.code.write(address, pc.0 as u16)?;
                self.code.write(address + 1, (pc.0 >> 16) as u16)?;
            }
        } else {
            // We don't need to do any back-patching here
            // Backward control frames already knew their start address
        }

        Ok(())
    }

    pub(crate) fn start_else(&mut self) -> Result<(), AllocError> {
        self.else_frames.push(self.code.pc())?;

        // Write the sentinel
        self.code.push(0xFFFF)?;
        self.code.push(0x7FFF)?;
        Ok(())
    }

    pub(crate) fn finish_else(&mut self) -> Result<(), ValidationError> {
        let Some(frame) = self.else_frames.pop() else {
            return Err(ValidationError::InvalidElseBlock);
        };

        let pc = self.code.pc();
        self.code.write(frame, pc.0 as u16)?;
        self.code.write(frame + 1, (pc.0 >> 16) as u16)?;
        Ok(())
    }

    pub(crate) fn push_jump_target(&mut self, label: LabelIdx) -> Result<(), ValidationError> {
        if label.0 as usize >= self.control_frames.len() {
            Err(ValidationError::InvalidLabelIndex)
        } else {
            let idx = self.control_frames.len() - 1 - label.0 as usize;
            let frame = &mut self.control_frames[idx];
            let address = frame.address();
            if frame.is_forward() {
                // Forward jump targets use a linked-list of addresses to denote their back-patching
                // We write the last head here and update the head to our address
                frame.set_address(self.code.pc());
            } else {
                // The address of a backward jump is already known
                // We can write the address directly
            }

            self.code.push(address.0 as u16)?;
            self.code.push((address.0 >> 16) as u16)?;
            Ok(())
        }
    }

    pub(crate) fn push_no_operand(&mut self, op: u8) -> Result<(), AllocError> {
        self.code.push((op as u16) << 8)
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

            self.code.push((op as u16) << 8 | idx_first)?;

            if idx > 1 << 7 {
                self.code.push((idx - (idx_first as u32)) as u16)?;
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
            self.code.push((op as u16) << 8 | (m.align as u16))?;
            self.code.push((m.offset & 0xFFFF) as u16)?;
            self.code.push(((m.offset >> 16) & 0xFFFF) as u16)?;
            Ok(())
        }
    }

    /// Push an operation with a 7-bit or 32-bit (fixed size) operand
    pub(crate) fn push_32(&mut self, op: u8, i: u32) -> Result<(), AllocError> {
        if i < 0xFF {
            let first = ((op as u16) << 8) | i as u16;
            self.code.push(first)?;
        } else {
            self.code.push(((op as u16) << 8) | 0xFF)?;
            self.code.push(i as u16)?;
            self.code.push((i >> 16) as u16)?;
        }

        Ok(())
    }

    /// Push an operation with a 7-bit or 64-bit operand
    pub(crate) fn push_64(&mut self, op: u8, i: u64) -> Result<(), AllocError> {
        if i < 0xFF {
            let first = ((op as u16) << 8) | i as u16;
            self.code.push(first)?;
        } else {
            self.code.push(((op as u16) << 8) | 0xFF)?;
            self.code.push(i as u16)?;
            self.code.push((i >> 16) as u16)?;
            self.code.push((i >> 32) as u16)?;
            self.code.push((i >> 48) as u16)?;
        }

        Ok(())
    }
}
