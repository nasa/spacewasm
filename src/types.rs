//! Methods to read Wasm Types from a [Reader] object.
//!
//! See: <https://webassembly.github.io/spec/core/binary/types.html>

use crate::util::String;
use crate::{Reader, ValidationError, Vec};

/// A pointer and length into a UTF-8 string on the original Wasm
pub struct Name;

impl Name {
    pub(crate) fn read(wasm: &mut Reader) -> Result<String, ValidationError> {
        wasm.read_vec(|r| r.read_u8())?.try_into()
    }
}

/// Value types classify the individual values that WebAssembly code can compute with and the values
/// that a variable accepts.
/// <https://www.w3.org/TR/wasm-core-1/#syntax-valtype>
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

/// A runtime type-tracked value
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

/// A compile-time/configured value
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawValue(u64);

impl RawValue {
    pub fn from_32(u: u32) -> RawValue {
        RawValue(u as u64)
    }

    pub fn from_64(u: u64) -> RawValue {
        RawValue(u)
    }

    pub fn from_i32(i: i32) -> RawValue {
        RawValue::from_32(i as u32)
    }

    pub fn from_i64(i: i64) -> RawValue {
        RawValue::from_64(i as u64)
    }

    pub fn from_f32(f: f32) -> RawValue {
        RawValue::from_32(f.to_bits())
    }

    pub fn from_f64(f: f64) -> RawValue {
        RawValue::from_64(f.to_bits())
    }

    pub fn write_32(&mut self, i: u32) {
        self.0 = i as u64;
    }

    pub fn write_64(&mut self, i: u64) {
        self.0 = i;
    }

    pub fn write_i32(&mut self, i: i32) {
        self.0 = i as u64;
    }

    pub fn write_i64(&mut self, i: i64) {
        self.0 = i as u64;
    }

    pub fn write_f32(&mut self, z: f32) {
        self.0 = z.to_bits() as u64;
    }

    pub fn write_f64(&mut self, z: f64) {
        self.0 = z.to_bits();
    }

    pub fn read_32(&self) -> u32 {
        self.0 as u32
    }

    pub fn read_64(&self) -> u64 {
        self.0
    }

    pub fn read_i32(&self) -> i32 {
        self.0 as i32
    }

    pub fn read_i64(&self) -> i64 {
        self.0 as i64
    }

    pub fn read_f32(&self) -> f32 {
        f32::from_bits(self.0 as u32)
    }

    pub fn read_f64(&self) -> f64 {
        f64::from_bits(self.0)
    }

    pub fn to_value(self, ty: ValType) -> Value {
        match ty {
            ValType::I32 => Value::I32((self.0 as u32) as i32),
            ValType::I64 => Value::I64(self.0 as i64),
            ValType::F32 => Value::F32(f32::from_bits(self.0 as u32)),
            ValType::F64 => Value::F64(f64::from_bits(self.0)),
        }
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

#[derive(Clone, PartialEq, Eq, Debug)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limit {
    pub min: u32,
    pub max: Option<u32>,
}

impl Limit {
    pub(crate) fn read(wasm: &mut Reader) -> Result<Self, ValidationError> {
        // Limits are encoded with a preceding flag indicating whether a maximum is present.
        match wasm.read_u8()? {
            0x00 => Ok(Limit {
                min: wasm.read_u32()?,
                max: None,
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
                    max: Some(max),
                })
            }
            c => Err(ValidationError::MalformedLimit(c)),
        }
    }

    pub fn matches(&self, other: &Limit) -> bool {
        if self.min < other.min {
            return false;
        }

        match (self.max, other.max) {
            (_, None) => true,
            (Some(m1), Some(m2)) => m1 <= m2,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemPageSize {
    _1,
    _65536,
}

impl MemPageSize {
    pub fn size(&self) -> usize {
        match self {
            MemPageSize::_1 => 1,
            MemPageSize::_65536 => 65536,
        }
    }

    pub fn alignment(&self) -> usize {
        match self {
            MemPageSize::_1 => 1,
            MemPageSize::_65536 => 16,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemType {
    pub initial_pages: u32,
    pub max_pages: Option<u32>,
    pub page_size: MemPageSize,
}

impl MemType {
    pub(crate) fn read(wasm: &mut Reader) -> Result<MemType, ValidationError> {
        let flag = wasm.read_u8()?;

        let min = wasm.read_u32()?;

        // Bit 0: Whether a maximum bound (m) for the limit follows.
        let max = if (flag & (1 << 0)) != 0 {
            let max = wasm.read_u32()?;
            if max < min {
                return Err(ValidationError::InvalidMaxLimit);
            }

            Some(max)
        } else {
            None
        };

        // Bit 1: Whether the memory is shared or unshared. This was introduced in the threads proposal.
        // Not supported
        if (flag & (1 << 1)) != 0 {
            return Err(ValidationError::MalformedMemType(flag));
        }

        // Bit 2: Whether the memory's index type is i32 or i64. This was introduced in the memory64 proposal.
        // Not supported
        if (flag & (1 << 2)) != 0 {
            return Err(ValidationError::MalformedMemType(flag));
        }

        // Bit 3: Whether the memory defines a custom page size (p) or not and therefore whether another u32 follows after the limits.
        // This was introduced in the custom-page-sizes proposal
        let page_size = if (flag & (1 << 3)) != 0 {
            let p = wasm.read_u32()?;
            match p {
                0 => MemPageSize::_1,
                16 => MemPageSize::_65536,
                p if p <= 64 => return Err(ValidationError::InvalidPageSize(p as u8)),
                _ => return Err(ValidationError::InvalidPageSize(0xFF)),
            }
        } else {
            MemPageSize::_65536
        };

        // Mask the rest of the flag bits to validate they are not set
        if (flag & 0xF0) != 0 {
            return Err(ValidationError::MalformedMemType(flag));
        }

        // The limits must be valid within the range 2**32 - 1.
        // The limits must be valid within the range 2**32 / pagesize

        let max_allowed_pages = match page_size {
            MemPageSize::_1 => (u32::MAX as u64) + 1,
            MemPageSize::_65536 => 65536,
        };

        if min as u64 > max_allowed_pages {
            return Err(ValidationError::MemoryTooLarge);
        } else if let Some(max) = max {
            if max as u64 > max_allowed_pages {
                return Err(ValidationError::MemoryTooLarge);
            }
        }

        Ok(MemType {
            initial_pages: min,
            max_pages: max,
            page_size,
        })
    }

    pub fn zero() -> MemType {
        MemType {
            initial_pages: 0,
            max_pages: Some(0),
            page_size: MemPageSize::_65536,
        }
    }

    pub fn min(&self) -> u32 {
        self.initial_pages
    }

    pub fn can_hold(&self, n_pages: u32) -> bool {
        if let Some(max) = self.max_pages {
            if n_pages > max {
                return false;
            }
        } else {
            // Wasm only has 4 GiB per memory
            let n_bytes = (n_pages as u64) * (self.page_size() as u64);
            if n_bytes > (1 << 32) {
                return false;
            }
        }

        self.initial_pages <= n_pages
    }

    pub fn page_size(&self) -> usize {
        self.page_size.size()
    }

    pub fn page_alignment(&self) -> usize {
        self.page_size.alignment()
    }

    pub fn matches(&self, other: &MemType) -> bool {
        Limit {
            min: self.initial_pages,
            max: self.max_pages,
        }
        .matches(&Limit {
            min: other.initial_pages,
            max: other.max_pages,
        }) && self.page_size == other.page_size
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy)]
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
