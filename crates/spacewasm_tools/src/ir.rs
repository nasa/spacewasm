//! An interpreter program counter back to the bytecode it was compiled from.
//!
//! `spacewasm` compiles each Wasm instruction to one or more 16-bit IR words, so the
//! mapping is only known by decoding the module again exactly as the flight interpreter
//! did. A PC here is the IR word address relative to the module's first compiled word;
//! the layout does not depend on where the module starts, since every jump is relative.
//!
//! Every PC the interpreter reports is inside the instruction it names: a trap's is the
//! faulting instruction, a pause's the `call` that paused, until resumed.

/// `(ir, wasm)` pairs: the first IR word of each compiled instruction, and the byte
/// offset of its opcode in the module. Sorted by `ir`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IrMap(Vec<(u32, u32)>);

impl IrMap {
    pub fn new(mut entries: Vec<(u32, u32)>) -> IrMap {
        entries.sort_unstable();
        IrMap(entries)
    }

    /// The bytecode offset of the instruction IR word `pc` belongs to.
    pub fn wasm_offset(&self, pc: u32) -> Option<u32> {
        let bound = self.0.partition_point(|&(start, _)| start <= pc);
        bound.checked_sub(1).map(|i| self.0[i].1)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three instructions: 1, 3 and 2 IR words long.
    fn map() -> IrMap {
        IrMap::new(vec![(4, 0x90), (0, 0x86), (1, 0x88)])
    }

    #[test]
    fn resolves_every_word_of_an_instruction() {
        let map = map();
        assert_eq!(map.wasm_offset(0), Some(0x86));
        assert_eq!(map.wasm_offset(1), Some(0x88));
        assert_eq!(map.wasm_offset(3), Some(0x88), "an operand word");
        assert_eq!(map.wasm_offset(5), Some(0x90));
    }

    #[test]
    fn before_the_first_instruction_is_nothing() {
        assert_eq!(IrMap::new(vec![(2, 0x86)]).wasm_offset(1), None);
    }
}
