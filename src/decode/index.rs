use crate::{CodeVisitor, ResultType, ValidationError};

pub struct CodeIndexer;

impl CodeVisitor for CodeIndexer {
    type Error = ValidationError;
    type State = ();

    fn enter_block(
        &self,
        block_type: ResultType,
        state: &mut Self::State,
    ) -> Result<(), Self::Error> {
        let _ = block_type;
        let _ = state;
        Ok(())
    }

    fn loop_(&self, block_type: ResultType, state: &mut Self::State) -> Result<(), Self::Error> {
        let _ = block_type;
        let _ = state;
        Ok(())
    }

    fn if_(&self, block_type: ResultType, state: &mut Self::State) -> Result<(), Self::Error> {
        let _ = block_type;
        let _ = state;
        Ok(())
    }

    fn else_(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let _ = state;
        Ok(())
    }

    fn exit_block(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let _ = state;
        Ok(())
    }
}
