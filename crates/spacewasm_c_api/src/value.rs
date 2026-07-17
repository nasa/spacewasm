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

impl From<spacewasm_valtype_t> for ValType {
    fn from(v: spacewasm_valtype_t) -> Self {
        match v {
            spacewasm_valtype_t::SPACEWASM_I32 => ValType::I32,
            spacewasm_valtype_t::SPACEWASM_I64 => ValType::I64,
            spacewasm_valtype_t::SPACEWASM_F32 => ValType::F32,
            spacewasm_valtype_t::SPACEWASM_F64 => ValType::F64,
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

impl spacewasm_value_t {
    /// Convert a C value into a [`spacewasm::Value`], reading the payload field
    /// selected by `tag`.
    pub fn to_value(self) -> Value {
        // SAFETY: reading the union field that the tag designates as active.
        unsafe {
            match self.tag {
                spacewasm_valtype_t::SPACEWASM_I32 => Value::I32(self.u.i32_),
                spacewasm_valtype_t::SPACEWASM_I64 => Value::I64(self.u.i64_),
                spacewasm_valtype_t::SPACEWASM_F32 => Value::F32(self.u.f32_),
                spacewasm_valtype_t::SPACEWASM_F64 => Value::F64(self.u.f64_),
            }
        }
    }

    /// Convert a [`spacewasm::Value`] into a C value.
    pub fn from_value(v: Value) -> spacewasm_value_t {
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

    /// Interpret a [`RawValue`] as the given type and convert to a C value.
    pub fn from_raw(raw: RawValue, ty: ValType) -> spacewasm_value_t {
        spacewasm_value_t::from_value(raw.to_value(ty))
    }
}
