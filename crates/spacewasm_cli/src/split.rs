//! Splitting a linked module into the module to uplink and its symbol file.
//!
//! The debug info cannot simply be carried through `wasm-opt`: its optimiser leaves the
//! DWARF sections in place but empties the line table. So the symbol file keeps the
//! module *as linked*, DWARF intact, and `wasm-opt` is asked for a source map from each
//! optimised byte back to the linked byte it came from. Feeding it an identity map (line
//! N is linked byte N) is what makes its output map read that way.

use crate::format::{MODULE_SECTION, OFFSETS_SECTION};
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;
use wasm_encoder::{CustomSection, Encode, Section};
use wasmparser::Parser;

/// What `wasm-opt` should be run with. `None` means link only: the module is stripped
/// of its custom sections and nothing is optimised.
#[derive(Debug, Clone, Default)]
pub struct Optimizer {
    /// The `wasm-opt` to run.
    pub program: PathBuf,
    /// Passed before the input, e.g. the feature set and `-Os`.
    pub args: Vec<String>,
}

impl Optimizer {
    /// `program` held to this release's feature set, then `extra`.
    pub fn held(program: PathBuf, extra: &[String]) -> Optimizer {
        Optimizer {
            program,
            args: crate::wasm_opt_flags()
                .into_iter()
                .chain(extra.iter().cloned())
                .collect(),
        }
    }
}

#[derive(Debug)]
pub struct Split {
    /// The module to uplink: no debug info, no custom sections.
    pub module: Vec<u8>,
    /// The symbol file to keep on the ground.
    pub symbols: Vec<u8>,
}

#[derive(Debug)]
pub enum SplitError {
    /// The input is not a module `wasmparser` can read.
    Unreadable(String),
    /// `wasm-opt` could not be started or failed.
    Optimizer(String),
    Io(std::io::Error),
}

