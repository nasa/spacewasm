extern crate std;
use core::ops::ControlFlow;

use crate::{Box, ImportDesc, Module, Reader, ValType, ValidationError, Value};

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

pub struct GlobalImport<'imports> {
    pub module: &'imports str,
    pub name: &'imports str,
    pub value: Box<dyn GlobalValue>,
}

impl<'imports> GlobalImport<'imports> {
    pub fn matches(&self, module: &str, name: &str) -> bool {
        self.module == module && self.name == name
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostFunctionPause {
    /// Halt execution due to an error
    Trap,

    /// Halt execution to perform work asynchronously
    Pause,
}

pub type HostFunctionResult = ControlFlow<HostFunctionPause, Option<Value>>;

pub struct HostFunction<'imports> {
    module: &'imports str,
    name: &'imports str,
    params: &'imports [ValType],
    returns: &'imports [ValType],
    f: fn(&[Value]) -> HostFunctionResult,

    param_size: u16,
    return_size: u16,
}

impl<'imports> HostFunction<'imports> {
    pub fn new(
        module: &'imports str,
        name: &'imports str,
        params: &'imports [ValType],
        returns: &'imports [ValType],
        f: fn(a: &[Value]) -> HostFunctionResult,
    ) -> Self {
        let mut o = HostFunction {
            module,
            name,
            params,
            returns,
            f,
            param_size: 0,
            return_size: 0,
        };

        let ps = o.params.iter().fold(0, |n, i| n + i.size());
        assert!(ps <= 0xFFFF);
        o.param_size = ps as u16;

        let rs = o.returns.iter().fold(0, |n, i| n + i.size());
        assert!(rs <= 0xFFFF);
        o.return_size = rs as u16;

        o
    }

    pub fn params(&self) -> &'imports [ValType] {
        self.params
    }

    pub fn param_size(&self) -> usize {
        self.param_size as usize
    }

    pub fn call(&self, a: &[Value]) -> HostFunctionResult {
        (self.f)(a)
    }

    pub fn matches(&self, module: &str, name: &str) -> bool {
        self.module == module && self.name == name
    }
}

pub struct MemoryImport<'imports> {
    pub module: &'imports str,
    pub name: &'imports str,
    pub data: &'imports mut [u8],
}

impl<'imports> MemoryImport<'imports> {
    pub fn matches(&self, module: &str, name: &str) -> bool {
        self.module == module && self.name == name
    }
}

pub struct ModuleImports<'imports> {
    pub globals: &'imports [GlobalImport<'imports>],
    pub functions: &'imports [HostFunction<'imports>],
    pub memories: &'imports [MemoryImport<'imports>],
}

pub enum Import {
    Func(u16),
    Table(u16),
    Mem(u16),
    Global(u16),
}

impl Import {
    pub fn read(wasm: &mut Reader, module: &Module<'_>) -> Result<Import, ValidationError> {
        let module_raw = wasm.read_vec_stack::<32, _>(|r| r.read_u8())?;
        let module_name = (&module_raw).try_into()?;

        let name_raw = wasm.read_vec_stack::<32, _>(|r| r.read_u8())?;
        let name = (&name_raw).try_into()?;

        let desc = ImportDesc::read(wasm)?;

        // Look up this import given its name/module
        match desc {
            ImportDesc::Func(f) => {
                // Look up the function type from the WASM module
                let ty = module
                    .types
                    .get(f.0 as usize)
                    .ok_or(ValidationError::FunctionImportOutOfRange)?;

                let wasm_params = &ty.params[..];
                let wasm_returns = &ty.returns[..];

                // Look up the global import that matches the module name and symbol name
                std::eprintln!("function import {module_name}::{name}");
                let (index, function_import) = module
                    .module_imports
                    .functions
                    .iter()
                    .enumerate()
                    .find_map(|(i, fi)| {
                        if fi.matches(module_name, name) {
                            Some((i, fi))
                        } else {
                            None
                        }
                    })
                    .ok_or(ValidationError::FunctionImportNotFound)?;

                // Validate the WASM type against the embedder's type
                if function_import.params == wasm_params && function_import.returns == wasm_returns
                {
                    Ok(Import::Func(index as u16))
                } else {
                    std::eprintln!(
                        "import {module_name}::{name} params: {:?}, returns {:?}",
                        function_import.params,
                        function_import.returns
                    );
                    std::eprintln!(
                        "wasm expected params: {:?}, returns {:?}",
                        wasm_params,
                        wasm_returns
                    );
                    Err(ValidationError::FunctionImportTypeMismatch)
                }
            }
            ImportDesc::Table(_) => Err(ValidationError::TableImportsNotSupportedYet),
            ImportDesc::Mem(_) => Err(ValidationError::MemoryImportsNotSupportedYet),
            ImportDesc::Global(g_ty) => {
                // Look up the global import that matches the module name and symbol name
                let (index, global_import) = module
                    .module_imports
                    .globals
                    .iter()
                    .enumerate()
                    .find_map(|(i, gi)| {
                        if gi.matches(module_name, name) {
                            Some((i, gi))
                        } else {
                            None
                        }
                    })
                    .ok_or(ValidationError::GlobalImportNotFound)?;

                // Validate the imported type matches the global type defined here
                if global_import.value.ty() != g_ty.ty {
                    return Err(ValidationError::GlobalImportTypeMismatch);
                } else if !global_import.value.mutable() && g_ty.mutable {
                    return Err(ValidationError::GlobalIsNotMutable);
                }

                Ok(Import::Global(index as u16))
            }
        }
    }
}
