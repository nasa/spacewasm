use crate::util::Vec;
use crate::*;
use core::fmt::{Debug, Formatter};
use ::core::ops::ControlFlow;

pub struct GlobalValueError;

/// Error returned when a host name exceeds [`HOST_NAME_CAP`] bytes or is not
/// valid UTF-8.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostNameError;

/// A bounded, inline, owned host name (module / function / global / symbol).
#[derive(Clone, Copy)]
pub struct HostName<const CAPACITY: usize> {
    data: [u8; CAPACITY],
    len: u8,
}

impl<const CAPACITY: usize> HostName<CAPACITY> {
    const fn build(bytes: &[u8]) -> Option<HostName<CAPACITY>> {
        // Using an 8-bit length so we must validate the capacity fits in a u8.
        const { assert!(CAPACITY < 256) };

        if bytes.len() > CAPACITY {
            return None;
        }

        let mut data = [0u8; CAPACITY];
        let mut i = 0;
        while i < bytes.len() {
            data[i] = bytes[i];
            i += 1;
        }

        Some(HostName {
            data,
            len: bytes.len() as u8,
        })
    }

    /// Construct a host name from a string slice, panicking if it is longer
    /// than [`HOST_NAME_CAP`]. Intended for compile-time string literals in
    /// Rust code; use [`HostName::try_from_str`] on caller-supplied input.
    pub const fn new(s: &str) -> HostName<CAPACITY> {
        match HostName::build(s.as_bytes()) {
            Some(n) => n,
            None => panic!("host name exceeds HOST_NAME_CAP"),
        }
    }

    pub fn try_from_str(s: &str) -> Result<HostName<CAPACITY>, HostNameError> {
        HostName::build(s.as_bytes()).ok_or(HostNameError)
    }

    pub fn try_from_bytes(bytes: &[u8]) -> Result<HostName<CAPACITY>, HostNameError> {
        if core::str::from_utf8(bytes).is_err() {
            return Err(HostNameError);
        }
        HostName::build(bytes).ok_or(HostNameError)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.data[..self.len as usize]
    }

    pub fn as_str(&self) -> &str {
        // SAFETY: `data[..len]` is only ever populated from a `&str` (via
        // `new`/`try_from_str`) or from bytes validated as UTF-8 in
        // `try_from_bytes`.
        unsafe { core::str::from_utf8_unchecked(self.as_bytes()) }
    }
}

impl<const CAPACITY: usize> From<&str> for HostName<CAPACITY> {
    fn from(value: &str) -> Self {
        HostName::new(value)
    }
}

impl<const CAPACITY: usize> Debug for HostName<CAPACITY> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        Debug::fmt(self.as_str(), f)
    }
}

impl<const CAPACITY: usize> PartialEq<str> for HostName<CAPACITY> {
    fn eq(&self, other: &str) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

impl<const CAPACITY: usize> PartialEq<&str> for HostName<CAPACITY> {
    fn eq(&self, other: &&str) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

impl<const CAPACITY: usize> PartialEq<HostName<CAPACITY>> for str {
    fn eq(&self, other: &HostName<CAPACITY>) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

impl<const CAPACITY: usize> PartialEq<HostName<CAPACITY>> for &str {
    fn eq(&self, other: &HostName<CAPACITY>) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

pub type HostFunctionFn = Box<dyn Fn(&Engine, &[Value]) -> HostFunctionResult>;

pub trait GlobalValue {
    /// Write a value to this global variable.
    /// This will not be called if this value is not mutable.
    /// The value will always correspond to the [self.ty()] variant
    fn write(&self, value: Value) -> Result<(), GlobalValueError>;

    /// Read a global's value
    /// The value should always correspond to the [self.ty()] variant
    fn read(&self) -> Result<Value, GlobalValueError>;

    /// Global's type
    fn ty(&self) -> ValType;

