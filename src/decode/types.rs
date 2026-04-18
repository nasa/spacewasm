//! Methods to read WASM Types from a [WasmReader] object.
//!
//! See: <https://webassembly.github.io/spec/core/binary/types.html>

use crate::util::String;
use crate::{ValidationError, Vec, WasmReader};

/// A pointer and length into a UTF-8 string on the original WASM
pub struct Name;

impl Name {
    pub(crate) fn read(wasm: &mut WasmReader) -> Result<String, ValidationError> {
        wasm.read_vec(|r| r.read_u8())?.try_into()
    }
}

/// Value types classify the individual values that WebAssembly code can compute with and the values
/// that a variable accepts.
/// https://www.w3.org/TR/wasm-core-1/#syntax-valtype
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum ValType {
    I32,
    I64,
    F32,
    F64,
}

impl ValType {
    fn convert(v: u8) -> Result<ValType, ValidationError> {
        // Value types are encoded by a single byte.
        use ValType::*;
        match v {
            0x7F => Ok(I32),
            0x7E => Ok(I64),
            0x7D => Ok(F32),
            0x7C => Ok(F64),
            other => Err(ValidationError::MalformedValueType(other)),
        }
    }

    pub(crate) fn read(wasm: &mut WasmReader) -> Result<Self, ValidationError> {
        ValType::convert(wasm.read_u8()?)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ResultType(pub Option<ValType>);

impl ResultType {
    pub(crate) fn read(wasm: &mut WasmReader) -> Result<Self, ValidationError> {
        // The only result types occurring in the binary format are the types of blocks.
        // These are encoded in special compressed form, by either the byte 0x40 indicating
        // the empty type or as a single value type.
        match wasm.read_u8()? {
            0x40 => Ok(ResultType(None)),
            c => ValType::convert(c).map(|v| ResultType(Some(v))),
        }
    }
}

pub struct FuncType {
    pub params: Vec<ValType>,
    pub returns: Vec<ValType>,
}

impl FuncType {
    pub(crate) fn read(wasm: &mut WasmReader) -> Result<Self, ValidationError> {
        // Function types are encoded by the byte 0x60 followed by the respective
        // vectors of parameter and result types.
        match wasm.read_u8()? {
            0x60 => {
                let params = wasm.read_vec(ValType::read)?;
                let returns = wasm.read_vec(ValType::read)?;

                Ok(FuncType { params, returns })
            }
            c => Err(ValidationError::MalformedFunction(c)),
        }
    }
}

pub struct Limit {
    pub min: u32,
    // Note: We are disabling `max` memory size since we don't support memory.grow
    // pub max: Option<core::num::NonZeroU32>,
}

impl Limit {
    pub(crate) fn read(wasm: &mut WasmReader) -> Result<Self, ValidationError> {
        // Limits are encoded with a preceding flag indicating whether a maximum is present.
        match wasm.read_u8()? {
            0x00 => Ok(Limit {
                min: wasm.read_u32()?,
                // max: None,
            }),
            0x01 => {
                let min = wasm.read_u32()?;

                // Note: We are disabling `max` memory size since we don't support memory.grow
                let _: core::num::NonZero<u32> = core::num::NonZero::new(wasm.read_u32()?)
                    .ok_or(ValidationError::InvalidZeroMaxLimit)?;

                Ok(Limit {
                    min,
                    // max: Some(
                    //     max,
                    // ),
                })
            }
            c => Err(ValidationError::MalformedLimit(c)),
        }
    }
}

pub struct MemType(pub Limit);

impl MemType {
    pub(crate) fn read(wasm: &mut WasmReader) -> Result<Self, ValidationError> {
        // Memory types are encoded with their limits.
        Ok(MemType(Limit::read(wasm)?))
    }
}

pub enum ElemType {
    FuncRef,
}

impl ElemType {
    pub(crate) fn read(wasm: &mut WasmReader) -> Result<Self, ValidationError> {
        match wasm.read_u8()? {
            0x70 => Ok(ElemType::FuncRef),
            c => Err(ValidationError::MalformedElemType(c)),
        }
    }
}

pub struct TableType {
    pub elem_type: ElemType,
    pub limits: Limit,
}

impl TableType {
    pub(crate) fn read(wasm: &mut WasmReader) -> Result<Self, ValidationError> {
        // Table types are encoded with their limits and a constant byte indicating their element type.
        Ok(TableType {
            elem_type: ElemType::read(wasm)?,
            limits: Limit::read(wasm)?,
        })
    }
}

pub struct GlobalType {
    pub val_type: ValType,
    pub mutable: bool,
}

impl GlobalType {
    pub(crate) fn read(wasm: &mut WasmReader) -> Result<Self, ValidationError> {
        let val_type = ValType::read(wasm)?;
        let mutable = match wasm.read_u8()? {
            0x00 => false, // const
            0x01 => true,  // mutable
            c => return Err(ValidationError::ExpectedConstOrVar(c)),
        };

        Ok(GlobalType { val_type, mutable })
    }
}
