use crate::{CodeVisitor, ResultType, StackVec, ValidationError, WasmIndex, WasmReader};

pub struct CodeIndexer<'wasm> {
    /// Keeps track of nested blocks with a static cap
    block_starts: StackVec<WasmIndex<'wasm>, 64>,

    /// Keeps track of block sizes
    indexes: StackVec<(WasmIndex<'wasm>, ResultType), 32>,
}

impl<'wasm> CodeIndexer<'wasm> {
    pub fn new() -> CodeIndexer<'wasm> {
        
    }
}

impl<'wasm> CodeVisitor<'wasm> for CodeIndexer<'wasm> {
    type Error = ValidationError;

    fn enter_block(
        &mut self,
        pc: &mut WasmReader<'wasm>,
        _: ResultType,
    ) -> Result<(), Self::Error> {
        self.block_starts.push(pc.save())?;
        Ok(())
    }

    fn loop_(
        &mut self,
        pc: &mut WasmReader<'wasm>,
        block_type: ResultType,
    ) -> Result<(), Self::Error> {
        self.block_starts.push(pc.save())?;
        self.block_types.push(block_type)?;
        Ok(())
    }

    fn if_(
        &mut self,
        pc: &mut WasmReader<'wasm>,
        block_type: ResultType,
    ) -> Result<(), Self::Error> {
        self.block_starts.push(pc.save())?;
        self.block_types.push(block_type)?;
        Ok(())
    }

    fn else_(&mut self, _pc: &mut WasmReader<'wasm>) -> Result<(), Self::Error> {
        Ok(())
    }

    fn exit_block(&mut self, _pc: &mut WasmReader<'wasm>) -> Result<(), Self::Error> {
        let (_block_start, _block_type) = (
            self.block_starts.pop().unwrap(),
            self.block_types.pop().unwrap(),
        );

        Ok(())
    }
}