impl fmt::Display for SplitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SplitError::Unreadable(err) => write!(f, "not a readable Wasm module: {err}"),
            SplitError::Optimizer(err) => f.write_str(err),
            SplitError::Io(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for SplitError {}

impl From<std::io::Error> for SplitError {
    fn from(err: std::io::Error) -> Self {
        SplitError::Io(err)
    }
}

/// Split `linked`, a module straight from the linker with its debug info.
///
/// `scratch` is a directory for `wasm-opt`'s input and output files.
pub fn split(
    linked: &[u8],
    optimizer: Option<&Optimizer>,
    scratch: &Path,
) -> Result<Split, SplitError> {
    let (module, offsets) = match optimizer {
        None => {
            let module = strip(linked)?;
            // `rust-lld` puts every custom section last, so stripping moves no code byte
            // and the symbol file needs no map. Anything else gets one.
            let shift = code_start(linked)? - code_start(&module)?;
            let offsets = (shift != 0)
                .then(|| shifted_map(&module, shift))
                .transpose()?;
            (module, offsets)
        }
        Some(optimizer) => {
            let (module, map) = optimize(linked, optimizer, scratch)?;
            (module, Some(map))
        }
    };

    let mut symbols = linked.to_vec();
    append(&mut symbols, MODULE_SECTION, &module);
    if let Some(offsets) = offsets {
        append(&mut symbols, OFFSETS_SECTION, &offsets);
    }
    Ok(Split { module, symbols })
}

/// `module` without its custom sections, byte for byte what `rust-lld --strip-all` writes.
pub fn strip(module: &[u8]) -> Result<Vec<u8>, SplitError> {
    let mut out = module
        .get(..8)
        .ok_or_else(|| SplitError::Unreadable("shorter than the Wasm header".into()))?
        .to_vec();
    for payload in Parser::new(0).parse_all(module) {
        let payload = payload.map_err(|err| SplitError::Unreadable(err.to_string()))?;
        // `CodeSectionEntry` is inside the code section `as_section` already covers.
        if let Some((id, range)) = payload.as_section() {
            if id != 0 {
                let body = &module[range.start as usize..range.end as usize];
                out.push(id);
                body.len().encode(&mut out);
                out.extend_from_slice(body);
            }
        }
    }
    Ok(out)
}

/// Where the code section's contents start; 0 for a module with no code.
fn code_start(module: &[u8]) -> Result<usize, SplitError> {
    for payload in Parser::new(0).parse_all(module) {
        let payload = payload.map_err(|err| SplitError::Unreadable(err.to_string()))?;
        if let wasmparser::Payload::CodeSectionStart { range, .. } = payload {
            return Ok(range.start as usize);
        }
    }
    Ok(0)
}

/// A source map from each code byte of `stripped` to the linked byte `shift` later.
fn shifted_map(stripped: &[u8], shift: usize) -> Result<Vec<u8>, SplitError> {
    let start = code_start(stripped)?;
    let end = Parser::new(0)
        .parse_all(stripped)
        .filter_map(Result::ok)
        .find_map(|payload| match payload {
            wasmparser::Payload::CodeSectionStart { range, .. } => Some(range.end as usize),
            _ => None,
        })
        .unwrap_or(start);

    let mut mappings = String::new();
    let (mut column, mut line) = (0i64, 0i64);
    for byte in start..end {
        if !mappings.is_empty() {
            mappings.push(',');
        }
        let (byte, linked) = (byte as i64, (byte + shift) as i64);
        for field in [byte - column, 0, linked - line, 0] {
            vlq(field, &mut mappings);
        }
        (column, line) = (byte, linked);
    }
    Ok(
        format!(r#"{{"version":3,"sources":["linked"],"names":[],"mappings":"{mappings}"}}"#)
            .into_bytes(),
    )
}

/// One base64 VLQ field.
fn vlq(value: i64, out: &mut String) {
    const DIGITS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut rest = if value < 0 {
        ((-value) << 1) | 1
    } else {
        value << 1
    };
    loop {
        let mut digit = rest & 31;
        rest >>= 5;
        if rest != 0 {
            digit |= 32;
        }
        out.push(DIGITS[digit as usize] as char);
        if rest == 0 {
            break;
        }
    }
}

fn append(out: &mut Vec<u8>, name: &str, data: &[u8]) {
    CustomSection {
        name: name.into(),
        data: data.into(),
    }
    .append_to(out);
}

fn optimize(
    linked: &[u8],
    optimizer: &Optimizer,
    scratch: &Path,
) -> Result<(Vec<u8>, Vec<u8>), SplitError> {
    let input = scratch.join("linked.wasm");
    let input_map = scratch.join("linked.map");
    let output = scratch.join("optimized.wasm");
    let output_map = scratch.join("optimized.map");

    std::fs::write(&input, linked)?;
    std::fs::write(&input_map, identity_map(linked.len()))?;

    // `--strip-dwarf`, not `--strip-debug`: the latter drops the source map too.
    let status = Command::new(&optimizer.program)
        .args(&optimizer.args)
        .args([
            "--strip-dwarf",
            "--strip-producers",
            "--strip-target-features",
        ])
        .arg(&input)
        .arg("-ism")
        .arg(&input_map)
        .arg("-o")
        .arg(&output)
        .arg("-osm")
        .arg(&output_map)
        .status()
        .map_err(|err| {
            SplitError::Optimizer(format!(
                "could not run `{}`: {err}",
                optimizer.program.display()
            ))
        })?;
    if !status.success() {
        return Err(SplitError::Optimizer(format!(
            "`{}` failed with {status}",
            optimizer.program.display()
        )));
    }

    let module = std::fs::read(&output)?;
    let map = std::fs::read(&output_map)?;
    Ok((module, map))
}

/// A source map whose line N is byte N, for every byte but the first.
///
/// Byte 0 is left out because `wasm-opt` drops a whole input map that maps column 0;
/// it is the `\0asm` magic, which no instruction can be optimised from.
fn identity_map(len: usize) -> String {
    let mut map = String::with_capacity(len * 5 + 64);
    map.push_str(r#"{"version":3,"sources":["linked"],"names":[],"mappings":""#);
    for byte in 1..len {
        if byte > 1 {
            map.push(',');
        }
        // Each segment moves one column and one line on from the last: `CACA`.
        map.push_str("CACA");
    }
    map.push_str(r#""}"#);
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_map_starts_at_byte_one() {
        assert_eq!(
            identity_map(4),
            r#"{"version":3,"sources":["linked"],"names":[],"mappings":"CACA,CACA,CACA"}"#
        );
    }

    use wasmparser::Payload;

    #[test]
    fn vlq_round_trips_through_the_reader() {
        let mut encoded = String::new();
        for value in [0, 1, -1, 15, 16, -16, 1000] {
            vlq(value, &mut encoded);
        }
        assert_eq!(encoded, "ACDegBhBw+B");
    }

    /// A custom section before the code moves it; stripping must then say where to.
    #[test]
    fn stripping_a_leading_custom_section_maps_the_shift() {
        let linked = wat::parse_str(
            r#"(module
                 (@custom "lead" (before func) "0123456789")
                 (func (export "main") nop nop))"#,
        )
        .unwrap();
        let split = split(&linked, None, Path::new(".")).unwrap();
        let symbols_code = code_start(&linked).unwrap();
        let module_code = code_start(&split.module).unwrap();
        assert!(symbols_code > module_code, "the custom section came first");

        let offsets = Parser::new(0)
            .parse_all(&split.symbols)
            .find_map(|p| match p.unwrap() {
                Payload::CustomSection(c) if c.name() == OFFSETS_SECTION => {
                    Some(crate::offsets::Offsets::parse(c.data()).unwrap())
                }
                _ => None,
            })
            .expect("a shifted strip writes offsets");
        assert_eq!(
            offsets.linked(module_code as u64),
            Some(symbols_code as u64)
        );
    }

    #[test]
    fn strip_keeps_every_other_section_in_order() {
        let module = wat::parse_str(
            r#"(module
                 (@custom "a" "x")
                 (memory 1)
                 (@custom "b" "yy")
                 (func (export "main") nop))"#,
        )
        .unwrap();
        let stripped = strip(&module).unwrap();

        let names: Vec<_> = Parser::new(0)
            .parse_all(&stripped)
            .filter_map(|p| match p.unwrap() {
                Payload::CustomSection(c) => Some(c.name().to_string()),
                _ => None,
            })
            .collect();
        assert!(names.is_empty(), "{names:?}");
        assert!(stripped.len() < module.len());
        wasmparser::Validator::new()
            .validate_all(&stripped)
            .unwrap();
    }
}
