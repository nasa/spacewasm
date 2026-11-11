//! Whether a module will load on board, and if not, why in terms its author can act on.
//!
//! The decoder is what decides; this only explains. Its errors carry an opcode number at
//! best, and the instruction is usually not the author's: `wasm-opt` introduces post-MVP
//! instructions while optimising even when the compiler emitted none.

use crate::features::{self, Proposal};
use crate::replay::{DecoderLimits, Loaded, ReplayError, load};
use spacewasm::{CompilerOptions, HostModule, ValidationError};
use std::fmt;
use wasmparser::{Operator, Parser, Payload};

/// Why a module does not load.
#[derive(Debug)]
pub enum Invalid {
    /// It uses an instruction from a proposal this release does not implement.
    Unsupported {
        /// Byte offset of the instruction in the module.
        offset: u64,
        instruction: String,
        proposal: &'static Proposal,
    },
    /// Anything else the decoder refused.
    Rejected(ReplayError),
}

impl fmt::Display for Invalid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Invalid::Unsupported {
                offset,
                instruction,
                proposal,
            } => {
                write!(
                    f,
                    "`{instruction}` at byte {offset:#x} is from the {} proposal, which this \
                     release of spacewasm does not implement. Supported: {}. ",
                    proposal.name,
                    features::supported()
                        .map(|p| p.name)
                        .collect::<Vec<_>>()
                        .join(", ")
                )?;
                match proposal.rustc {
                    Some(feature) => write!(
                        f,
                        "Keep it out with `-Ctarget-feature=-{feature}`, and link through \
                         `spacewasm link`, whose `wasm-opt` is held to the supported set."
                    ),
                    None => write!(
                        f,
                        "Link through `spacewasm link`, whose `wasm-opt` is held to the \
                         supported set."
                    ),
                }
            }
            Invalid::Rejected(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for Invalid {}

/// Load `module` exactly as the flight interpreter would.
pub fn validate(
    module: &[u8],
    options: CompilerOptions,
    host_modules: spacewasm::Vec<HostModule>,
    limits: DecoderLimits,
) -> Result<Loaded, Invalid> {
    match load(module, options, host_modules, limits) {
        Ok(loaded) => Ok(loaded),
        Err(ReplayError::Decode(err)) if is_opcode_error(&err.err.err) => {
            Err(find_unsupported(module).unwrap_or(Invalid::Rejected(ReplayError::Decode(err))))
        }
        Err(err) => Err(Invalid::Rejected(err)),
    }
}

fn is_opcode_error(err: &ValidationError) -> bool {
    matches!(
        err,
        ValidationError::InvalidOpcode(_) | ValidationError::InvalidFcOpcode(_)
    )
}

/// The first instruction from a proposal this release does not implement, read with
/// `wasmparser`, which knows every proposal's instructions and so can name the one the
/// decoder could not. `None` if every instruction is supported, or the module is too
/// malformed to read.
pub fn find_unsupported(module: &[u8]) -> Option<Invalid> {
    for payload in Parser::new(0).parse_all(module) {
        let Ok(Payload::CodeSectionEntry(body)) = payload else {
            continue;
        };
        let mut reader = body.get_operators_reader().ok()?;
        while !reader.eof() {
            let (operator, offset) = reader.read_with_offset().ok()?;
            let at = module.get(usize::try_from(offset).ok()?..)?;
            let (opcode, sub) = (*at.first()?, fc_sub_opcode(at));
            if let Some(proposal) = features::proposal_of(opcode, sub)
                && !proposal.supported
            {
                return Some(Invalid::Unsupported {
                    offset,
                    instruction: mnemonic(&operator),
                    proposal,
                });
            }
        }
    }
    None
}

/// The LEB128 sub-opcode after a `0xFC` prefix.
fn fc_sub_opcode(bytes: &[u8]) -> Option<u32> {
    if bytes.first() != Some(&0xFC) {
        return None;
    }
    let mut value = 0u32;
    for (i, byte) in bytes.iter().skip(1).take(5).enumerate() {
        value |= u32::from(byte & 0x7F) << (7 * i);
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

/// `I64Extend16S` as `i64.extend16_s`, near enough to the text format to search for.
fn mnemonic(operator: &Operator) -> String {
    let debug = format!("{operator:?}");
    let name = debug.split([' ', '{', '(']).next().unwrap_or(&debug);
    let mut out = String::new();
    let mut previous_lower = false;
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 && previous_lower {
            out.push(if out.contains('.') { '_' } else { '.' });
        }
        previous_lower = c.is_ascii_lowercase() || c.is_ascii_digit();
        out.push(c.to_ascii_lowercase());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mnemonics_read_like_the_text_format() {
        assert_eq!(mnemonic(&Operator::I64Extend16S), "i64.extend16_s");
        assert_eq!(mnemonic(&Operator::MemoryFill { mem: 0 }), "memory.fill");
    }

    #[test]
    fn fc_sub_opcodes_decode_as_leb128() {
        assert_eq!(fc_sub_opcode(&[0xFC, 0x0B]), Some(11));
        assert_eq!(fc_sub_opcode(&[0xFC, 0x80, 0x01]), Some(128));
        assert_eq!(fc_sub_opcode(&[0xC0]), None);
    }
}
