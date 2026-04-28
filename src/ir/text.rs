use crate::*;

/// # SpaceWASM IR Encoding
///
/// This module implements the compiled intermediate representation (IR) for SpaceWASM.
/// The IR is organized into pages of 16-bit words to support streaming execution.
///
/// ## Memory Layout
/// - Code is stored in 256-word pages (512 bytes each)
/// - Each instruction is 1-5 words depending on operand size
/// - Pages are allocated dynamically as code is compiled
///
/// ## Instruction Format
/// All instructions use 16-bit words with the opcode in the upper 8 bits:
/// ```text
/// [opcode:8][operand:8]
/// ```
///
/// ## Operand Encoding
/// - **No operand**: `[opcode:8][0x00:8]`
/// - **8-bit or 16-bit index**: `[opcode:8][idx:8]` for 0-254, or `[opcode:8][0xFF]` `[idx:16]` for larger values
/// - **8-bit inline**: `[opcode:8][value:8]` - for values 0-254
/// - **32-bit extended**: `[opcode:8][0xFF]` `[lo:16]` `[hi:16]`
/// - **64-bit extended**: `[opcode:8][0xFF]` `[w0:16]` `[w1:16]` `[w2:16]` `[w3:16]`
/// - **Memory arg**: `[opcode:8][align:8]` `[offset_lo:16]` `[offset_hi:16]`
/// - **Jump target**: 2 words encoding a 32-bit address

/// A page of compiled IR code containing 256 16-bit words (512 bytes).
pub struct TextPage(pub [u16; 256]);
impl Default for TextPage {
    fn default() -> TextPage {
        TextPage([0; 256])
    }
}

/// A 32-bit address into the paged IR code.
///
/// Format: `[reserved:2][page_index:22][offset:8]`
/// - `page_index`: which page (0-4M pages supported)
/// - `offset`: word index within the page (0-255)
#[derive(PartialEq, Eq, Clone, Copy)]
pub struct JumpTarget(pub u32);

impl core::ops::Add<u32> for JumpTarget {
    type Output = JumpTarget;
    fn add(self, rhs: u32) -> JumpTarget {
        JumpTarget(self.0 + rhs)
    }
}

impl JumpTarget {
    /// Sentinel value used to mark the end of a linked list of jump addresses.
    /// Uses 0x3FFF_FFFF (all 1s in the 30-bit address space) so it never conflicts
    /// with the control bits (bits 30-31) or valid addresses (24-bit page index).
    const SENTINEL: JumpTarget = JumpTarget(0x3FFF_FFFFu32);

    /// Extract the page index (upper 24 bits).
    pub fn page(&self) -> usize {
        (self.0 >> 8) as usize
    }

    /// Extract the word offset within the page (lower 8 bits).
    pub fn offset(&self) -> usize {
        (self.0 & 0xFF) as usize
    }
}

/// A control flow frame tracking jump targets for blocks, loops, and if-else statements.
///
/// Bit layout in the JumpTarget:
/// - Bit 31 (MSB): forward (0) vs backward (1) jump
/// - Bit 30: is_if flag (1 if this is an if block)
/// - Bits 0-29: jump target address
///
/// For forward jumps, the address field maintains a linked list of all locations
/// that need to be back-patched when the block ends.
#[derive(Clone, Copy)]
struct ControlFrame(JumpTarget);

impl ControlFrame {
    /// Create a forward control frame (block statement).
    fn forward(jt: JumpTarget) -> ControlFrame {
        assert!(jt.0 < 0x4000_0000); // 30-bit address space
        ControlFrame(jt)
    }

    /// Create a forward control frame for an if statement.
    fn forward_if(jt: JumpTarget) -> ControlFrame {
        assert!(jt.0 < 0x4000_0000); // 30-bit address space
        ControlFrame(JumpTarget(jt.0 | 0x4000_0000)) // Set is_if bit
    }

