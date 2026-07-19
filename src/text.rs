//! # SpaceWasm IR Encoding
//!
//! This module implements the compiled intermediate representation (IR) for SpaceWasm.
//! The IR is organized into pages of 16-bit words to support streaming execution.
//!
//! ## Memory Layout
//! - Code is stored in 256-word pages (512 bytes each)
//! - Each instruction is 1-5 words depending on operand size
//! - Pages are allocated dynamically as code is compiled
//!
//! ## Instruction Format
//! All instructions use 16-bit words with the opcode in the upper 8 bits:
//! ```text
//! [opcode:8][operand:8]
//! ```
//!
//! ## Operand Encoding
//! - **No operand**: `[opcode:8][0x00:8]`
//! - **8-bit or 16-bit index**: `[opcode:8][idx:8]` for 0-254, or `[opcode:8][0xFF]` `[idx:16]` for larger values
//! - **8-bit inline**: `[opcode:8][value:8]` - for values 0-254
//! - **32-bit extended**: `[opcode:8][0xFF]` `[lo:16]` `[hi:16]`
//! - **64-bit extended**: `[opcode:8][0xFF]` `[w0:16]` `[w1:16]` `[w2:16]` `[w3:16]`
//! - **Memory arg**: `[opcode:8][align:8]` `[offset_lo:16]` `[offset_hi:16]`
//! - **Jump target**: 2 words encoding a 32-bit address

use crate::*;
use ::core::ops::AddAssign;

/// A page of compiled IR code containing 256 16-bit words (512 bytes).
#[derive(Clone)]
pub struct TextPage(pub [u16; 256]);
impl Default for TextPage {
    fn default() -> TextPage {
        TextPage([0; 256])
    }
}

/// A relative offset to jump to from the current program counter
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub struct JumpOffset(i32);

impl JumpOffset {
    pub fn sentinel() -> JumpOffset {
        JumpOffset(0)
    }

    pub fn offset(n: i32) -> JumpOffset {
        JumpOffset(n)
    }

    pub fn new(current: JumpTarget, to: JumpTarget) -> Result<JumpOffset, ValidationError> {
        if to == JumpTarget::SENTINEL {
            Ok(JumpOffset::sentinel())
        } else {
            let offset = (to.0 as i32) - (current.0 as i32);
            let fits = offset == ((offset << (32 - 22)) >> (32 - 22));
            if !fits {
                Err(ValidationError::LabelJumpTooLarge)
            } else {
                Ok(JumpOffset(offset))
            }
        }
    }
}

/// A 32-bit value encoding a label's arity, jump address, and stack truncation information
/// `[arity:2][depth:8][offset:22]`
///
/// Arity is the result type needed by the target block we are jumping to:
/// arity == 0 (no result)
/// arity == 1 (I32 / F32)
/// arity == 2 (I64 / F64)
/// arity == 3 (unused/invalid)
///
/// Depth is the 32-bit width truncation of the stack to perform during this jump.
///
/// offset is the pc offset (signed) from the current PC. For forward jumps this will be 0
/// by default to mark the 'sentinel' jump target.
///
#[derive(Clone, Copy)]
pub struct LabelTarget(u32);

impl ::core::fmt::Debug for LabelTarget {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LabelTarget")
            .field("arity", &self.arity())
            .field("depth", &self.depth())
            .field("jump", &self.jump())
            .finish()
    }
}

