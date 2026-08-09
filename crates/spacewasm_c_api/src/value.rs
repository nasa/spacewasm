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

impl From<spacewasm_value_t> for Value {
    fn from(value: spacewasm_value_t) -> Self {
        // SAFETY: reading the union field that the tag designates as active.
        unsafe {
            match value.tag {
                spacewasm_valtype_t::SPACEWASM_I32 => Value::I32(value.u.i32_),
                spacewasm_valtype_t::SPACEWASM_I64 => Value::I64(value.u.i64_),
                spacewasm_valtype_t::SPACEWASM_F32 => Value::F32(value.u.f32_),
                spacewasm_valtype_t::SPACEWASM_F64 => Value::F64(value.u.f64_),
            }
        }
    }
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
    #[must_use]
    pub fn to_value(self) -> Value {
        self.into()
    }

    #[must_use]
    pub fn from_value(v: Value) -> spacewasm_value_t {
        v.into()
    }

    /// Interpret a [`RawValue`] as the given type and convert to a C value.
    #[must_use]
    pub fn from_raw(raw: RawValue, ty: ValType) -> spacewasm_value_t {
        raw.to_value(ty).into()
    }
}