    /// Create a backward control frame (loop statement).
    ///
    /// The jump target is already known (the start of the loop), so no back-patching
    /// is needed. Sets the MSB to mark this as a backward jump.
    fn backward(jt: JumpTarget) -> ControlFrame {
        assert!(jt.0 < 0x4000_0000); // 30-bit address space
        ControlFrame(JumpTarget(jt.0 | 0x8000_0000))
    }

    /// Check if this is a forward control frame (needs back-patching).
    fn is_forward(&self) -> bool {
        (self.0.0 & 0x8000_0000) == 0
    }

    /// Check if this is an if block.
    fn is_if(&self) -> bool {
        (self.0.0 & 0x4000_0000) != 0
    }

    /// Get the jump target address, stripping the control bits.
    fn address(&self) -> JumpTarget {
        JumpTarget(self.0.0 & 0x3FFF_FFFF)
    }

    /// Update the address for a forward control frame (used to maintain the linked list).
    ///
    /// Preserves the control bits while updating the address.
    fn set_address(&mut self, addr: JumpTarget) {
        assert!(self.is_forward()); // Cannot set address on backward branch
        assert!(addr.0 < 0x4000_0000); // Address exceeds 30-bit limit
        self.0.0 = addr.0 | (self.0.0 & 0xC000_0000); // Preserve both control bits
    }
}

/// Low-level builder for paged IR code.
///
/// Manages allocation of pages and writing 16-bit words to the current position.
/// The `N` generic parameter limits the maximum number of pages that can be allocated.
pub struct CodeBuilder<const N: usize> {
    pages: StaticVec<Box<TextPage>, N>,
    offset: usize,
}

impl<const N: usize> CodeBuilder<N> {
    pub fn new() -> CodeBuilder<N> {
        CodeBuilder {
            pages: Default::default(),
            offset: 0,
        }
    }

    pub fn finish(self) -> Result<(Vec<Box<TextPage>>, usize), AllocError> {
        let mut v = Vec::new(self.pages.len() as u32)?;
        for i in self.pages {
            v.push(i);
        }

        Ok((v, self.offset))
    }

    /// Get the current program counter (address of the next word to be written)
    pub fn pc(&self) -> JumpTarget {
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
        assert!(page_index < (1 << 24));
        assert!(offset < 256);

        JumpTarget(((page_index as u32) << 8) | (offset as u32))
    }

    /// Allocate a new page and reset the offset to the start of that page.
    fn add_page(&mut self) -> Result<(), AllocError> {
        self.pages.push(Box::new(TextPage::default())?)?;
        self.offset = 0;
        Ok(())
    }

