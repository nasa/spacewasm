use crate::util::Vec;
use crate::*;
use ::core::ops::ControlFlow;

pub struct GlobalValueError;

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

pub struct HostGlobal {
    pub name: &'static str,
    pub value: Box<dyn GlobalValue>,
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

#[derive(Copy, Clone)]
pub struct HostValList(&'static str);

impl HostValList {
    pub fn new(s: &'static str) -> Self {
        // Validate input string
        for c in s.chars() {
            match c {
                'i' | 'I' | 'f' | 'd' => {}
                _ => assert!(false, "invalid host value signature"),
            }
        }

        HostValList(s)
    }

    pub fn iter(&self) -> HostValListIter {
        HostValListIter {
            index: 0,
            data: self.0,
        }
    }
}

impl From<&'static str> for HostValList {
    fn from(value: &'static str) -> Self {
        HostValList::new(value)
    }
}

impl PartialEq<[ValType]> for HostValList {
    fn eq(&self, other: &[ValType]) -> bool {
        if self.0.len() != other.len() {
            return false;
        }

        for (c, o) in self.iter().zip(other.iter()) {
            if c != *o {
                return false;
            }
        }

        true
    }
}

pub struct HostValListIter {
    index: usize,
    data: &'static str,
}

impl Iterator for HostValListIter {
    type Item = ValType;

    fn next(&mut self) -> Option<Self::Item> {
        let c = match self.data.chars().nth(self.index)? {
            'i' => ValType::I32,
            'I' => ValType::I64,
            'f' => ValType::F32,
            'd' => ValType::F64,
            _ => unreachable!(),
        };

        self.index += 1;
        Some(c)
    }
}

impl<T: Fn(&mut InterpreterState, &[Value]) -> HostFunctionResult> Box<T> {
    pub fn into_host_function_dyn(
        mut self,
    ) -> Box<dyn Fn(&mut InterpreterState, &[Value]) -> HostFunctionResult>
    where
        T: Fn(&mut InterpreterState, &[Value]) -> HostFunctionResult + 'static,
    {
        let ptr =
            self.as_mut_ptr() as *mut dyn Fn(&mut InterpreterState, &[Value]) -> HostFunctionResult;
        core::mem::forget(self); // Prevent double free
        unsafe { Box::from_raw(GlobalAllocator, ptr) }
    }
}

pub struct HostFunction{
    name: &'static str,
    params: HostValList,
    returns: HostValList,
    f: Box<dyn Fn(&mut InterpreterState, &[Value]) -> HostFunctionResult>,
    param_size: u16,
    return_size: u16,
}

pub struct HostModule {
    /// Module name
    pub name: &'static str,
    pub globals: Vec<HostGlobal>,
    pub functions: Vec<HostFunction>,
}

impl HostFunction {
    pub fn new(
        name: &'static str,
        params: HostValList,
        returns: HostValList,
        f: impl Fn(&mut InterpreterState, &[Value]) -> HostFunctionResult + 'static,
    ) -> Self {
        let mut o = HostFunction {
            name,
            params,
            returns,
            f: Box::new(f).unwrap().into_host_function_dyn(),
            param_size: 0,
            return_size: 0,
        };

        let ps = o.params.iter().fold(0, |n, i| n + i.size()) / 4;
        assert!(ps <= 0xFFFF);
        o.param_size = ps as u16;

        let rs = o.returns.iter().fold(0, |n, i| n + i.size()) / 4;
        assert!(rs <= 0xFFFF);
        o.return_size = rs as u16;

        o
    }

    pub fn params(&self) -> HostValList {
        self.params
    }

    pub fn returns(&self) -> HostValList {
        self.returns
    }

    pub fn param_size(&self) -> usize {
        self.param_size as usize
    }

    pub fn call(&self, state: &mut InterpreterState, a: &[Value]) -> HostFunctionResult {
        (self.f)(state, a)
    }

    pub fn name(&self) -> &'static str {
        self.name
    }
}
