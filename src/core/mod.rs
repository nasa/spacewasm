mod visitor;
pub use visitor::*;

mod store;
pub use store::*;

mod reader;
pub use reader::*;

mod host;
pub use host::*;

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

mod constant;

mod compiler;
pub use compiler::*;

mod text;
pub use text::*;

#[cfg(test)]
mod compiler_tests;
