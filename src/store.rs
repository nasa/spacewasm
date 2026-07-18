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
        assert!(max_modules <= 256);
        Ok(Store {
            modules: Vec::new(max_modules as u32)?,
            host_modules: Vec::from_array(host_modules)?,
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

    /// Finish linking Wasm modules and generate the next stage of store
    pub fn allocate(&mut self, stack_size: usize) -> Result<InterpreterState<'_>, MemoryError> {
        Ok(InterpreterState {
            pc: JumpTarget::SENTINEL,
            sp: 0x0,
            fp: 0x0,
            stack: Stack::new(stack_size)?,
            memories: Rc::new([]),
            table: self.zero_table.clone(),
            jumped: false,
            module: ModuleRef(0),
            store: self,
            result: None,
        })
    }

    pub fn get_memory(&mut self, module_ref: ModuleRef, mem_idx: usize) -> &Rc<Memory> {
        match self.modules[module_ref.0 as usize].memories.get(mem_idx) {
            None => &self.zero_memory,
            Some(MemoryKind::Owned(mem)) => mem,
            Some(MemoryKind::Import(import_module_ref)) => {
                let r = import_module_ref.0 as usize;
                let Some(MemoryKind::Owned(mem)) = self.modules[r].memories.get(0) else {
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

    pub fn get_memory_mut(&mut self, module_ref: ModuleRef, mem_idx: usize) -> &mut Rc<Memory> {
        match self.modules[module_ref.0 as usize].memories.get(mem_idx) {
            None => &mut self.zero_memory,
            Some(MemoryKind::Owned(_)) => {
                let Some(MemoryKind::Owned(mem)) = &mut self.modules[module_ref.0 as usize].memories[mem_idx]
                else {
                    unreachable!()
                };
                mem
            }
            Some(MemoryKind::Import(import_module_ref)) => {
                let r = import_module_ref.0 as usize;
                let Some(MemoryKind::Owned(mem)) = self.modules[r].memories.get_mut(0) else {
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
    pub fn get_memories(&mut self, module_ref: ModuleRef) -> Rc<[Rc<Memory>]> {
        let mut memories = Vec::new();
        for i in 0..self.modules[module_ref.0 as usize].memories.len() {
            memories.push(self.get_memory(module_ref, i).clone());
        }
        Rc::from(memories.into_boxed_slice())
    }
}

impl<'store> InterpreterState<'store> {
    pub fn clear_memories(&mut self) {
        self.memories = Rc::new([]);
    }

    pub fn clear_table(&mut self) {
        self.table = self.store.zero_table.clone();
    }

    pub fn initialize_module(
        &mut self,
        module: Module,
        code: &[Box<TextPage>],
        n_instructions: usize,
    ) -> InterpreterResult {
        let interpreter = Interpreter::default();
        self.store.modules.push(module);

        if let Some(start) = self.store.modules.last().unwrap().start {
            match start {
                Ref::Module(i) => {
                    if let Err(e) = self.invoke(
                        WasmRef {
                            module: ModuleRef((self.store.modules().len() - 1) as u8),
                            index: i,
                        },
                        &[],
                    ) {
                        return match e {
                            InvokeError::StackOverflow => {
                                InterpreterResult::Trap(TrapReason::StackOverflow)
                            }
                            _ => unreachable!(),
                        };
                    }
                }
                Ref::Host { module, index } => {
                    // We don't need to run the interpreter for host functions
                    // We can just invoke the function
                    return match self.store.host_modules[module.0 as usize].functions
                        [index as usize]
                        .call(self, &[])
                    {
                        HostFunctionResult::Continue(_) => InterpreterResult::Finished,
                        HostFunctionResult::Break(HostFunctionBreak::Trap) => {
                            InterpreterResult::Trap(TrapReason::Host)
                        }
                        HostFunctionResult::Break(HostFunctionBreak::Pause) => {
                            InterpreterResult::Pause
                        }
                    };
                }
                Ref::Extern { module, index } => {
                    if let Err(e) = self.invoke(WasmRef { module, index }, &[]) {
                        return match e {
                            InvokeError::StackOverflow => {
                                InterpreterResult::Trap(TrapReason::StackOverflow)
                            }
                            _ => unreachable!(),
                        };
                    }
                }
            }
        } else {
            return InterpreterResult::Finished;
        }

        // Spin the interpreter
        interpreter.run(code, self, n_instructions)
    }
}
