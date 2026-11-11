//! A backtrace of interpreter PCs, resolved to source.

use crate::error::{Error, Result};
use crate::format::Symbols;
use crate::ir::IrMap;
use crate::source::Frame;

/// One backtrace entry, resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// The PC as reported, if this entry was one.
    pub pc: Option<u32>,
    /// Byte offset into the uplinked module.
    pub offset: u64,
    /// Inline chain at `offset`, innermost first. Empty when the debug info records
    /// nothing there.
    pub frames: Vec<Frame>,
}

/// Resolve `pcs`, innermost first, as `spacewasm_backtrace` / `Engine::backtrace` report
/// them.
///
/// Every element but the first is a caller's `call` instruction, exactly. The first is
/// the faulting instruction for a trap, the `call` that paused for a pause, or the next
/// instruction to run for a call out of fuel.
pub fn resolve_pcs(symbols: &Symbols, map: &IrMap, pcs: &[u32]) -> Result<Vec<Resolved>> {
    pcs.iter()
        .map(|&pc| {
            let offset = map.wasm_offset(pc).ok_or_else(|| {
                Error::Position(format!(
                    "PC {pc:#x} is before the module's first instruction"
                ))
            })?;
            let offset = u64::from(offset);
            Ok(Resolved {
                pc: Some(pc),
                offset,
                frames: symbols.frames(offset)?,
            })
        })
        .collect()
}

/// Resolve byte offsets into the uplinked module, innermost first.
pub fn resolve_offsets(symbols: &Symbols, offsets: &[u64]) -> Result<Vec<Resolved>> {
    offsets
        .iter()
        .map(|&offset| {
            Ok(Resolved {
                pc: None,
                offset,
                frames: symbols.frames(offset)?,
            })
        })
        .collect()
}
