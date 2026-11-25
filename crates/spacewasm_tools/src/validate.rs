//! Whether a module will load on board, and if not, why in terms its author can act on.
//!
//! The decoder is what decides; this only explains. Its errors carry an opcode number at
//! best, and the instruction is usually not the author's: `wasm-opt` introduces post-MVP
//! instructions while optimising even when the compiler emitted none.

use crate::features::{self, Proposal, Version};
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
        /// The release it was checked against; `None` for this one.
        release: Option<Version>,
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
                release,
            } => {
                let (which, supported): (String, Vec<_>) = match release {
                    Some(release) => (
                        format!("spacewasm {release}"),
                        features::supported_in(*release).map(|p| p.name).collect(),
                    ),
                    None => (
                        "this release of spacewasm".into(),
                        features::supported().map(|p| p.name).collect(),
                    ),
                };
                write!(
                    f,
                    "`{instruction}` at byte {offset:#x} is from the {} proposal, which {which} \
                     does not implement. Supported: {}. ",
                    proposal.name,
                    supported.join(", ")
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
    first_unsupported(module, None)
}

/// [`find_unsupported`] against an earlier `release`, for a deployment that has not
/// upgraded: this release's decoder accepts what that one would refuse.
pub fn find_unsupported_in(module: &[u8], release: Version) -> Option<Invalid> {
    first_unsupported(module, Some(release))
}

fn first_unsupported(module: &[u8], release: Option<Version>) -> Option<Invalid> {
    for payload in Parser::new(0).parse_all(module) {
        let Ok(Payload::CodeSectionEntry(body)) = payload else {
            continue;
        };
        let mut reader = body.get_operators_reader().ok()?;
        while !reader.eof() {
            let (operator, offset) = reader.read_with_offset().ok()?;
            let at = module.get(usize::try_from(offset).ok()?..)?;
            let (opcode, sub) = (*at.first()?, fc_sub_opcode(at));
            let Some(proposal) = features::proposal_of(opcode, sub) else {
                continue;
            };
            let supported = match release {
                Some(release) => proposal.supported_in(release),
                None => proposal.supported(),
            };
            if !supported {
                return Some(Invalid::Unsupported {
                    offset,
                    instruction: mnemonic(&operator),
                    proposal,
                    release,
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

    /// `i32.extend8_s` is fine for this release and not for 0.7.1, which predates it.
    #[test]
    fn an_earlier_release_refuses_what_came_later() {
        let module = wat::parse_str(
            r#"(module (func (export "main") (result i32) i32.const 1 i32.extend8_s))"#,
        )
        .unwrap();
        assert!(find_unsupported(&module).is_none());
        let Some(err) = find_unsupported_in(&module, Version::new(0, 7, 1)) else {
            panic!("0.7.1 has no sign-extension operators");
        };
        let message = err.to_string();
        assert!(message.contains("i32.extend8_s"), "{message}");
        assert!(message.contains("spacewasm 0.7.1"), "{message}");
        assert!(message.contains("-Ctarget-feature=-sign-ext"), "{message}");
        assert!(!message.contains("nontrapping"), "{message}");
    }

    #[test]
    fn fc_sub_opcodes_decode_as_leb128() {
        assert_eq!(fc_sub_opcode(&[0xFC, 0x0B]), Some(11));
        assert_eq!(fc_sub_opcode(&[0xFC, 0x80, 0x01]), Some(128));
        assert_eq!(fc_sub_opcode(&[0xC0]), None);
    }
}
