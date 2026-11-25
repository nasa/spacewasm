//! A linked code offset to source, through the DWARF `rust-lld` wrote.

use crate::error::{Error, Result};
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

type Reader = gimli::EndianRcSlice<gimli::LittleEndian>;

/// Where in the source one frame is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub file: String,
    pub line: u32,
    /// `None` when the compiler recorded the line alone.
    pub column: Option<u32>,
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.file, self.line)?;
        match self.column {
            Some(column) => write!(f, ":{column}"),
            None => Ok(()),
        }
    }
}

/// One function in an inline chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Demangled. `None` for a frame the DWARF has a line for but no function.
    pub function: Option<String>,
    pub location: Option<Location>,
}

pub struct Source {
    context: addr2line::Context<Reader>,
}

impl Source {
    /// `sections` by name, `.debug_info` and so on.
    pub fn new(mut sections: HashMap<String, Rc<[u8]>>) -> Result<Source> {
        let dwarf = gimli::Dwarf::load(|id| {
            let bytes = sections.remove(id.name()).unwrap_or_else(|| Rc::from([]));
            Ok::<_, Error>(Reader::new(bytes, gimli::LittleEndian))
        })?;
        let context =
            addr2line::Context::from_dwarf(dwarf).map_err(|err| Error::Dwarf(err.to_string()))?;
        Ok(Source { context })
    }

    /// Innermost first. `address` is relative to the start of the code section's
    /// contents, as DWARF for Wasm counts it.
    pub fn frames(&self, address: u64) -> Result<Vec<Frame>> {
        let unreadable = |err: gimli::Error| Error::Dwarf(err.to_string());
        let mut iter = self
            .context
            .find_frames(address)
            .skip_all_loads()
            .map_err(unreadable)?;

        let mut frames = Vec::new();
        while let Some(frame) = iter.next().map_err(unreadable)? {
            let function = frame
                .function
                .as_ref()
                .map(|name| name.demangle().map(|name| name.into_owned()))
                .transpose()
                .map_err(unreadable)?;
            let location = frame.location.and_then(|location| {
                Some(Location {
                    file: location.file?.to_string(),
                    line: location.line?,
                    column: location.column.filter(|&column| column > 0),
                })
            });
            frames.push(Frame { function, location });
        }
        Ok(frames)
    }
}
