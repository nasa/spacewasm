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
    pub fn new<const HOST_MODULE_N: usize>(
        max_modules: usize,
        host_modules: [HostModule; HOST_MODULE_N],
    ) -> Result<Self, AllocError> {
        if max_modules > 256 {
            return Err(AllocError::OutOfMemory);
        }
        Ok(Store {
            modules: Vec::new(max_modules as u32)?,
            host_modules: Vec::from_array(host_modules)?,
            zero_memory: Rc::new(Memory::zero())?,
            zero_table: Rc::new_slice_with_default(0)?,
        })
    }

    /// Construct a store from a runtime-built collection of host modules,
    /// rather than a const-sized array. Useful for embedders (e.g. the C FFI
    /// layer) that accumulate host modules dynamically. Returns
    /// [`AllocError::OutOfMemory`] if `max_modules` exceeds the 256-module
    /// limit, instead of panicking.
    pub fn from_host_modules(
        max_modules: usize,
        host_modules: Vec<HostModule>,
    ) -> Result<Self, AllocError> {
        if max_modules > 256 {
            return Err(AllocError::OutOfMemory);
        }
        Ok(Store {
            modules: Vec::new(max_modules as u32)?,
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

    /// Push a module onto the store
    /// Panics if the store is at capacity
    #[inline(always)]
    pub fn push_module(&mut self, module: Module) {
        self.modules.push(module);
    }

    pub fn get_memory(&mut self, module_ref: ModuleRef) -> &Rc<Memory> {
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

    pub fn get_table(&mut self, module_ref: ModuleRef) -> &Rc<[TableElement]> {
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
        })
    }

    pub fn reset(&mut self) {
        self.pc = JumpTarget::SENTINEL;
        self.sp = 0;
        self.fp = 0;
        self.jumped = false;
        self.result = None;
        self.clear_memory();
        self.clear_table();
    }

    pub fn clear_memory(&mut self) {
        self.memory = self.store.zero_memory.clone();
    }

    pub fn clear_table(&mut self) {
        self.table = self.store.zero_table.clone();
    }

    /// Append a module to the store without running its start function.
    /// Note: The start function still needs to be run (if there is one)
    /// Returns the ModuleRef of the new module
    pub fn push_module(&mut self, module: Module) -> ModuleRef {
        self.store.modules.push(module);
        ModuleRef((self.store.modules.len() - 1) as u8)
    }

    /// Returns `true` if the module at `module_ref` declares a start function
    /// that must be run before the module is used.
    pub fn needs_start(&self, module_ref: ModuleRef) -> bool {
        self.store.modules()[module_ref.0 as usize].start.is_some()
    }

    /// Invoke the module's start function for execution, if it declares one.
    ///
    /// The interpreter must be idle (no invocation in flight), matching the
    /// preconditions of [`Engine::invoke`].
    pub fn invoke_start(&mut self, module_ref: ModuleRef) -> StartInvocation {
        let Some(start) = self.store.modules()[module_ref.0 as usize].start else {
            return StartInvocation::Finished;
        };

        match start {
            // A local or cross-module Wasm start function is seeded like a
            // normal invocation; the caller runs the interpreter to drive it.
            Ref::Module(index) => self.setup_start_invoke(WasmRef {
                module: module_ref,
                index,
            }),
            Ref::Extern { module, index } => self.setup_start_invoke(WasmRef { module, index }),
            // Host start functions run immediately; no interpreter loop needed.
            Ref::Host { module, index } => {
                match self.store.host_modules()[module.0 as usize].functions[index as usize]
                    .call(self, &[])
                {
                    HostFunctionResult::Continue(_) => StartInvocation::Finished,
                    HostFunctionResult::Break(HostFunctionBreak::Trap) => {
                        StartInvocation::Trap(TrapReason::Host)
                    }
                    HostFunctionResult::Break(HostFunctionBreak::Pause) => StartInvocation::Pause,
                }
            }
        }
    }

    fn setup_start_invoke(&mut self, f_ref: WasmRef) -> StartInvocation {
        match self.invoke(f_ref, &[]) {
            Ok(()) => StartInvocation::Running,
            Err(InvokeError::StackOverflow) => StartInvocation::Trap(TrapReason::StackOverflow),
            // The start function is validated to be `[] -> []`, so parameter
            // length/type mismatches cannot occur.
            Err(_) => unreachable!(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartInvocation {
    /// There was no start function, or a host start function completed.
    Finished,
    /// A Wasm start function was invoked, need to spin the interpreter
    Running,
    /// A host start function trapped.
    Trap(TrapReason),
    /// A host start function paused
    Pause,
}
