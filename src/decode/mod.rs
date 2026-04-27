mod reader;
pub use reader::*;

mod imports;
pub use imports::*;

mod stream;
pub use stream::*;

pub mod error;
pub use error::*;

pub mod module;
pub use module::*;

pub(crate) mod opcode;
pub use opcode::*;

mod types;
pub use types::*;

mod code;
pub use code::*;
