use crate::*;

/// Holds ownership of all the loaded modules. As new modules are loaded,
/// imports/exports are referenced through the store.
#[derive(Debug)]
pub struct Store {
    modules: Vec<Module>,
    host_modules: Vec<HostModule>,
    zero_memory: Rc<Memory>,
    zero_table: Rc<[TableElement]>,
}

impl Store {
    /// Create a store from host modules built at runtime, with room for
    /// `max_modules` (<= 256) guest modules.
    pub fn from_host_modules(
        max_modules: usize,
        host_modules: Vec<HostModule>,
    ) -> Result<Self, AllocError> {
        if max_modules > 256 {
            return Err(AllocError::OutOfMemory);
        }

        Ok(Store {
            modules: Vec::new(max_modules)?,
            host_modules,
            zero_memory: Rc::new(Memory::zero())?,
            zero_table: Rc::new_slice_with_default(0)?,
        })
    }

    #[inline(always)]
    pub fn modules(&self) -> &[Module] {
        &self.modules
    }

    #[inline(always)]
    pub fn modules_mut(&mut self) -> &mut [Module] {
        &mut self.modules
    }

    #[inline(always)]
    pub fn host_modules(&self) -> &[HostModule] {
        &self.host_modules
    }

    #[inline(always)]
    pub fn host_modules_mut(&mut self) -> &mut [HostModule] {
        &mut self.host_modules
    }

    /// Remove the last module from the store if it exists
    /// Returns the removed module, or None if the store is empty
    #[inline(always)]
    pub fn pop_module(&mut self) -> Option<Module> {
        self.modules.pop()
    }

    /// Append a module. [`AllocError::OutOfMemory`] if the store is full.
    #[inline(always)]
    pub fn push_module(&mut self, module: Module) -> Result<ModuleRef, AllocError> {
        self.modules.try_push(module)?;
        Ok(ModuleRef((self.modules.len() - 1) as u8))
    }

    pub fn get_memory(&self, module_ref: ModuleRef) -> &Rc<Memory> {
        match &self.modules[module_ref.0 as usize].memory {
            None => &self.zero_memory,
            Some(MemoryKind::Owned(mem)) => mem,
            Some(MemoryKind::Import(import_module_ref)) => {
                let r = import_module_ref.0 as usize;
                let Some(MemoryKind::Owned(mem)) = &self.modules[r].memory else {
                    unreachable!()
                };

                mem
            }
            Some(MemoryKind::ImportHost(host_import)) => {
                &self.host_modules[host_import.module.0 as usize].memory[host_import.index as usize]
                    .value
            }
        }
    }

    pub fn get_table(&self, module_ref: ModuleRef) -> &Rc<[TableElement]> {
        match &self.modules[module_ref.0 as usize].table {
            None => &self.zero_table,
            Some(TableKind::Owned(table)) => &table.0,
            Some(TableKind::Import(import_module_ref)) => {
                let r = import_module_ref.0 as usize;
                let Some(TableKind::Owned(table)) = &self.modules[r].table else {
                    unreachable!()
                };

                &table.0
            }
            Some(TableKind::ImportHost(host_import)) => {
                &self.host_modules[host_import.module.0 as usize].table[host_import.index as usize]
                    .value
                    .0
            }
        }
    }

    /// A module's linear memory. A module without one gets the shared zero-memory
    /// sentinel: check [`Memory::is_zero`] before mutating.
    pub fn get_memory_mut(&mut self, module_ref: ModuleRef) -> &mut Rc<Memory> {
        match &self.modules[module_ref.0 as usize].memory {
            None => &mut self.zero_memory,
            Some(MemoryKind::Owned(_)) => {
                let Some(MemoryKind::Owned(mem)) = &mut self.modules[module_ref.0 as usize].memory
                else {
                    unreachable!()
                };
                mem
            }
            Some(MemoryKind::Import(import_module_ref)) => {
                let r = import_module_ref.0 as usize;
                let Some(MemoryKind::Owned(mem)) = &mut self.modules[r].memory else {
                    unreachable!()
                };

                mem
            }
            Some(MemoryKind::ImportHost(host_import)) => {
                &mut self.host_modules[host_import.module.0 as usize].memory
                    [host_import.index as usize]
                    .value
            }
        }
    }
}

impl Engine {
    pub fn new(
        stack_size: usize,
        max_modules: usize,
        host_modules: Vec<HostModule>,
    ) -> Result<Engine, MemoryError> {
        let store = Store::from_host_modules(max_modules, host_modules)?;

        Ok(Engine {
            pc: JumpTarget::SENTINEL,
            sp: 0x0,
            fp: 0x0,
            stack: Stack::new(stack_size)?,
            memory: store.zero_memory.clone(),
            table: store.zero_table.clone(),
            jumped: false,
            module: ModuleRef(0),
            store,
            result: None,
            host_pause: None,
            trap_pc: None,
            trap_fp: 0,
        })
    }

    pub fn reset(&mut self) {
        self.pc = JumpTarget::SENTINEL;
        self.sp = 0;
        self.fp = 0;
        self.jumped = false;
        self.result = None;
        self.host_pause = None;
        self.trap_pc = None;
        self.trap_fp = 0;
        self.clear_memory();
        self.clear_table();
    }

    pub fn clear_memory(&mut self) {
        self.memory = self.store.zero_memory.clone();
    }

    pub fn clear_table(&mut self) {
        self.table = self.store.zero_table.clone();
    }

    /// Append a module without running its start function. An `Err` invalidates
    /// the engine; see [`Module::new`].
    pub fn push_module(&mut self, module: Module) -> Result<ModuleRef, AllocError> {
        self.store.push_module(module)
    }

    /// Returns `true` if the engine is idle (not currently executing)
    #[inline(always)]
    pub fn is_idle(&self) -> bool {
        self.pc == JumpTarget::SENTINEL
    }

    /// Returns `true` if a host function has paused the engine and it is waiting on
    /// [`Engine::resume`]. Running a paused engine executes nothing.
    #[inline(always)]
    pub fn is_paused(&self) -> bool {
        self.host_pause.is_some()
    }

    /// Returns `true` if the module at `module_ref` declares a start function
    /// that must be run before the module is used.
    pub fn needs_start(&self, module_ref: ModuleRef) -> bool {
        self.store.modules()[module_ref.0 as usize].start.is_some()
    }

    /// Get the reference to a module's start function
    /// If the module does not have a start function, return None
    pub fn module_start(&self, module_ref: ModuleRef) -> Option<WasmRef> {
        if let Some(start) = self
            .store
            .modules()
            .get(module_ref.0 as usize)
            .and_then(|m| m.start)
        {
            // Unwrap the Ref -> WasmRef since we do not allow start mapped to host functions
            match start {
                // A local or cross-module Wasm start function is seeded like a
                // normal invocation; the caller runs the interpreter to drive it.
                Ref::Module(index) => Some(WasmRef {
                    module: module_ref,
                    index,
                }),
                Ref::Extern { module, index } => Some(WasmRef { module, index }),
                // Mapping start to host functions is not supported in spacewasm
                // This should have already been checked in the loader
                Ref::Host { .. } => unreachable!(),
            }
        } else {
            None
        }
    }

    pub fn call_host_fn(
        &mut self,
        module: HostModuleRef,
        index: u16,
        args: &[Value],
    ) -> HostFunctionResult {
        let f = self.store.host_modules[module.0 as usize].functions[index as usize]
            .get_call()
            .expect("host function is already executing");
        let r = f(self, args);
        self.store.host_modules[module.0 as usize].functions[index as usize].finish_call(f);
        r
    }
}
