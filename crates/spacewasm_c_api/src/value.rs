//! FFI-safe value marshalling between C and [`spacewasm::Value`].

use spacewasm::{RawValue, ValType, Value};

/// FFI-safe value type tag. Matches the ordering of [`spacewasm::ValType`].
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum spacewasm_valtype_t {
    SPACEWASM_I32 = 0,
    SPACEWASM_I64 = 1,
    SPACEWASM_F32 = 2,
    SPACEWASM_F64 = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidValtypeTag;

impl TryFrom<&spacewasm_valtype_t> for ValType {
    type Error = InvalidValtypeTag;

    fn try_from(tag: &spacewasm_valtype_t) -> Result<Self, Self::Error> {
        // SAFETY: `spacewasm_valtype_t` is `#[repr(u8)]`
        let raw = unsafe { *(tag as *const spacewasm_valtype_t).cast::<u8>() };
        match raw {
            0 => Ok(ValType::I32),
            1 => Ok(ValType::I64),
            2 => Ok(ValType::F32),
            3 => Ok(ValType::F64),
            _ => Err(InvalidValtypeTag),
        }
    }
}

impl From<ValType> for spacewasm_valtype_t {
    fn from(v: ValType) -> Self {
        match v {
            ValType::I32 => spacewasm_valtype_t::SPACEWASM_I32,
            ValType::I64 => spacewasm_valtype_t::SPACEWASM_I64,
            ValType::F32 => spacewasm_valtype_t::SPACEWASM_F32,
            ValType::F64 => spacewasm_valtype_t::SPACEWASM_F64,
        }
    }
}

/// FFI-safe union of the four WebAssembly 1.0 value payloads.
#[repr(C)]
#[derive(Clone, Copy)]
pub union spacewasm_value_payload_t {
    pub i32_: i32,
    pub i64_: i64,
    pub f32_: f32,
    pub f64_: f64,
}

/// FFI-safe tagged value. `tag` selects the active `u` field.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct spacewasm_value_t {
    pub tag: spacewasm_valtype_t,
    pub u: spacewasm_value_payload_t,
}

impl From<Value> for spacewasm_value_t {
    fn from(v: Value) -> Self {
        match v {
            Value::I32(x) => spacewasm_value_t {
                tag: spacewasm_valtype_t::SPACEWASM_I32,
                u: spacewasm_value_payload_t { i32_: x },
            },
            Value::I64(x) => spacewasm_value_t {
                tag: spacewasm_valtype_t::SPACEWASM_I64,
                u: spacewasm_value_payload_t { i64_: x },
            },
            Value::F32(x) => spacewasm_value_t {
                tag: spacewasm_valtype_t::SPACEWASM_F32,
                u: spacewasm_value_payload_t { f32_: x },
            },
            Value::F64(x) => spacewasm_value_t {
                tag: spacewasm_valtype_t::SPACEWASM_F64,
                u: spacewasm_value_payload_t { f64_: x },
            },
        }
    }
}

impl spacewasm_value_t {
    /// Validated conversion of a value received from C into a core [`Value`].
    ///
    /// Returns `None` when `tag` is not valid
    pub fn try_to_value(&self) -> Option<Value> {
        let ty = ValType::try_from(&self.tag).ok()?;
        // SAFETY: the union field read is selected by the validated tag; every
        // bit pattern is a valid value of the corresponding scalar type.
        Some(unsafe {
            match ty {
                ValType::I32 => Value::I32(self.u.i32_),
                ValType::I64 => Value::I64(self.u.i64_),
                ValType::F32 => Value::F32(self.u.f32_),
                ValType::F64 => Value::F64(self.u.f64_),
            }
        })
    }

    pub fn from_value(v: Value) -> spacewasm_value_t {
        v.into()
    }

    /// Interpret a [`RawValue`] as the given type and convert to a C value.
    pub fn from_raw(raw: RawValue, ty: ValType) -> spacewasm_value_t {
        raw.to_value(ty).into()
    }
}
