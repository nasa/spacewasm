use crate::*;

pub struct Expr<'wasm>(WasmReaderState<'wasm>);

impl<'wasm> Expr<'wasm> {
    pub fn read(wasm: &mut WasmReader<'wasm>) -> Result<Self, ValidationError> {
        let e = Expr(wasm.save());
        wasm.visit_code(&mut EmptyVisitor {})?;
        Ok(e)
    }

    pub fn visit<E, V>(&self, wasm: &mut WasmReader<'wasm>, visitor: &mut V) -> Result<(), E>
    where
        E: From<ValidationError>,
        V: CodeVisitor<Error = E>,
    {
        wasm.restore(self.0);
        wasm.visit_code(visitor)
    }
}

pub struct Func<'wasm> {
    pub locals: Vec<ValType>,
    pub expr: Slice<'wasm>,
}

impl<'wasm> Func<'wasm> {
    pub fn read(wasm: &mut WasmReader<'wasm>) -> Result<Self, ValidationError> {
        let size = wasm.read_u32()?;

        let start = wasm.save();

        // Nested list of locals needs to be counted before allocating
        let mut n_locals = 0;
        let n_local_lists = wasm.read_u32()?;

        let start_locals = wasm.save();
        for _ in 0..n_local_lists as usize {
            let n = wasm.read_u32()?;
            wasm.read_u8()?;
            n_locals += n;
        }

        wasm.restore(start_locals);
        let mut locals = Vec::new(n_locals)?;
        for _ in 0..n_local_lists {
            let n = wasm.read_u32()?;
            let t = ValType::read(wasm)?;
            for _ in 0..n {
                locals.push(t)
            }
        }

        let expr = Slice::read(wasm, size - (wasm.save() - start))?;

        Ok(Func { locals, expr })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BlockType(pub Option<ValType>);
impl BlockType {
    pub fn read(wasm: &mut WasmReader<'_>) -> Result<Self, ValidationError> {
        if let Ok(c) = wasm.peek_u8()
            && c == 0x40
        {
            Ok(BlockType(None))
        } else {
            ValType::read(wasm).map(|d| BlockType(Some(d)))
        }
    }
}

type CodeAllocator = StackAllocator<1024, 4>;
type CodeVec<'a, T> = Vec<T, &'a CodeAllocator>;

impl<'wasm> WasmReader<'wasm> {
    pub fn visit_code<B: From<ValidationError>, V: CodeVisitor<Error = B>>(
        &mut self,
        visitor: &mut V,
    ) -> Result<(), B> {
        let mut blocks: StackVec<BlockType, 32> = StackVec::new();
        let stack_allocator: CodeAllocator = StackAllocator::new();

        use crate::decode::opcode::*;
        loop {
            let op = self.read_u8()?;
            match op {
                END => {
                    let Some(block_type) = blocks.pop() else {
                        // No more blocks to nest in, this is the end of the code
                        break;
                    };

                    visitor.exit_block(self, block_type)?;
                }
                UNREACHABLE => visitor.unreachable(self)?,
                NOP => visitor.nop(self)?,
                BLOCK => {
                    let block_type = BlockType::read(self)?;
                    blocks.push(block_type);
                    visitor.enter_block(self, block_type)?;
                }
                LOOP => {
                    let block_type = BlockType::read(self)?;
                    blocks.push(block_type);
                    visitor.loop_(self, block_type)?;
                }
                IF => {
                    let block_type = BlockType::read(self)?;
                    blocks.push(block_type);
                    visitor.if_(self, block_type)?;
                }
                ELSE => visitor.else_(self)?,
                BR => {
                    let l = LabelIdx::read(self)?;
                    visitor.br(self, l)?;
                }
                BR_IF => {
                    let l = LabelIdx::read(self)?;
                    visitor.br_if(self, l)?;
                }
                BR_TABLE => {
                    let lut = self.read_vec_in(&stack_allocator, LabelIdx::read)?;
                    let default_ = LabelIdx::read(self)?;
                    visitor.br_table(self, lut, default_)?;
                }
                
                _ => unimplemented!(),
            }
        }

        Ok(())
    }
}

pub trait CodeVisitor {
    type Error: From<ValidationError>;

    fn unreachable(&mut self, pc: &mut WasmReader) -> Result<(), Self::Error> {
        let _ = pc;
        Ok(())
    }

    fn nop(&mut self, pc: &mut WasmReader) -> Result<(), Self::Error> {
        let _ = pc;
        Ok(())
    }

    fn enter_block(
        &mut self,
        pc: &mut WasmReader,
        block_type: BlockType,
    ) -> Result<(), Self::Error> {
        let _ = pc;
        let _ = block_type;
        Ok(())
    }

    fn exit_block(
        &mut self,
        pc: &mut WasmReader,
        block_type: BlockType,
    ) -> Result<(), Self::Error> {
        let _ = pc;
        let _ = block_type;
        Ok(())
    }

    fn loop_(&mut self, pc: &mut WasmReader, block_type: BlockType) -> Result<(), Self::Error> {
        let _ = pc;
        let _ = block_type;
        Ok(())
    }

    fn if_(&mut self, pc: &mut WasmReader, block_type: BlockType) -> Result<(), Self::Error> {
        let _ = pc;
        let _ = block_type;
        Ok(())
    }

    fn else_(&mut self, pc: &mut WasmReader) -> Result<(), Self::Error> {
        let _ = pc;
        Ok(())
    }

    fn br(&mut self, pc: &mut WasmReader, l: LabelIdx) -> Result<(), Self::Error> {
        let _ = pc;
        let _ = l;
        Ok(())
    }

    fn br_if(&mut self, pc: &mut WasmReader, l: LabelIdx) -> Result<(), Self::Error> {
        let _ = pc;
        let _ = l;
        Ok(())
    }

    fn br_table(
        &mut self,
        pc: &mut WasmReader,
        lut: CodeVec<LabelIdx>,
        default_: LabelIdx,
    ) -> Result<(), Self::Error> {
        let _ = pc;
        let _ = lut;
        let _ = default_;
        Ok(())
    }
}

pub struct EmptyVisitor;
impl CodeVisitor for EmptyVisitor {
    type Error = ValidationError;
}
