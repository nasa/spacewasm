extern crate std;
use crate::*;

pub struct Func<'wasm> {
    pub locals: Vec<ValType>,
    pub expr: Slice<'wasm>,
}

impl<'wasm> Func<'wasm> {
    pub fn read(wasm: &mut WasmReader<'wasm>) -> Result<Self, DecodeError> {
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
