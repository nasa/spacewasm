//! Methods to read WASM Types from a [Reader] object.
//!
//! See: <https://webassembly.github.io/spec/core/binary/types.html>

use crate::util::String;
use crate::{Reader, ValidationError, Vec};

/// A pointer and length into a UTF-8 string on the original WASM
pub struct Name;

impl Name {
    pub(crate) fn read(wasm: &mut Reader) -> Result<String, ValidationError> {
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

impl From<u8> for ValType {
    fn from(val: u8) -> Self {
        #[cfg(feature = "strict-assertions")]
        match val {
            0 => ValType::I32,
            1 => ValType::I64,
            2 => ValType::F32,
            3 => ValType::F64,
            _ => unreachable!(),
        }

        #[cfg(not(feature = "strict-assertions"))]
        unsafe {
            core::mem::transmute(val)
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Value {
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
}

impl ValType {
    pub fn size(&self) -> usize {
        match self {
            ValType::I32 => 4,
            ValType::I64 => 8,
            ValType::F32 => 4,
            ValType::F64 => 8,
        }
    }

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

    pub(crate) fn read(wasm: &mut Reader) -> Result<Self, ValidationError> {
        ValType::convert(wasm.read_u8()?)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResultType(pub Option<ValType>);

impl ResultType {
    pub(crate) fn read(wasm: &mut Reader) -> Result<Self, ValidationError> {
        // The only result types occurring in the binary format are the types of blocks.
        // These are encoded in special compressed form, by either the byte 0x40 indicating
        // the empty type or as a single value type.
        match wasm.read_u8()? {
            0x40 => Ok(ResultType(None)),
            c => ValType::convert(c).map(|v| ResultType(Some(v))),
        }
    }
}

#[derive(PartialEq, Eq, Debug)]
pub struct FuncType {
    pub params: Vec<ValType>,
    pub returns: Vec<ValType>,
}

impl FuncType {
    pub(crate) fn read(wasm: &mut Reader) -> Result<Self, ValidationError> {
        // Function types are encoded by the byte 0x60 followed by the respective
        // vectors of parameter and result types.
        match wasm.read_u8()? {
            0x60 => {
                let params = wasm.read_vec(ValType::read)?;
                let returns = wasm.read_vec(ValType::read)?;

                if returns.len() > 1 {
                    // Wasm 1.0 does not support multiple returns
                    Err(ValidationError::FunctionReturnsTooLarge)
                } else {
                    Ok(FuncType { params, returns })
                }
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
    pub(crate) fn read(wasm: &mut Reader) -> Result<Self, ValidationError> {
        // Limits are encoded with a preceding flag indicating whether a maximum is present.
        match wasm.read_u8()? {
            0x00 => Ok(Limit {
                min: wasm.read_u32()?,
                // max: None,
            }),
            0x01 => {
                let min = wasm.read_u32()?;

                // Note: We are disabling `max` memory size since we don't support memory.grow
                let max = wasm.read_u32()?;
                if max < min {
                    return Err(ValidationError::InvalidMaxLimit);
                }

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
    pub(crate) fn read(wasm: &mut Reader) -> Result<Self, ValidationError> {
        // Memory types are encoded with their limits.
        Ok(MemType(Limit::read(wasm)?))
    }
}

pub enum ElemType {
    FuncRef,
}

impl ElemType {
    pub(crate) fn read(wasm: &mut Reader) -> Result<Self, ValidationError> {
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
    pub(crate) fn read(wasm: &mut Reader) -> Result<Self, ValidationError> {
        // Table types are encoded with their limits and a constant byte indicating their element type.
        Ok(TableType {
            elem_type: ElemType::read(wasm)?,
            limits: Limit::read(wasm)?,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct GlobalType {
    pub ty: ValType,
    pub mutable: bool,
}

impl GlobalType {
    pub(crate) fn read(wasm: &mut Reader) -> Result<Self, ValidationError> {
        let ty = ValType::read(wasm)?;
        let mutable = match wasm.read_u8()? {
            0x00 => false, // const
            0x01 => true,  // mutable
            c => return Err(ValidationError::ExpectedConstOrVar(c)),
        };

        Ok(GlobalType { ty, mutable })
    }
}
