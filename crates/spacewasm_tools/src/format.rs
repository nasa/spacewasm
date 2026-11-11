//! The symbol file: a module as linked, debug info and all, followed by two custom
//! sections.
//!
//! * `spacewasm.module` — the exact module that flies.
//! * `spacewasm.offsets` — a source map from each byte of that module to the linked byte
//!   it was optimised from. Absent when nothing was optimised, since then the code
//!   offsets of both agree.
//!
//! Being a Wasm module itself, a symbol file opens in `wasm-tools`, `llvm-dwarfdump` and
//! `llvm-symbolizer` as it is: their addresses are linked addresses.

use crate::error::{Error, Result};
use crate::offsets::Offsets;
use crate::source::{Frame, Source};
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use wasmparser::{Parser, Payload};

pub const MODULE_SECTION: &str = "spacewasm.module";
pub const OFFSETS_SECTION: &str = "spacewasm.offsets";

/// One symbol file, ready to resolve offsets into the module it describes.
pub struct Symbols {
    /// The exact bytes uplinked.
    module: Vec<u8>,
    /// `None` when nothing was optimised, so uplinked and linked offsets agree.
    offsets: Option<Offsets>,
    /// Where the linked module's code section contents start; DWARF counts from here.
    linked_code: u64,
    source: Source,
}

impl Symbols {
    pub fn read(path: &Path) -> Result<Symbols> {
        let bytes = std::fs::read(path)?;
        Symbols::parse(&bytes)
    }

    pub fn parse(bytes: &[u8]) -> Result<Symbols> {
        let mut module = None;
        let mut offsets = None;
        let mut linked_code = None;
        let mut dwarf: HashMap<String, Rc<[u8]>> = HashMap::new();

        for payload in Parser::new(0).parse_all(bytes) {
            let payload = payload
                .map_err(|err| Error::Format(format!("not a readable Wasm module: {err}")))?;
            match payload {
                Payload::CodeSectionStart { range, .. } => linked_code = Some(range.start),
                Payload::CustomSection(section) => match section.name() {
                    MODULE_SECTION => module = Some(section.data().to_vec()),
                    OFFSETS_SECTION => offsets = Some(Offsets::parse(section.data())?),
                    name if name.starts_with(".debug_") => {
                        dwarf.insert(name.to_string(), Rc::from(section.data()));
                    }
                    _ => {}
                },
                _ => {}
            }
        }

        let module = module.ok_or_else(|| {
            Error::Format(format!(
                "it has no `{MODULE_SECTION}` section, so it was not written by \
                 `spacewasm split` or `spacewasm link`"
            ))
        })?;
        let linked_code =
            linked_code.ok_or_else(|| Error::Format("it has no code section".into()))?;
        if dwarf.get(".debug_info").is_none_or(|info| info.is_empty()) {
            return Err(Error::NoDebugInfo);
        }

        Ok(Symbols {
            module,
            offsets,
            linked_code,
            source: Source::new(dwarf)?,
        })
    }

    /// Whether `uplinked` is the module these symbols describe.
    pub fn describes(&self, uplinked: &[u8]) -> bool {
        self.module == uplinked
    }

    /// The module that flies, as embedded in the symbol file.
    pub fn module(&self) -> &[u8] {
        &self.module
    }

    /// The inline chain at `offset`, innermost first. `offset` is a byte offset into the
    /// uplinked module, from its first byte.
    pub fn frames(&self, offset: u64) -> Result<Vec<Frame>> {
        if offset >= self.module.len() as u64 {
            return Err(Error::Position(format!(
                "offset {offset:#x} is past the end of the {}-byte module",
                self.module.len()
            )));
        }
        let linked = match &self.offsets {
            Some(offsets) => offsets.linked(offset).ok_or_else(|| {
                Error::Position(format!("offset {offset:#x} is not in the module's code"))
            })?,
            None => offset,
        };
        let address = linked.checked_sub(self.linked_code).ok_or_else(|| {
            Error::Position(format!(
                "offset {offset:#x} comes before the module's code section"
            ))
        })?;
        self.source.frames(address)
    }
}
