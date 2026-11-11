//! The `wasm-opt` source map: uplinked byte offset to linked byte offset.
//!
//! `wasm-link` feeds `wasm-opt` an identity map where line N is byte N of the linked
//! module, so the "original line" of each output segment is a linked offset. Only one
//! line is ever generated, and `sources`/`names`/columns carry nothing.

use crate::error::{Error, Result};
use crate::format::OFFSETS_SECTION;
use serde::Deserialize;

#[derive(Deserialize)]
struct SourceMap {
    version: u32,
    mappings: String,
}

/// Sorted by uplinked offset. Instructions `wasm-opt` synthesised have no entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offsets(Vec<(u64, u64)>);

impl Offsets {
    pub fn parse(json: &[u8]) -> Result<Offsets> {
        let map: SourceMap = serde_json::from_slice(json)
            .map_err(|err| bad(format!("`{OFFSETS_SECTION}` is not a source map: {err}")))?;
        if map.version != 3 {
            return Err(bad(format!(
                "`{OFFSETS_SECTION}` is source map version {}, not 3",
                map.version
            )));
        }
        if map.mappings.contains(';') {
            return Err(bad(format!(
                "`{OFFSETS_SECTION}` maps more than one generated line; a module is one"
            )));
        }

        let mut entries = Vec::new();
        let (mut generated, mut original) = (0i64, 0i64);
        for segment in map.mappings.split(',').filter(|s| !s.is_empty()) {
            let fields = vlq(segment)?;
            generated += fields[0];
            // A one-field segment marks bytes with no origin: synthesised by `wasm-opt`.
            if let [_, _, line, ..] = fields[..] {
                original += line;
                entries.push((offset(generated)?, offset(original)?));
            }
        }
        entries.sort_unstable();
        Ok(Offsets(entries))
    }

    /// The linked offset `uplinked` came from. Synthesised instructions take the nearest
    /// mapped one before them, which is the source they were optimised out of.
    pub fn linked(&self, uplinked: u64) -> Option<u64> {
        let index = self.0.partition_point(|&(at, _)| at <= uplinked);
        index.checked_sub(1).map(|i| self.0[i].1)
    }
}

fn offset(value: i64) -> Result<u64> {
    u64::try_from(value).map_err(|_| bad(format!("`{OFFSETS_SECTION}` maps to a negative offset")))
}

fn bad(why: String) -> Error {
    Error::Format(why)
}

/// One segment's base64 VLQ fields.
fn vlq(segment: &str) -> Result<Vec<i64>> {
    let mut fields = Vec::new();
    let (mut value, mut shift) = (0i64, 0u32);
    for c in segment.bytes() {
        let digit = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => {
                return Err(bad(format!(
                    "`{OFFSETS_SECTION}` has `{}` in a mapping",
                    c as char
                )));
            }
        } as i64;
        if shift > 60 {
            return Err(bad(format!(
                "`{OFFSETS_SECTION}` has a mapping field too large to be an offset"
            )));
        }
        value |= (digit & 31) << shift;
        shift += 5;
        if digit & 32 == 0 {
            fields.push(if value & 1 == 1 {
                -(value >> 1)
            } else {
                value >> 1
            });
            (value, shift) = (0, 0);
        }
    }
    if shift != 0 {
        return Err(bad(format!("`{OFFSETS_SECTION}` ends mid-field")));
    }
    Ok(fields)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(mappings: &str) -> Vec<u8> {
        format!(r#"{{"version":3,"sources":["linked"],"names":[],"mappings":"{mappings}"}}"#)
            .into_bytes()
    }

    #[test]
    fn decodes_vlq() {
        assert_eq!(vlq("AAAA").unwrap(), [0, 0, 0, 0]);
        assert_eq!(vlq("CACA").unwrap(), [1, 0, 1, 0]);
        assert_eq!(vlq("D").unwrap(), [-1]);
        // 16 needs a continuation digit.
        assert_eq!(vlq("gB").unwrap(), [16]);
        assert!(vlq("g").is_err(), "a dangling continuation must not decode");
    }

    #[test]
    fn deltas_accumulate() {
        // 0x10 -> 0x20, then +2 -> +3, then a synthesised byte, then +1 -> +1.
        let offsets = Offsets::parse(&map("gBAgCA,EAGA,C,CACA")).unwrap();
        assert_eq!(offsets.0, [(0x10, 0x20), (0x12, 0x23), (0x14, 0x24)]);
    }

    #[test]
    fn synthesised_takes_preceding() {
        let offsets = Offsets::parse(&map("gBAgCA,EAGA,C,CACA")).unwrap();
        assert_eq!(
            offsets.linked(0x0f),
            None,
            "nothing maps before the first entry"
        );
        assert_eq!(offsets.linked(0x10), Some(0x20));
        assert_eq!(
            offsets.linked(0x13),
            Some(0x23),
            "the unmapped byte at 0x13"
        );
        assert_eq!(offsets.linked(0x99), Some(0x24));
    }

    #[test]
    fn rejects_other_maps() {
        assert!(Offsets::parse(b"not json").is_err());
        assert!(Offsets::parse(&map("AAAA;AAAA")).is_err());
        let v2 = br#"{"version":2,"mappings":""}"#;
        assert!(Offsets::parse(v2).is_err());
    }
}
