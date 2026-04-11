#![no_std]

mod reader;
pub(crate) use reader::*;

pub mod error;
pub use error::*;

pub mod module;
pub use module::*;

mod opcode;
mod types;

pub mod alloc;
mod vec;
pub(crate) use vec::*;
