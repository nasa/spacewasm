//! What can go wrong reading a symbol file or resolving an offset.

use std::fmt;

#[derive(Debug)]
pub enum Error {
    /// The bytes are not a symbol file, or are a damaged one.
    Format(String),
    /// The symbol file was built without debug info.
    NoDebugInfo,
    /// The DWARF is there but `gimli` cannot read it.
    Dwarf(String),
    /// An offset or PC that is not inside the module's code.
    Position(String),
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Format(why) | Error::Position(why) => f.write_str(why),
            Error::NoDebugInfo => f.write_str(
                "the symbol file has no debug info. Link with debug info (`debug = true` in \
                 the Cargo profile) before splitting",
            ),
            Error::Dwarf(why) => write!(f, "the symbol file's debug info is unreadable: {why}"),
            Error::Io(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::Io(err)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