impl From<u32> for LabelTarget {
    fn from(value: u32) -> Self {
        LabelTarget(value)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum LabelArity {
    None = 0,
    I32 = 1,
    I64 = 2,
}

impl LabelTarget {
    fn new(result_type: ResultType, depth: u8, offset: JumpOffset) -> LabelTarget {
        let rt = match result_type.0 {
            None => 0,
            Some(ValType::I32 | ValType::F32) => 1,
            Some(ValType::I64 | ValType::F64) => 2,
        };

        LabelTarget((rt << 30) | ((depth as u32) << 22) | (offset.0 as u32) & 0x3FFFFF)
    }

    fn early_return(result_type: ResultType) -> LabelTarget {
        Self::new(result_type, 0, JumpOffset::sentinel())
    }

    pub fn with_jump(self, offset: JumpOffset) -> LabelTarget {
        LabelTarget(
            self.0 & 0xFFC00000 // First 10 bits
                | ((offset.0 as u32) & 0x3FFFFF), // Final 22 bits
        )
    }

    /// Extract the jump target from the encoded label target
    pub fn jump(&self) -> JumpOffset {
        // The offset is 22-bits
        let u = self.0 & 0x3FFFFF;

        // Grab the sign bit and move extend it
        JumpOffset(((u << 10) as i32) >> 10)
    }

    pub fn is_sentinel(&self) -> bool {
        self.jump() == JumpOffset(0)
    }

    /// Number of 32-bit values to move from the current stack to the result stack
    pub fn arity(&self) -> LabelArity {
        match (self.0 >> 30) & 0b11 {
            0 => LabelArity::None,
            1 => LabelArity::I32,
            2 => LabelArity::I64,
            _ => unreachable!(),
        }
    }

    /// Number of 32-bit values to drop from the stack
    pub fn depth(&self) -> u8 {
        ((self.0 >> 22) & 0xFF) as u8
    }
}

/// A 32-bit address into the paged IR code.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub struct JumpTarget(pub u32);

impl core::ops::Add<JumpOffset> for JumpTarget {
    type Output = JumpTarget;
    fn add(self, rhs: JumpOffset) -> JumpTarget {
        JumpTarget(((self.0 as i32) + rhs.0) as u32)
    }
}

impl core::ops::Add<u32> for JumpTarget {
    type Output = JumpTarget;
    fn add(self, rhs: u32) -> JumpTarget {
        JumpTarget(self.0 + rhs)
    }
}

impl AddAssign<JumpOffset> for JumpTarget {
    fn add_assign(&mut self, rhs: JumpOffset) {
        let a = (self.0 as i32) + rhs.0;
        self.0 = a as u32;
    }
}

impl JumpTarget {
    /// Sentinel value used to mark the end of a linked list of jump addresses.
    pub const SENTINEL: JumpTarget = JumpTarget(0xFFFF_FFFFu32);

    /// Extract the page index (upper 24 bits).
    pub fn page(&self) -> usize {
        (self.0 >> 8) as usize
    }

    /// Extract the word offset within the page (lower 8 bits).
    pub fn offset(&self) -> usize {
        (self.0 & 0xFF) as usize
    }
}

#[derive(Clone, Copy)]
pub enum BlockKind {
    Loop,  // Loops
    Block, // Block, Else
    If,
}

/// A control flow frame tracking jump targets for blocks, loops, and if-else statements.
#[derive(Clone)]
struct ControlFrame {
    kind: BlockKind,
    label: ResultType,
    out: ResultType,
    /// The operand stack height in _number_ of elements rather than stack word usage
    height: u16,
    /// For loops: the backward jump target
    /// For blocks/if/else: the head of the linked list of forward jumps
    /// For if: the false-branch placeholder is at the TAIL of this list
    target: JumpTarget,
    unreachable: bool,
}

/// The decoded representation of a global variable
#[derive(Debug)]
pub struct GlobalVariable {
    // The index of the global variable
    pub reference: Ref,
    pub ty: ValType,
    pub mutable: bool,
}

/// Low-level builder for paged IR code.
///
/// Manages allocation of pages and writing 16-bit words to the current position.
/// The `N` generic parameter limits the maximum number of pages that can be allocated.
#[derive(Clone)]
pub struct CodeBuilder<const MAX_CODE_PAGES: usize> {
    pages: StaticVec<Box<TextPage>, MAX_CODE_PAGES>,
    offset: usize,
}

impl<const MAX_CODE_PAGES: usize> Default for CodeBuilder<MAX_CODE_PAGES> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const MAX_CODE_PAGES: usize> CodeBuilder<MAX_CODE_PAGES> {
    pub fn new() -> CodeBuilder<MAX_CODE_PAGES> {
        const {
            assert!(
                MAX_CODE_PAGES < (1 << 24),
                "SpaceWasm supports up to 24-bit code pages"
            );
        }

        CodeBuilder {
            pages: Default::default(),
            offset: 0,
        }
    }

    fn backpatch(
        &mut self,
        start: JumpTarget,
        mut f: impl FnMut(&mut Self, JumpTarget, LabelTarget) -> Result<(), ValidationError>,
    ) -> Result<(), ValidationError> {
        if start == JumpTarget::SENTINEL {
            return Ok(());
        }

        let mut next = start;

        // Bound our loops to not go into an infinite cycle
        let mut n = 0;

        loop {
            let address = next;

            let lt = LabelTarget(self.read_32(address)?);
            f(self, address, lt)?;

            if lt.is_sentinel() {
                break;
            }

            next = address + lt.jump();

            n += 1;
            if n > 0xFFFF {
                return Err(ValidationError::PossibleBackpatchCycle);
            }
        }

        Ok(())
    }

    /// Move all the text pages into a heap vector with the used size.
    /// This consumes the builder and returns the pages and the used size.
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
            } else if self.pages.is_empty() {
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
        if self.offset == 0 && self.pages.is_empty() {
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

    /// Write two IR words at a given address from a single 32-bit value
    fn write_32(&mut self, address: JumpTarget, value: u32) -> Result<(), ValidationError> {
        self.write(address, value as u16)?;
        self.write(address + 1, (value >> 16) as u16)?;
        Ok(())
    }

    /// Read a single IR word at a given address.
    fn read(&self, address: JumpTarget) -> Result<u16, ValidationError> {
        let page_index = address.page();
        let offset = address.offset();

        if page_index >= self.pages.len() || offset >= 256 {
            Err(ValidationError::PageFault)
        } else {
            Ok(self.pages[page_index].0[offset])
        }
    }

    /// Read two IR words at a given address into a single 32-bit value
    fn read_32(&self, address: JumpTarget) -> Result<u32, ValidationError> {
        let w1 = self.read(address)?;
        let w2 = self.read(address + 1)?;

        Ok((w1 as u32) | ((w2 as u32) << 16))
    }
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
pub struct TextBuilder<
    'a,
    const MAX_CODE_PAGES: usize,
    const MAX_CONTROL_FRAMES: usize,
    const MAX_STACK_DEPTH: usize,
> {
    code: &'a mut CodeBuilder<MAX_CODE_PAGES>,
    store: &'a Store,
    module: &'a Module,
    func: &'a Func,
    control_frames: StaticVec<ControlFrame, MAX_CONTROL_FRAMES>,
    value_stack: StaticVec<OperandType, MAX_STACK_DEPTH>,
    stack_highwater: usize,
    br_table_result: Option<ResultType>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum OperandType {
    Unknown,
    Known(ValType),
}

impl From<ValType> for OperandType {
    fn from(value: ValType) -> Self {
        OperandType::Known(value)
    }
}

impl From<&ValType> for OperandType {
    fn from(value: &ValType) -> Self {
        OperandType::Known(*value)
    }
}

impl From<OperandType> for ValType {
    fn from(value: OperandType) -> Self {
        match value {
            OperandType::Unknown => ValType::I32,
            OperandType::Known(t) => t,
        }
    }
}

impl From<OperandType> for u8 {
    fn from(value: OperandType) -> Self {
        let vty: ValType = value.into();
        vty as u8
    }
}

impl<'a, const MAX_CODE_PAGES: usize, const MAX_CONTROL_FRAMES: usize, const MAX_STACK_DEPTH: usize>
    TextBuilder<'a, MAX_CODE_PAGES, MAX_CONTROL_FRAMES, MAX_STACK_DEPTH>
{
    pub fn new(
        code: &'a mut CodeBuilder<MAX_CODE_PAGES>,
        store: &'a Store,
        module: &'a Module,
        func: &'a Func,
    ) -> Self {
        let mut builder = TextBuilder {
            code,
            store,
            module,
            func,
            control_frames: Default::default(),
            value_stack: Default::default(),
            stack_highwater: 0,
            br_table_result: None,
        };

        // Push the implicit function control frame per the spec
        // This represents the function's return point
        let return_type = ResultType(func.return_ty);
        let _ = builder.push_control(BlockKind::Block, return_type, return_type);

        builder
    }

    pub fn store(&self) -> &'a Store {
        self.store
    }

    pub fn func(&self) -> &'a Func {
        self.func
    }

    pub fn module(&self) -> &'a Module {
        self.module
    }

    pub fn stack_usage(&self) -> usize {
        self.stack_highwater
    }

    pub fn check_br_table_result(&mut self, r: ResultType) -> Result<(), ValidationError> {
        if let Some(other) = self.br_table_result {
            if other != r {
                Err(ValidationError::BlockResultTypeMismatch)
            } else {
                Ok(())
            }
        } else {
            self.br_table_result = Some(r);
            Ok(())
        }
    }

    pub fn check_and_clear_br_table_result(
        &mut self,
        def_result: ResultType,
    ) -> Result<(), ValidationError> {
        if let Some(other) = self.br_table_result {
            if other != def_result {
                Err(ValidationError::BlockResultTypeMismatch)
            } else {
                self.br_table_result = None;
                Ok(())
            }
        } else {
            Ok(())
        }
    }

    /// Compute the offset in 32-bit words of a local variable given its index
    pub fn get_local(&self, x: LocalIdx) -> Result<LocalVariable, ValidationError> {
        if x.0 > 0xFFFF {
            return Err(ValidationError::LocalIdxOutOfRange);
        }

        let x = x.0 as u16;

        let signature = self
            .module
            .types
            .get(self.func.ty.0 as usize)
            .ok_or(ValidationError::TypeIdxOutOfRange)?;

        // Search for the variable and compute it's offset
        let mut current_offset = 0usize;
        let mut current_index = 0;

        // Check the parameters first
        for (i, p_ty) in signature.params.iter().enumerate() {
            if x == i as u16 {
                // This offset is the offset from the start of the parameters list
                // We need to convert this offset to be relative to the frame pointer
                // which is immediately after the final parameter
                let frame_offset =
                    (((current_offset / 4) as i32) - (self.func.parameter_size as i32)) as i16;

                return Ok(LocalVariable {
                    frame_offset,
                    ty: *p_ty,
                });
            }

            current_offset += p_ty.size();
            current_index += 1;
        }

        current_offset = 0;

        // Now check the local variables
        for (n, ty) in &self.func.locals {
            if current_index + n > x {
                // This bucket has the local variable
                // Compute it's offset as a word index from the frame
                let offset = current_offset + ty.size() * (x - current_index) as usize;
                return Ok(LocalVariable {
                    // Add 2 to skip over fp and lr
                    frame_offset: ((offset / 4) as i16) + 2,
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
        let reference = self
            .module
            .get_global_ref(x)
            .ok_or(ValidationError::GlobalIdxOutOfRange)?;

        match reference {
            // This index is one of the Wasm defined globals
            Ref::Module(idx) => {
                let g = self
                    .module
                    .globals
                    .get(idx as usize)
                    .ok_or(ValidationError::GlobalIdxOutOfRange)?;

                Ok(GlobalVariable {
                    reference,
                    ty: g.type_.ty,
                    mutable: g.type_.mutable,
                })
            }
            // This index refers to an imported global
            Ref::Host { module, index } => {
                // Unwrap() should be fine since the imports are already resolved
                let module = self.store.host_modules().get(module.0 as usize).unwrap();
                let global = module.globals.get(index as usize).unwrap();

                Ok(GlobalVariable {
                    reference,
                    ty: global.value.ty(),
                    mutable: global.value.mutable(),
                })
            }
            Ref::Extern { module, index } => {
                let module = self.store.modules().get(module.0 as usize).unwrap();
                let global = module.globals.get(index as usize).unwrap();

                Ok(GlobalVariable {
                    reference,
                    ty: global.type_.ty,
                    mutable: global.type_.mutable,
                })
            }
        }
    }

    pub fn pc(&self) -> JumpTarget {
        self.code.pc()
    }

    pub(crate) fn mark_unreachable(&mut self) {
        // With the implicit function frame, there's always a frame
        if let Some(frame) = self.control_frames.last_mut() {
            self.value_stack.truncate(frame.height as usize);
            frame.unreachable = true;
        }
    }

    fn is_unreachable(&self) -> bool {
        // With the implicit function frame, there's always a frame
        self.control_frames
            .last()
            .map(|c| c.unreachable)
            .unwrap_or(false)
    }

    fn control_frame_stack_len(&self) -> usize {
        let height = self
            .control_frames
            .last()
            .map(|c| c.height as usize)
            .unwrap_or(0);
        self.value_stack.len() - height
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
    /// Label targets also encode the arity and stack reset information in the first 10-bits.
    /// The jump address is encoded in the final 22-bits
    pub(crate) fn write_label_target(
        &mut self,
        label: LabelIdx,
    ) -> Result<ResultType, ValidationError> {
        if label.0 as usize >= self.control_frames.len() {
            return Err(ValidationError::InvalidLabelIndex);
        }

        let idx = self.control_frames.len() - 1 - label.0 as usize;

        // Check if we're branching to the function frame (index 0)
        // In that case, generate an early return instead of a forward jump
        let result = if idx == 0 {
            let lt = LabelTarget::early_return(ResultType(self.func.return_ty));
            self.code.push(lt.0 as u16)?;
            self.code.push((lt.0 >> 16) as u16)?;

            ResultType(self.func.return_ty)
        } else {
            let pc = self.pc();
            let start_stack_height = self.stack_height();
            let end_stack_height = self.stack_height_for(self.control_frames[idx].height as usize);
            let stack_delta = start_stack_height - end_stack_height;

            if stack_delta > u8::MAX as usize {
                return Err(ValidationError::LabelStackJumpTooDeep);
            }

            let control_frame = &mut self.control_frames[idx];
            match control_frame.kind {
                // Backward jump target
                BlockKind::Loop => {
                    let lt = LabelTarget::new(
                        // Loops do not produce a value on every loop
                        ResultType(None),
                        stack_delta as u8,
                        JumpOffset::new(pc, control_frame.target)?,
                    );

                    self.code.push(lt.0 as u16)?;
                    self.code.push((lt.0 >> 16) as u16)?;
                    ResultType(None)
                }
                // Forward jump targets
                BlockKind::Block | BlockKind::If => {
                    // Forward jump targets use a linked-list of addresses to denote their back-patching
                    // We write the last head here and update the head to our address
                    let target = control_frame.target;
                    control_frame.target = self.code.pc();

                    let lt = LabelTarget::new(
                        control_frame.out,
                        (stack_delta & 0xFF) as u8,
                        JumpOffset::new(pc, target)?,
                    );

                    self.code.push(lt.0 as u16)?;
                    self.code.push((lt.0 >> 16) as u16)?;

                    control_frame.label
                }
            }
        };

        Ok(result)
    }

    pub(crate) fn stack_height(&self) -> usize {
        let mut size: usize = 0;
        for i in self.value_stack.iter() {
            size += match i {
                OperandType::Unknown => break,
                OperandType::Known(ValType::I32) => 1,
                OperandType::Known(ValType::I64) => 2,
                OperandType::Known(ValType::F32) => 1,
                OperandType::Known(ValType::F64) => 2,
            }
        }

        size
    }

    pub(crate) fn stack_height_for(&self, truncate_len: usize) -> usize {
        assert!(truncate_len <= self.value_stack.len());

        let mut size: usize = 0;
        for i in self.value_stack[0..truncate_len].iter() {
            size += match i {
                OperandType::Unknown => break,
                OperandType::Known(ValType::I32) => 1,
                OperandType::Known(ValType::I64) => 2,
                OperandType::Known(ValType::F32) => 1,
                OperandType::Known(ValType::F64) => 2,
            }
        }

        size
    }

    pub(crate) fn push_stack(&mut self, ty: impl Into<OperandType>) -> Result<(), ValidationError> {
        self.value_stack.push(ty.into())?;
        let l = self.stack_height();
        if l > self.stack_highwater {
            self.stack_highwater = l;
        }

        Ok(())
    }

    pub(crate) fn pop_stack_t(&mut self) -> Result<OperandType, ValidationError> {
        // func pop_opd() : val_type | Unknown =
        //     if (opds.size() = ctrls[0].height && ctrls[0].unreachable) return Unknown
        // error_if(opds.size() = ctrls[0].height)
        // return opds.pop()

        if self.control_frame_stack_len() == 0 && self.is_unreachable() {
            Ok(OperandType::Unknown)
        } else if self.control_frame_stack_len() == 0 {
            Err(ValidationError::StackUnderflow)
        } else {
            Ok(self
                .value_stack
                .pop()
                .ok_or(ValidationError::StackUnderflow)?)
        }
    }

    pub(crate) fn pop_result_type(&mut self, result: ResultType) -> Result<(), ValidationError> {
        if let Some(result) = result.0 {
            self.pop_stack(result)?;
            Ok(())
        } else {
            Ok(())
        }
    }

    pub(crate) fn push_result_type(&mut self, result: ResultType) -> Result<(), ValidationError> {
        if let Some(result) = result.0 {
            self.push_stack(result)
        } else {
            Ok(())
        }
    }

    pub(crate) fn pop_stack(
        &mut self,
        expect: impl Into<OperandType>,
    ) -> Result<OperandType, ValidationError> {
        // func pop_opd(expect : val_type | Unknown) : val_type | Unknown =
        //      let actual = pop_opd()
        //      if (actual = Unknown) return expect
        //      if (expect = Unknown) return actual
        //      error_if(actual =/= expect)
        //      return actual

        let expect = expect.into();
        let actual = self.pop_stack_t()?;
        if actual == OperandType::Unknown {
            return Ok(expect);
        }

        if expect == OperandType::Unknown {
            return Ok(actual);
        }

        if actual != expect {
            Err(ValidationError::TypeMismatch)
        } else {
            Ok(actual)
        }
    }

    pub(crate) fn push_control(
        &mut self,
        kind: BlockKind,
        label: ResultType,
        out: ResultType,
    ) -> Result<(), ValidationError> {
        // func push_ctrl(label : list(val_type), out : list(val_type)) =
        //     let frame = ctrl_frame(label, out, opds.size(), false)
        //     ctrls.push(frame)

        let frame = ControlFrame {
            kind,
            label,
            out,
            height: self.value_stack.len() as u16,
            target: JumpTarget::SENTINEL,
            unreachable: false,
        };

        self.control_frames.push(frame)?;
        Ok(())
    }

    /// Set the target (br chain head) of the current control frame.
    /// Used when entering an else block to transfer the br chain from the if block.
    pub(crate) fn set_control_target(&mut self, target: JumpTarget) -> Result<(), ValidationError> {
        let frame = self
            .control_frames
            .last_mut()
            .ok_or(ValidationError::InvalidEndBlock)?;
        frame.target = target;
        Ok(())
    }

    /// Write a placeholder label target for an if's false branch (else or end).
    /// This is called immediately after pushing an if control frame.
    /// The placeholder becomes the TAIL of the linked list (any br instructions will be inserted before it).
    pub(crate) fn write_if_else_target(&mut self) -> Result<(), ValidationError> {
        // Get the result type from the control frame we just pushed
        let out = self
            .control_frames
            .last()
            .ok_or(ValidationError::InvalidEndBlock)?
            .out;

        // Save the current PC - this becomes the initial tail
        let patch_location = self.pc();

        // Write a sentinel placeholder that will be patched later
        let placeholder = LabelTarget::new(out, 0, JumpOffset::sentinel());
        self.code.push(placeholder.0 as u16)?;
        self.code.push((placeholder.0 >> 16) as u16)?;

        // Initialize target to this location - subsequent br instructions will be inserted at the head
        self.control_frames.last_mut().unwrap().target = patch_location;

        Ok(())
    }

    /// Exit an if control block and patch its false-branch to point to the current location.
    /// This is called when entering an else block.
    /// Walks the linked list to find the tail (false-branch with sentinel), patches it,
    /// and breaks it off from the chain. Returns (result_type, remaining_chain_head).
    pub(crate) fn pop_control_and_patch_if(
        &mut self,
    ) -> Result<(ResultType, JumpTarget), ValidationError> {
        // Read validation data from the frame without popping it yet
        let (out, height) = {
            let Some(last) = self.control_frames.last() else {
                return Err(ValidationError::InvalidEndBlock);
            };
            (last.out, last.height)
        };

        // Pop the expected end types while the frame is still on the stack
        self.pop_result_type(out)?;

        // Check that the stack height matches
        if self.value_stack.len() != height as usize {
            return Err(ValidationError::BlockResultTypeMismatch);
        }

        // Pop the control frame
        let last = self.control_frames.pop().unwrap();

        // Walk the linked list to find and patch the tail (false-branch placeholder)
        let pc = self.pc();
        let mut prev = JumpTarget::SENTINEL;
        let mut remaining_chain = JumpTarget::SENTINEL;

        self.code.backpatch(last.target, |code, address, label| {
            if label.is_sentinel() {
                // Found the tail - this is the false-branch placeholder
                // Patch it to jump here (start of else)
                let patched = label.with_jump(JumpOffset::new(address, pc)?);
                code.write_32(address, patched.0)?;

                // Break it off: make the previous node the new tail
                if prev != JumpTarget::SENTINEL {
                    let old_label = LabelTarget(code.read_32(prev)?);
                    let new_tail = old_label.with_jump(JumpOffset::sentinel());
                    code.write_32(prev, new_tail.0)?;
                    // The remaining chain starts at the original head
                    remaining_chain = last.target;
                }
                // If prev is SENTINEL, there were no br instructions, only the false-branch
            } else {
                prev = address;
            }
            Ok(())
        })?;

        Ok((last.out, remaining_chain))
    }

    /// Exit the current control block and back-patch any forward jump targets.
    ///
    /// For forward blocks (block/if/else):
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
    pub(crate) fn pop_control(&mut self) -> Result<ResultType, ValidationError> {
        // func pop_ctrl() : list(val_type) =
        //     error_if(ctrls.is_empty())
        //     let frame = ctrls[0]
        //     pop_opds(frame.end_types)
        //     error_if(opds.size() =/= frame.height)
        //     ctrls.pop()
        //     return frame.end_types

        // Read validation data from the frame without popping it yet
        // (needed for unreachable handling in pop_result_type)
        let (out, height) = {
            let Some(last) = self.control_frames.last() else {
                return Err(ValidationError::InvalidEndBlock);
            };
            (last.out, last.height)
        };

        // Pop the expected end types while the frame is still on the stack
        self.pop_result_type(out)?;

        // Check that the stack height matches
        if self.value_stack.len() != height as usize {
            return Err(ValidationError::BlockResultTypeMismatch);
        }

        // Now pop the control frame and get the actual target value
        // (which may have been modified by write_label_target calls)
        let last = self.control_frames.pop().unwrap();

        // Patch in the forward jump targets with the current PC.
        match last.kind {
            BlockKind::Loop => (),
            BlockKind::Block => {
                let pc = self.pc();
                // Patch all forward jumps (including false-branch if it's an if without else)
                self.code.backpatch(last.target, |code, address, label| {
                    let patched = label.with_jump(JumpOffset::new(address, pc)?);
                    code.write_32(address, patched.0)?;
                    Ok(())
                })?;
            }
            BlockKind::If => {
                // We are currently inside an if-statement without an else.
                // Only if-statements without return values are valid here (or inside an unreachable state).
                if last.out.0.is_none() || last.unreachable {
                    let pc = self.pc();
                    self.code.backpatch(last.target, |code, address, label| {
                        let patched = label.with_jump(JumpOffset::new(address, pc)?);
                        code.write_32(address, patched.0)?;
                        Ok(())
                    })?;
                } else {
                    Err(ValidationError::BlockResultTypeMismatch)?;
                }
            }
        }

        Ok(last.out)
    }

    /// Emit an instruction with no operand.
    ///
    /// Format: `[opcode:8][0x00:8]`
    pub(crate) fn instr(&mut self, op: u8) -> Result<(), AllocError> {
        self.code.push((op as u16) << 8)
    }

    /// Emit an instruction with an 8-bit operand
    pub(crate) fn instr_imm_8(&mut self, op: u8, imm: u8) -> Result<(), AllocError> {
        self.code.push((op as u16) << 8 | (imm as u16))
    }

    /// Emit an instruction with an 8-bit or 16-bit index operand.
    ///
    /// Encoding:
    /// - If the index is between 0-254, the index will be encoded in 8-bits
    /// - Otherwise: `[opcode:8][255:8]` `[index:16]` (2 words)
    pub(crate) fn instr_imm_8_or_16(&mut self, op: u8, idx: u32) -> Result<(), ValidationError> {
        // We support up to 16-bit indexes
        if idx > 0xFFFF {
            Err(ValidationError::IdxTooLarge)
        } else if idx < 0xFF {
            // Encode in a single 16-bit word
            self.instr_imm_8(op, idx as u8)?;
            Ok(())
        } else {
            self.instr_imm_8(op, 0xFF)?;
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
    pub(crate) fn instr_mem(&mut self, op: u8, m: MemArg) -> Result<(), ValidationError> {
        // Check if the alignment exponent can fit into 8-bits
        if m.align >= 0xFF {
            Err(ValidationError::MemAlignTooLarge)
        } else {
            self.instr_imm_8(op, m.align as u8)?;
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
    pub(crate) fn instr_imm_8_or_32(&mut self, op: u8, i: u32) -> Result<(), AllocError> {
        if i < 0xFF {
            // Inline encoding: operand fits in 8 bits (0-254)
            self.instr_imm_8(op, i as u8)?;
        } else {
            // Extended encoding: 0xFF is sentinel, read full 32-bit value from next 2 words
            self.instr_imm_8(op, 0xFF)?;
            self.write_32(i)?;
        }

        Ok(())
    }

    /// Push an operation with an 8-bit inline or 64-bit extended operand.
    ///
    /// Encoding:
    /// - Values 0-254: encoded inline in lower 8 bits of first word
    /// - Values 255+: 0xFF sentinel in first word, followed by full 64-bit value in next 4 words
    pub(crate) fn instr_imm_8_or_64(&mut self, op: u8, i: u64) -> Result<(), AllocError> {
        if i < 0xFF {
            // Inline encoding: operand fits in 8 bits (0-254)
            self.instr_imm_8(op, i as u8)?;
        } else {
            // Extended encoding: 0xFF is sentinel, read full 64-bit value from next 4 words
            self.instr_imm_8(op, 0xFF)?;
            self.write_64(i)?;
        }

        Ok(())
    }

    pub(crate) fn write_16(&mut self, word: u16) -> Result<(), AllocError> {
        self.code.push(word)
    }

    pub(crate) fn write_32(&mut self, i: u32) -> Result<(), AllocError> {
        self.code.push(i as u16)?;
        self.code.push((i >> 16) as u16)?;
        Ok(())
    }

    pub(crate) fn write_64(&mut self, i: u64) -> Result<(), AllocError> {
        self.code.push(i as u16)?;
        self.code.push((i >> 16) as u16)?;
        self.code.push((i >> 32) as u16)?;
        self.code.push((i >> 48) as u16)?;
        Ok(())
    }
}

#[cfg(kani)]
mod kani_proofs {
    use super::*;

    /// Verify LabelTarget encoding/decoding correctness
    #[kani::proof]
    fn proof_label_target_roundtrip() {
        let type_selector: u8 = kani::any();
        let depth: u8 = kani::any();
        let offset_raw: i32 = kani::any();

        // Constrain offset to 22-bit signed range
        kani::assume(offset_raw >= -(1 << 21) && offset_raw < (1 << 21));

        let result_type = match type_selector % 5 {
            0 => ResultType(None),
            1 => ResultType(Some(ValType::I32)),
            2 => ResultType(Some(ValType::I64)),
            3 => ResultType(Some(ValType::F32)),
            4 => ResultType(Some(ValType::F64)),
            _ => unreachable!(),
        };

        let offset = JumpOffset(offset_raw);

        let label = LabelTarget::new(result_type, depth, offset);

        let decoded_arity = label.arity();
        let decoded_depth = label.depth();
        let decoded_offset = label.jump();

        let expected_arity = match result_type.0 {
            None => LabelArity::None,
            Some(ValType::I32 | ValType::F32) => LabelArity::I32,
            Some(ValType::I64 | ValType::F64) => LabelArity::I64,
        };
        assert_eq!(decoded_arity, expected_arity, "Arity must decode correctly");

        assert_eq!(decoded_depth, depth, "Depth must decode correctly");

        assert_eq!(
            decoded_offset.0, offset.0,
            "Offset must decode correctly with proper sign extension"
        );
    }

    /// Verify JumpOffset::new() validation only accepts offsets in [-2^21, 2^21-1]
    #[kani::proof]
    fn proof_jump_offset_validation() {
        let current: u32 = kani::any();
        let to: u32 = kani::any();

        kani::assume(current < (1 << 30));
        kani::assume(to < (1 << 30));

        let current_target = JumpTarget(current);
        let to_target = JumpTarget(to);

        let result = JumpOffset::new(current_target, to_target);

        let offset = (to as i32).wrapping_sub(current as i32);

        let fits_in_22_bits = offset == ((offset << 10) >> 10);

        if fits_in_22_bits {
            assert!(
                result.is_ok(),
                "Offset within 22-bit range should be accepted"
            );

            if let Ok(jump_offset) = result {
                assert_eq!(
                    jump_offset.0, offset,
                    "Accepted offset should match computed value"
                );
            }
        } else {
            assert!(
                result.is_err(),
                "Offset outside 22-bit range should be rejected"
            );
        }
    }

    /// Verify JumpTarget + JumpOffset arithmetic works correctly
    #[kani::proof]
    fn proof_jump_target_addition() {
        let pc: u32 = kani::any();
        let offset_raw: i32 = kani::any();

        kani::assume(pc < (1 << 30));
        kani::assume(offset_raw >= -(1 << 21) && offset_raw < (1 << 21));

        let target = JumpTarget(pc);
        let offset = JumpOffset(offset_raw);

        let result = target + offset;

        let expected = ((pc as i32).wrapping_add(offset_raw)) as u32;
        assert_eq!(
            result.0, expected,
            "JumpTarget + JumpOffset should compute correct address"
        );
    }

    /// Verify that sentinel jump targets work correctly
    #[kani::proof]
    fn proof_jump_offset_sentinel() {
        let current: u32 = kani::any();
        kani::assume(current < (1 << 30));

        let current_target = JumpTarget(current);
        let sentinel = JumpTarget::SENTINEL;

        let result = JumpOffset::new(current_target, sentinel);

        assert!(
            result.is_ok(),
            "Creating offset to SENTINEL should always succeed"
        );

        if let Ok(offset) = result {
            assert_eq!(
                offset,
                JumpOffset::sentinel(),
                "Offset to SENTINEL should be sentinel"
            );
            assert_eq!(offset.0, 0, "Sentinel offset should be 0");
        }
    }
}
