use core::cell::RefCell;

use crate::util::Vec;
use crate::{
    AllocError, Box, HostModule, Memory, MemoryError, MemoryKind, Module, WasmMemoryAllocator,
};

pub struct StoreLinker {
    /// The modules we have initialized so far
    pub modules: Vec<Box<Module>>,
    /// The host modules of the store
    pub host_modules: Vec<HostModule>,
}

/// Holds ownership of all the loaded modules. As new modules are loaded,
/// imports/exports are referenced through the store.
pub struct Store {
    pub modules: Vec<Box<Module>>,
    pub host_modules: Vec<HostModule>,
    pub memory: Vec<RefCell<Memory>>,
}

impl StoreLinker {
    pub fn new<const N: usize>(
        max_modules: usize,
        host_modules: [HostModule; N],
    ) -> Result<Self, AllocError> {
        Ok(StoreLinker {
            modules: Vec::new(max_modules as u32)?,
            host_modules: Vec::from_array(host_modules)?,
        })
    }

    /// This function must be called _after_ all modules are
    pub fn finish(self, allocator: &'static dyn WasmMemoryAllocator) -> Result<Store, MemoryError> {
        // Count the owned memories in the entire store
        let total_memories = self
            .modules
            .iter()
            .filter(|m| match m.memory {
                Some(MemoryKind::Allocate { .. }) => true,
                _ => false,
            })
            .count();

        // Allocate some space to hold all the memories
        let mut memory = Vec::new(total_memories as u32)?;

        // Initialize the memory and fill up the data
        for module in &self.modules {
            let mut linear_memory = match &module.memory {
                Some(MemoryKind::Allocate { ty, index }) => {
                    // Make sure the module construction is pointing to the index we expect
                    assert_eq!(*index as usize, memory.len());
                    memory.push(RefCell::new(Memory::new(*ty, allocator)?));
                    memory.last().unwrap().borrow_mut()
                }
                Some(MemoryKind::Import(mem_idx)) => {
                    // Make sure this memory index is valid
                    assert!((*mem_idx as usize) < memory.len());
                    memory.get(*mem_idx as usize).unwrap().borrow_mut()
                }
                None => {
                    assert!(module.data.is_empty());
                    continue;
                }
            };

            // Initialize the data into linear memory
            for data in &module.data {
                linear_memory.store(data.offset as usize, &data.init)?;
            }
        }

        Ok(Store {
            modules: self.modules,
            host_modules: self.host_modules,
            memory,
        })
    }
}

impl Module {
    /// Pull out the memory for this module from the store
    pub fn take_memory(&self, store: &Store) -> Memory {
        match &self.memory {
            Some(MemoryKind::Allocate { index, .. } | MemoryKind::Import(index)) => {
                core::mem::take(&mut store.memory[*index as usize].borrow_mut())
            }
            None => Memory::zero(),
        }
    }

    /// Return memory back to the store
    pub fn return_memory(&self, store: &Store, m: Memory) {
        match &self.memory {
            Some(MemoryKind::Allocate { index, .. } | MemoryKind::Import(index)) => {
                let old = core::mem::replace(&mut *store.memory[*index as usize].borrow_mut(), m);
                assert_eq!(old.size(), 0);
            }
            None => {}
        }
    }
}
