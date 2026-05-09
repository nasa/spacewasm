use crate::util::Vec;
use crate::{AllocError, Box, Func, HostFunction, HostModule, Module};

pub enum StoreModule {
    Host(HostModule),
    Module(Module),
}

impl StoreModule {
    pub fn name(&self) -> &str {
        match self {
            StoreModule::Host(m) => m.name,
            StoreModule::Module(m) => &m.name,
        }
    }
}

pub struct Store(pub Vec<Box<StoreModule>>);

pub enum StoreFunction<'store> {
    Host(HostFunction),
    Function {
        /// The parent module
        module: &'store Module,
        /// The wasm function
        func: &'store Func,
    },
}

impl Store {
    pub fn new(max_modules: usize) -> Result<Self, AllocError> {
        Ok(Store(Vec::new(max_modules as u32)?))
    }
}
