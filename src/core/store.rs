use crate::util::Vec;
use crate::{AllocError, Box, HostModule, Module};

pub struct Store {
    pub modules: Vec<Box<Module>>,
    pub host_modules: Vec<HostModule>,
}

impl Store {
    pub fn new<const N: usize>(
        max_modules: usize,
        host_modules: [HostModule; N],
    ) -> Result<Self, AllocError> {
        Ok(Store {
            modules: Vec::new(max_modules as u32)?,
            host_modules: Vec::from_array(host_modules)?,
        })
    }
}
