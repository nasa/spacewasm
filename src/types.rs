//! Methods to read WASM Types from a [WasmReader] object.
//!
//! See: <https://webassembly.github.io/spec/core/binary/types.html>

use crate::{ValidationError, Vec, WasmReader, WasmReaderState};

/// An offset and length to select a subset of the WASM binary
#[repr(C)]
pub struct Slice {
    pub start: WasmReaderState,
    pub len: u32,
}

impl Slice {
    pub fn read(wasm: &mut WasmReader, len: usize) -> Result<Slice, ValidationError> {
        let start = wasm.save();

        // Make sure we can read the entire slice
        let _ = wasm.read_n(len)?;

        Ok(Slice {
            start,
            len: len as u32,
        })
    }

    /// Dereference the string name from the WASM binary
    /// Safety note: `wasm` MUST point to the same memory that was used to construct this name
    pub fn deref<'wasm>(&self, mut wasm: WasmReader<'wasm>) -> &'wasm [u8] {
        wasm.restore(self.start);
        wasm.read_n(self.len as usize).unwrap()
    }
}

/// A pointer and length into a UTF-8 string on the original WASM
#[repr(C)]
pub struct Name(Slice);

impl Name {
    pub fn read(wasm: &mut WasmReader) -> Result<Name, ValidationError> {
        let len = wasm.read_u32()? as usize;

        // Read the string and validate the utf-8 characters
        let start = wasm.save();
        let data = wasm.read_n(len)?;

        match core::str::from_utf8(data) {
            Ok(_) => Ok(Name(Slice {
                start,
                len: len as u32,
            })),
            Err(err) => Err(ValidationError::MalformedUtf8(err)),
        }
    }

    /// Dereference the string name from the WASM binary
    /// Safety note: `wasm` MUST point to the same memory that was used to construct this name
    pub fn deref<'wasm>(&self, wasm: WasmReader<'wasm>) -> &'wasm str {
        // This has already been checked for validity ahead of time
        // We could just unwrap here though it's unnecessary.
        unsafe { core::str::from_utf8_unchecked(self.0.deref(wasm)) }
    }
}

/// Value types classify the individual values that WebAssembly code can compute with and the values
/// that a variable accepts.
/// https://www.w3.org/TR/wasm-core-1/#syntax-valtype
#[derive(Copy, Clone, PartialEq, Eq)]
#[repr(C)]
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

    pub fn read(wasm: &mut WasmReader) -> Result<Self, ValidationError> {
        ValType::convert(wasm.read_u8()?)
    }
}

#[derive(Clone, PartialEq, Eq)]
#[repr(C)]
pub struct ResultType(pub Option<ValType>);

impl ResultType {
    pub fn read(wasm: &mut WasmReader) -> Result<Self, ValidationError> {
        // The only result types occurring in the binary format are the types of blocks.
        // These are encoded in special compressed form, by either the byte 0x40 indicating
        // the empty type or as a single value type.
        match wasm.read_u8()? {
            0x40 => Ok(ResultType(None)),
            c => ValType::convert(c).map(|v| ResultType(Some(v))),
        }
    }
}

#[repr(C)]
pub struct FuncType {
    pub params: Vec<ValType>,
    pub results: Vec<ValType>,
}

impl FuncType {
    pub fn read(wasm: &mut WasmReader) -> Result<Self, ValidationError> {
        // Function types are encoded by the byte 0x60 followed by the respective vectors of parameter and result types.
        match wasm.read_u8()? {
            0x60 => {
                let params = wasm.read_vec(ValType::read)?;
                let results = wasm.read_vec(ValType::read)?;

                Ok(FuncType { params, results })
            }
            c => Err(ValidationError::MalformedFunction(c)),
        }
    }
}

pub struct Limit {
    pub min: u32,
    pub max: Option<u32>,
}

impl Limit {
    pub fn read(wasm: &mut WasmReader) -> Result<Self, ValidationError> {
        // Limits are encoded with a preceding flag indicating whether a maximum is present.
        match wasm.read_u8()? {
            0x00 => Ok(Limit {
                min: wasm.read_u32()?,
                max: None,
            }),
            0x01 => Ok(Limit {
                min: wasm.read_u32()?,
                max: Some(wasm.read_u32()?),
            }),
            c => Err(ValidationError::MalformedLimit(c)),
        }
    }
}

pub struct MemType(pub Limit);

impl MemType {
    pub fn read(wasm: &mut WasmReader) -> Result<Self, ValidationError> {
        // Memory types are encoded with their limits.
        Ok(MemType(Limit::read(wasm)?))
    }
}

pub enum ElemType {
    FuncRef,
}

impl ElemType {
    pub fn read(wasm: &mut WasmReader) -> Result<Self, ValidationError> {
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
    pub fn read(wasm: &mut WasmReader) -> Result<Self, ValidationError> {
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
    pub fn read(wasm: &mut WasmReader) -> Result<Self, ValidationError> {
        let val_type = ValType::read(wasm)?;
        let mutable = match wasm.read_u8()? {
            0x00 => false, // const
            0x01 => true,  // mutable
            c => return Err(ValidationError::ExpectedConstOrVar(c)),
        };

        Ok(GlobalType { val_type, mutable })
    }
}