    /// If a global is not mutable, [write] will not be called
    fn mutable(&self) -> bool;
}

pub const HOST_GLOBAL_NAME_CAP: usize = 15;

pub struct HostGlobal {
    pub name: HostName<HOST_GLOBAL_NAME_CAP>,
    pub value: Box<dyn GlobalValue>,
}

impl Debug for HostGlobal {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HostGlobal")
            .field("name", &self.name)
            .finish()
    }
}

impl<T: GlobalValue> Box<T> {
    pub fn into_global_value_dyn(mut self) -> Box<dyn GlobalValue>
    where
        T: GlobalValue + 'static,
    {
        let ptr = self.as_mut_ptr() as *mut dyn GlobalValue;
        core::mem::forget(self); // Prevent double free
        unsafe { Box::from_raw(GlobalAllocator, ptr) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostFunctionBreak {
    /// Halt execution due to an error
    Trap,

    /// Halt execution to perform work asynchronously
    Pause,
}

pub type HostFunctionResult = ControlFlow<HostFunctionBreak, Option<Value>>;

/// Maximum number of values in a host function parameter / result signature.
pub const HOST_SIGNATURE_CAP: usize = 63;

/// Error returned when a host value signature contains an invalid character or
/// exceeds [`HOST_SIGNATURE_CAP`] entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostValListError;

/// An owned, bounded list of [`ValType`] describing a host function's
/// parameters or results. Parsed from a signature string of the characters
/// `i` (i32), `I` (i64), `f` (f32), `d` (f64).
#[derive(Copy, Clone)]
pub struct HostValList {
    data: [ValType; HOST_SIGNATURE_CAP],
    len: u8,
}

impl HostValList {
    fn map_char(c: char) -> Result<ValType, HostValListError> {
        match c {
            'i' => Ok(ValType::I32),
            'I' => Ok(ValType::I64),
            'f' => Ok(ValType::F32),
            'd' => Ok(ValType::F64),
            _ => Err(HostValListError),
        }
    }

    /// Construct a signature list, panicking on an invalid or too-long
    /// signature. Intended for compile-time string literals in Rust code; use
    /// [`HostValList::try_new`] on caller-supplied input.
    pub fn new(s: &str) -> Self {
        HostValList::try_new(s).expect("invalid host value signature")
    }

    /// Fallibly construct a signature list. Returns an error if any character
    /// is not one of `iIfd` or the signature exceeds [`HOST_SIG_CAP`] entries.
    /// This is the FFI-safe constructor.
    pub fn try_new(s: &str) -> Result<Self, HostValListError> {
        let mut data = [ValType::I32; HOST_SIGNATURE_CAP];
        let mut len = 0usize;

        for c in s.chars() {
            if len >= HOST_SIGNATURE_CAP {
                return Err(HostValListError);
            }
            data[len] = HostValList::map_char(c)?;
            len += 1;
        }

        Ok(HostValList {
            data,
            len: len as u8,
        })
    }

    pub fn as_slice(&self) -> &[ValType] {
        &self.data[..self.len as usize]
    }

    pub fn iter(&self) -> HostValListIter {
        HostValListIter {
            index: 0,
            back: self.len as usize,
            data: *self,
        }
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl From<&str> for HostValList {
    fn from(value: &str) -> Self {
        HostValList::new(value)
    }
}

impl PartialEq<[ValType]> for HostValList {
    fn eq(&self, other: &[ValType]) -> bool {
        self.as_slice() == other
    }
}

pub struct HostValListIter {
    index: usize,
    back: usize,
    data: HostValList,
}

impl DoubleEndedIterator for HostValListIter {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.index >= self.back {
            return None;
        }

        self.back -= 1;
        Some(self.data.data[self.back])
    }
}

impl Iterator for HostValListIter {
    type Item = ValType;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.back {
            return None;
        }

        let c = self.data.data[self.index];
        self.index += 1;
        Some(c)
    }
}

impl<T: Fn(&Engine, &[Value]) -> HostFunctionResult> Box<T> {
    pub fn into_host_function_dyn(mut self) -> HostFunctionFn
    where
        T: Fn(&Engine, &[Value]) -> HostFunctionResult + 'static,
    {
        let ptr = self.as_mut_ptr() as *mut dyn Fn(&Engine, &[Value]) -> HostFunctionResult;
        core::mem::forget(self); // Prevent double free
        unsafe { Box::from_raw(GlobalAllocator, ptr) }
    }
}

pub const HOST_FUNCTION_NAME_CAP: usize = 31;

pub struct HostFunction {
    name: HostName<HOST_FUNCTION_NAME_CAP>,
    params: HostValList,
    returns: HostValList,
    f: HostFunctionFn,
}

impl Debug for HostFunction {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HostFunction")
            .field("name", &self.name)
            .field("params", &self.params.as_slice())
            .field("returns", &self.returns.as_slice())
            .finish()
    }
}

#[derive(Debug)]
pub struct HostSymbol<const NAME_CAPACITY: usize, T> {
    pub name: HostName<NAME_CAPACITY>,
    pub value: T,
}

pub const HOST_MODULE_NAME_CAP: usize = 31;

#[derive(Debug)]
pub struct HostModule {
    /// Module name
    pub name: HostName<HOST_MODULE_NAME_CAP>,
    pub globals: Vec<HostGlobal>,
    pub functions: Vec<HostFunction>,
    pub memory: Vec<HostSymbol<15, Rc<Memory>>>,
    pub table: Vec<HostSymbol<15, (Rc<[TableElement]>, Limit)>>,
}

impl HostFunction {
    /// Construct a host function. `name`, `params`, and `returns` are checked
    /// via the panicking [`HostName::new`] / [`HostValList::new`] constructors,
    /// so this is only appropriate for compile-time-known values in Rust code.
    /// FFI callers should validate input and use [`HostFunction::try_new`].
    pub fn new(
        name: impl Into<HostName<HOST_FUNCTION_NAME_CAP>>,
        params: HostValList,
        returns: HostValList,
        f: impl Fn(&Engine, &[Value]) -> HostFunctionResult + 'static,
    ) -> Self {
        HostFunction::try_new(name.into(), params, returns, f)
            .expect("host function signature too large")
    }

    /// Fallibly construct a host function, returning an error if the parameter
    /// or result signature is too large to encode. This is the FFI-safe
    /// constructor.
    pub fn try_new(
        name: HostName<HOST_FUNCTION_NAME_CAP>,
        params: HostValList,
        returns: HostValList,
        f: impl Fn(&Engine, &[Value]) -> HostFunctionResult + 'static,
    ) -> Result<Self, HostValListError> {
        let ps = params.iter().fold(0, |n, i| n + i.size()) / 4;
        if ps > 0xFFFF {
            return Err(HostValListError);
        }

        let rs = returns.iter().fold(0, |n, i| n + i.size()) / 4;
        if rs > 0xFFFF {
            return Err(HostValListError);
        }

        Ok(HostFunction {
            name,
            params,
            returns,
            f: Box::new(f).unwrap().into_host_function_dyn(),
        })
    }

    pub fn params(&self) -> HostValList {
        self.params
    }

    pub fn returns(&self) -> HostValList {
        self.returns
    }

    pub fn param_size(&self) -> usize {
        self.params.iter().fold(0, |n, i| n + i.size()) / 4
    }

    pub fn call(&self, state: &Engine, a: &[Value]) -> HostFunctionResult {
        (self.f)(state, a)
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }
}
