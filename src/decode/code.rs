use crate::*;

pub struct Func {
    locals: Vec<ValType>,
    expr: Slice,
}

impl Func {
    pub fn read(wasm: &mut WasmReader, size: u32) -> Result<Func, DecodeError> {
        // Nested list of locals needs to be counted before allocating
        let mut n_locals = 0;
        let n_local_lists = wasm.read_u32()?;

        let start = wasm.save();
        for _ in 0..n_local_lists as usize {
            let n = wasm.read_u32()?;
            n_locals += n;
        }

        wasm.restore(start);
        let mut locals = Vec::new(n_locals)?;
        for _ in 0..n_local_lists {
            let n = wasm.read_u32()?;
            for _ in 0..n {
                locals.push(ValType::read(wasm)?)
            }
        }

        let expr = Slice::read(wasm, size - (wasm.save() - start))?;

        Ok(Func { locals, expr })
    }
}