    /// Write a 16-bit word to the current position and advance the program counter.
    ///
    /// Automatically allocates a new page if we've reached the end of the current one.
    fn push(&mut self, c: u16) -> Result<(), AllocError> {
        if self.offset == 0 && self.pages.len() == 0 {
            // No pages have been allocated yet
            self.add_page()?;
        } else if self.offset >= 256 {
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

pub enum TextContext<'f> {
    Constant,
    Function(&'f Func),
}

/// High-level builder for compiled IR that handles control flow and instruction encoding.
///
/// This builder manages:
/// - Emitting instructions with various operand formats
/// - Tracking control flow (blocks, loops, if-else)
/// - Back-patching forward jump targets
/// - Managing a stack of control frames for nested blocks
///
/// The `N` generic parameter limits the maximum number of code pages.
pub struct TextBuilder<'module, 'ctx, const N: usize> {
    code: &'module mut CodeBuilder<N>,
    module: &'module Module<'module>,
    ctx: TextContext<'ctx>,
    control_frames: StaticVec<ControlFrame, 64>,
    else_frames: StaticVec<JumpTarget, 64>,
}

impl<'module, 'ctx, const N: usize> TextBuilder<'module, 'ctx, N> {
    pub fn new(
        code: &'module mut CodeBuilder<N>,
        module: &'module Module,
        ctx: TextContext<'ctx>,
    ) -> TextBuilder<'module, 'ctx, N> {
        TextBuilder {
            code,
            module,
            ctx,
            control_frames: Default::default(),
            else_frames: Default::default(),
        }
    }

    /// Compute the offset in 32-bit words of a local variable given its index
    pub fn get_local(&self, x: LocalIdx) -> Result<LocalVariable, ValidationError> {
        let TextContext::Function(func) = self.ctx else {
            return Err(ValidationError::InstructionOutsideOfFunction);
        };

        let signature = self
            .module
            .types
            .get(func.ty.0 as usize)
            .ok_or(ValidationError::TypeIdxOutOfRange)?;

        // Search for the variable and compute it's offset
        let mut current_offset = 0;
        let mut current_index = 0;

        let params = &signature.params[..];
        let locals = &func.locals[..];

        // Check the parameters first
        // Frame offsets are negative
        for (i, p_ty) in params.iter().enumerate() {
            if x.0 == i as u32 {
                return Ok(LocalVariable {
                    frame_offset: (current_offset / 4) as i32,
                    ty: *p_ty,
                });
            }

            current_offset += p_ty.size();
            current_index += 1;
        }

        // Skip over the fp/lr on the stack
        current_offset += 8;

        // Now check the local variables
        for (n, ty) in locals {
            if current_index + n > x.0 {
                // This bucket has the local variable
                // Compute it's offset as a word index from the frame
                let offset = current_offset + ty.size() * (x.0 - current_index) as usize;
                return Ok(LocalVariable {
                    frame_offset: (offset / 4) as i32,
                    ty: *ty,
                });
            }

            let section_size = *n as usize * ty.size();
            current_offset += section_size;
            current_index += n;
        }

        // No more locals
        Err(ValidationError::LocalIdxOutOfRange)
    }

    /// Look up a global variable given its index
    /// If we are computing a constant expression, globals can only refer to imported globals
    pub fn get_global(&self, x: GlobalIdx) -> Result<GlobalVariable, ValidationError> {
        let imported_idx = self
            .module
            .imports
            .iter()
            .filter_map(|i| match &i {
                Import::Global(g) => Some(*g),
                _ => None,
            })
            .skip(x.0 as usize)
            .next();

        match self.ctx {
            // Constant global references must be to imported values
            TextContext::Constant => {
                let idx = imported_idx.ok_or(ValidationError::GlobalIdxOutOfRange)?;
                let g = self
                    .module
                    .module_imports
                    .globals
                    .get(idx as usize)
                    .unwrap();

                Ok(GlobalVariable {
                    reference: GlobalVariableRef::Imported(idx as u32),
                    ty: g.value.ty(),
                    mutable: g.value.mutable(),
                })
            }

            TextContext::Function(_) => match imported_idx {
                // This index is one of the WASM defined globals
                None => {
                    let idx = x.0 as usize - self.module.module_imports.globals.len();
                    let g = self
                        .module
                        .globals
                        .get(idx)
                        .ok_or(ValidationError::GlobalIdxOutOfRange)?;

                    Ok(GlobalVariable {
                        reference: GlobalVariableRef::Internal(idx as u32),
                        ty: g.type_.ty,
                        mutable: g.type_.mutable,
                    })
                }
                // This index refers to an imported global
                Some(idx) => {
                    let g = self
                        .module
                        .module_imports
                        .globals
                        .get(idx as usize)
                        .unwrap();

                    Ok(GlobalVariable {
                        reference: GlobalVariableRef::Imported(idx as u32),
                        ty: g.value.ty(),
                        mutable: g.value.mutable(),
                    })
                }
            },
        }
    }

    pub fn pc(&self) -> JumpTarget {
        self.code.pc()
    }

    /// Enter a forward control block (block statement).
    pub(crate) fn enter_forward_block(&mut self) -> Result<(), AllocError> {
        self.control_frames
            .push(ControlFrame::forward(JumpTarget::SENTINEL).into())
    }

    /// Enter a forward control block for an if statement.
    ///
    /// This is tracked separately to handle if-without-else cases properly.
    pub(crate) fn enter_forward_if_block(&mut self) -> Result<(), AllocError> {
        self.control_frames
            .push(ControlFrame::forward_if(JumpTarget::SENTINEL).into())
    }

    /// Enter a backward control block (loop statement).
    pub(crate) fn enter_backward_block(&mut self) -> Result<(), AllocError> {
        self.control_frames
            .push(ControlFrame::backward(self.code.pc()).into())
    }

    /// Exit the current control block and back-patch any forward jump targets.
    ///
    /// For forward blocks (block/if):
    /// - Walks the linked list of jump locations that need the exit address
    /// - Each jump location stores the address of the next jump in the list
    /// - Overwrites each jump with the actual target (current PC)
    ///
    /// For backward blocks (loop):
    /// - No back-patching needed since jumps already knew the target
    ///
    /// The linked list structure:
    /// ```text
    /// control_frame.address -> [addr1] -> [addr2] -> ... -> SENTINEL
    /// ```
    /// Each location in brackets holds 2 words encoding the next address.
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

            // Handle if-without-else case: if this is an if block and else_frames has an entry,
            // it means no else_() was called, so we need to back-patch the IF sentinel here
            if last.is_if() && !self.else_frames.is_empty() {
                let Some(else_addr) = self.else_frames.pop() else {
                    return Err(ValidationError::InvalidElseBlock);
                };
                // Back-patch the IF instruction's jump target to point to end of block (current PC)
                self.code.write(else_addr, pc.0 as u16)?;
                self.code.write(else_addr + 1, (pc.0 >> 16) as u16)?;
            }
        } else {
            // We don't need to do any back-patching here
            // Backward control frames already knew their start address
        }

        Ok(())
    }

    /// Mark the start of an else block for an if statement.
    ///
    /// Emits placeholder words (0x3FFF_FFFF sentinel) that will be back-patched
    /// with the else branch target address when `finish_else` is called.
    /// The current PC is saved so we know where to back-patch.
    pub(crate) fn start_else(&mut self) -> Result<(), AllocError> {
        self.else_frames.push(self.code.pc())?;

        // Write the sentinel placeholder (will be back-patched with else target)
        self.code.push(0xFFFF)?;
        self.code.push(0x3FFF)?;
        Ok(())
    }

    /// Finish the else block by back-patching the else branch target.
    ///
    /// Overwrites the placeholder emitted by `start_else` with the current PC
    /// (the start of the else block).
    pub(crate) fn finish_else(&mut self) -> Result<(), ValidationError> {
        let Some(frame) = self.else_frames.pop() else {
            return Err(ValidationError::InvalidElseBlock);
        };

        let pc = self.code.pc();
        self.code.write(frame, pc.0 as u16)?;
        self.code.write(frame + 1, (pc.0 >> 16) as u16)?;
        Ok(())
    }

    /// Emit a jump to the specified label (control frame).
    ///
    /// Labels are indexed relative to the top of the control frame stack:
    /// - LabelIdx(0) = innermost block
    /// - LabelIdx(1) = next outer block, etc.
    ///
    /// For forward jumps (block/if):
    /// - Adds this location to the linked list of jumps to back-patch
    /// - Emits the old head of the list as placeholder
    /// - Updates the frame's head to point to this location
    ///
    /// For backward jumps (loop):
    /// - The target is already known, emit it directly
    ///
    /// Jump targets are encoded as 2 words (32-bit address).
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

    /// Emit an instruction with no operand.
    ///
    /// Format: `[opcode:8][0x00:8]`
    pub(crate) fn push_no_operand(&mut self, op: u8) -> Result<(), AllocError> {
        self.code.push((op as u16) << 8)
    }

    pub(crate) fn push_local(&mut self, op: u8, l: LocalVariable) -> Result<(), ValidationError> {
        self.code
            .push(((op as u16) << 8) | (l.ty.size() as u8 as u16))?;

        if l.frame_offset > 0xFFFF {
            Err(ValidationError::IdxTooLarge)
        } else {
            self.code.push(l.frame_offset as u16)?;
            Ok(())
        }
    }

    pub(crate) fn push_global(&mut self, op: u8, g: GlobalVariable) -> Result<(), ValidationError> {
        let ty_enc = g.ty as u8;
        let (index_ty, idx) = match &g.reference {
            GlobalVariableRef::Imported(i) => (1u8 << 7, *i),
            GlobalVariableRef::Internal(i) => (0u8, *i),
        };

        self.code
            .push(((op as u16) << 8) | index_ty as u16 | ty_enc as u16)?;

        if idx > 0xFFFF {
            Err(ValidationError::IdxTooLarge)
        } else {
            self.code.push(idx as u16)?;
            Ok(())
        }
    }

    /// Emit an instruction with an 8-bit or 16-bit index operand.
    ///
    /// Encoding:
    /// - If the index is between 0-254, the index will be encoded in 8-bits
    /// - Otherwise: `[opcode:8][255:8]` `[index:16]` (2 words)
    pub(crate) fn push_8_or_16(&mut self, op: u8, idx: u32) -> Result<(), ValidationError> {
        // We support up to 16-bit indexes
        if idx > 0xFFFF {
            Err(ValidationError::IdxTooLarge)
        } else if idx < 0xFF {
            // Encode in a single 16-bit word
            self.code.push((op as u16) << 8 | (idx as u16))?;
            Ok(())
        } else {
            self.code.push((op as u16) << 8 | 0xFF)?;
            self.code.push(idx as u16)?;
            Ok(())
        }
    }

    /// Emit a memory instruction with alignment and offset operands.
    ///
    /// Format: `[opcode:8][align:8]` `[offset_lo:16]` `[offset_hi:16]` (3 words)
    ///
    /// Memory instructions (load/store) use this encoding to specify:
    /// - `align`: alignment exponent (must fit in 8 bits)
    /// - `offset`: 32-bit memory offset
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

    /// Push an operation with an 8-bit inline or 32-bit extended operand.
    ///
    /// Encoding:
    /// - Values 0-254: encoded inline in lower 8 bits of first word
    /// - Values 255+: 0xFF sentinel in first word, followed by full 32-bit value in next 2 words
    pub(crate) fn push_8_or_32(&mut self, op: u8, i: u32) -> Result<(), AllocError> {
        if i < 0xFF {
            // Inline encoding: operand fits in 8 bits (0-254)
            let first = ((op as u16) << 8) | i as u16;
            self.code.push(first)?;
        } else {
            // Extended encoding: 0xFF is sentinel, read full 32-bit value from next 2 words
            self.code.push(((op as u16) << 8) | 0xFF)?;
            self.code.push(i as u16)?;
            self.code.push((i >> 16) as u16)?;
        }

        Ok(())
    }

    /// Push an operation with an 8-bit inline or 64-bit extended operand.
    ///
    /// Encoding:
    /// - Values 0-254: encoded inline in lower 8 bits of first word
    /// - Values 255+: 0xFF sentinel in first word, followed by full 64-bit value in next 4 words
    pub(crate) fn push_8_or_64(&mut self, op: u8, i: u64) -> Result<(), AllocError> {
        if i < 0xFF {
            // Inline encoding: operand fits in 8 bits (0-254)
            let first = ((op as u16) << 8) | i as u16;
            self.code.push(first)?;
        } else {
            // Extended encoding: 0xFF is sentinel, read full 64-bit value from next 4 words
            self.code.push(((op as u16) << 8) | 0xFF)?;
            self.code.push(i as u16)?;
            self.code.push((i >> 16) as u16)?;
            self.code.push((i >> 32) as u16)?;
            self.code.push((i >> 48) as u16)?;
        }

        Ok(())
    }
}
