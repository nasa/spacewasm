use crate::*;

/// Holds ownership of all the loaded modules. As new modules are loaded,
/// imports/exports are referenced through the store.
#[derive(Debug)]
pub struct Store {
    modules: Vec<Box<Module>>,
    host_modules: Vec<HostModule>,
    zero_memory: Rc<Memory>,
    zero_table: Rc<[TableElement]>,
}

impl Store {
    pub fn new<const N: usize>(
        max_modules: usize,
        host_modules: [HostModule; N],
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
    pub fn modules(&self) -> &[Box<Module>] {
        &self.modules
    }

    #[inline(always)]
    pub fn modules_mut(&mut self) -> &mut [Box<Module>] {
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
    pub fn pop_module(&mut self) -> Option<Box<Module>> {
        self.modules.pop()
    }

    /// Push a module onto the store
    /// Panics if the store is at capacity
    #[inline(always)]
    pub fn push_module(&mut self, module: Box<Module>) {
        self.modules.push(module);
    }

    /// Finish linking WASM modules and generate the next stage of store
    pub fn allocate(&mut self, stack_size: usize) -> Result<InterpreterState<'_>, MemoryError> {
        Ok(InterpreterState {
            pc: JumpTarget::SENTINEL,
            sp: 0x0,
            fp: 0x0,
            stack: Stack::new(stack_size)?,
            memory: self.zero_memory.clone(),
            table: self.zero_table.clone(),
            jumped: false,
            module: ModuleRef(0),
            store: self,
            result: None,
        })
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

#[derive(Debug)]
pub enum InitializeResult {
    Ok,
    OutOfFuel,
    Trap(TrapReason),
    ReaderError(IrReaderError),
    Pause,
}

impl<'store> InterpreterState<'store> {
    pub fn clear_memory(&mut self) {
        self.memory = self.store.zero_memory.clone();
    }

    pub fn clear_table(&mut self) {
        self.table = self.store.zero_table.clone();
    }

    pub fn initialize_module(
        &mut self,
        module: Box<Module>,
        code: &[Box<TextPage>],
        n_instructions: usize,
    ) -> InitializeResult {
        let interpreter = Interpreter::default();
        self.store.modules.push(module);

        if let Some(start) = self.store.modules.last().unwrap().start {
            match start {
                Ref::Module(i) => {
                    self.invoke(
                        WasmRef {
                            module: ModuleRef((self.store.modules().len() - 1) as u8),
                            index: i,
                        },
                        &[],
                    ).unwrap();
                }
                Ref::Host { module, index } => {
                    // We don't need to run the interpreter for host functions
                    // We can just invoke the function
                    return match self.store.host_modules[module.0 as usize].functions
                        [index as usize]
                        .call(&self, &[])
                    {
                        HostFunctionResult::Continue(_) => InitializeResult::Ok,
                        HostFunctionResult::Break(HostFunctionBreak::Trap) => {
                            InitializeResult::Trap(TrapReason::Host)
                        }
                        HostFunctionResult::Break(HostFunctionBreak::Pause) => {
                            InitializeResult::Pause
                        }
                    };
                }
                Ref::Extern { module, index } => {
                    self.invoke(WasmRef { module, index }, &[]).unwrap();
                }
            }
        } else {
            return InitializeResult::Ok;
        }

        // Spin the interpreter
        match interpreter.run(code, self, n_instructions) {
            InterpreterResult::OutOfFuel => InitializeResult::OutOfFuel,
            InterpreterResult::Instruction(i) => match i {
                InterpreterBreak::Finished => InitializeResult::Ok,
                InterpreterBreak::Trap(t) => InitializeResult::Trap(t),
                InterpreterBreak::Pause => InitializeResult::Pause,
            },
            InterpreterResult::ReaderError(r) => InitializeResult::ReaderError(r),
        }
    }
}
