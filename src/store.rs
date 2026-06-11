use crate::util::Vec;
use crate::{
    AllocError, Box, HostFunctionBreak, HostFunctionResult, HostModule, HostModuleRef, Interpreter,
    InterpreterBreak, InterpreterResult, InterpreterRunner, InterpreterState, IrReaderError,
    Memory, MemoryError, MemoryKind, Module, ModuleRef, Ref, TextPage, TrapReason,
    WasmMemoryAllocator, WasmRef,
};
use core::cell::RefCell;
use core::ops::ControlFlow;

/// A data structure representing the WebAssembly Store during load / validation time.
/// This linker must be allocated into an [UninitializedStore] which is later
/// initialized into a full [Store].
pub struct StoreLinker {
    /// The modules we have initialized so far
    pub modules: Vec<Box<Module>>,
    /// The host modules of the store
    pub host_modules: Vec<HostModule>,
}

/// Holds ownership of all the loaded modules. As new modules are loaded,
/// imports/exports are referenced through the store.
#[derive(Default)]
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
        assert!(max_modules < 255);
        Ok(StoreLinker {
            modules: Vec::new(max_modules as u32)?,
            host_modules: Vec::from_array(host_modules)?,
        })
    }

    pub fn get_memory(&self, index: u8) -> Option<Ref> {
        // Host memory goes first
        if let Some((host_index, _)) = self
            .host_modules
            .iter()
            .enumerate()
            .filter(|(_, hm)| hm.memory.is_some())
            .nth(index as usize)
        {
            return Some(Ref::Host {
                module: HostModuleRef::new(host_index),
                index: 0,
            });
        }

        let n_host_memories = self
            .host_modules
            .iter()
            .enumerate()
            .filter(|(_, hm)| hm.memory.is_some())
            .count();

        let (original_module_index, _) =
            self.modules
                .iter()
                .enumerate()
                .find(|(_, mp)| match mp.memory {
                    Some(MemoryKind::Allocate {
                        index: index_iter, ..
                    }) if index_iter == ((n_host_memories as u8) - index) => true,
                    _ => false,
                })?;

        Some(Ref::Extern {
            module: ModuleRef(original_module_index as u8),
            index: 0,
        })
    }

    /// Take loaded modules and allocate the linear memory needed into an [UninitializedStore].
    /// The data is also populated into the linear memory.
    /// This function must be called _after_ all modules are loaded
    pub fn allocate(
        self,
        allocator: &'static dyn WasmMemoryAllocator,
    ) -> Result<UninitializedStore, MemoryError> {
        // Count the owned memories in the entire store
        let host_memories = self
            .host_modules
            .iter()
            .filter(|m| m.memory.is_some())
            .count();

        let wasm_memories = self
            .modules
            .iter()
            .filter(|m| match m.memory {
                Some(MemoryKind::Allocate { .. }) => true,
                _ => false,
            })
            .count();

        let total_memories = host_memories + wasm_memories;

        // Allocate some space to hold all the memories
        let mut memory = Vec::new(total_memories as u32)?;
        let mut host_modules = self.host_modules;

        // Initialize the memory and fill up the data
        for module in &mut host_modules {
            if let Some(ty) = module.memory {
                memory.push(RefCell::new(Memory::new(ty, allocator)?));
            }
        }

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

        Ok(UninitializedStore {
            store: Store {
                modules: self.modules,
                host_modules,
                memory,
            },
            finished_start: None,
            running_start: None,
        })
    }
}

/// This is a store that still needs to run the start functions on the modules.
/// There is an embedded full store that is not safe to invoke functions on since the 'start'
/// functions have not been executed.
///
/// This store can be initialized into a [Store] using [UninitializedStore::initialize]
pub struct UninitializedStore {
    store: Store,
    finished_start: Option<ModuleRef>,
    running_start: Option<ModuleRef>,
}

#[derive(Debug)]
pub enum InitializeError {
    Trap(TrapReason),
    /// Host function requested an interpreter pause during initialization
    PauseDuringInitialization,
    ReaderError(IrReaderError),
}

enum InitializeContinue {
    /// The current module finished its start function, go on to the next one
    FinishedModuleStart(UninitializedStore),
    /// No more fuel (ran to instruction bound)
    OutOfFuel(UninitializedStore),
}

type InitializeImplResult = Result<ControlFlow<Store, InitializeContinue>, InitializeError>;
pub enum InitializeResult {
    Finished(Store),
    Continue(UninitializedStore),
}

impl UninitializedStore {
    fn initialize_impl_run_interpreter(
        mut self,
        code: &[Box<TextPage>],
        state: &mut InterpreterState,
        n_instructions: usize,
    ) -> InitializeImplResult {
        let interpreter = Interpreter::new(self.store);
        match interpreter.run(code, state, n_instructions) {
            InterpreterResult::OutOfFuel => {
                self.store = interpreter.store;
                Ok(ControlFlow::Continue(InitializeContinue::OutOfFuel(self)))
            }
            InterpreterResult::Instruction(InterpreterBreak::Finished) => {
                // Finished this function
                self.store = interpreter.store;
                self.finished_start = self.running_start;
                self.running_start = None;
                Ok(ControlFlow::Continue(
                    InitializeContinue::FinishedModuleStart(self),
                ))
            }
            InterpreterResult::Instruction(InterpreterBreak::Pause) => {
                Err(InitializeError::PauseDuringInitialization)
            }
            InterpreterResult::Instruction(InterpreterBreak::Trap(t)) => {
                Err(InitializeError::Trap(t))
            }
            InterpreterResult::ReaderError(e) => Err(InitializeError::ReaderError(e)),
        }
    }

    fn initialize_impl(
        mut self,
        code: &[Box<TextPage>],
        state: &mut InterpreterState,
        n_instructions: usize,
    ) -> InitializeImplResult {
        if let Some(_) = self.running_start {
            self.initialize_impl_run_interpreter(code, state, n_instructions)
        } else {
            let next_module = if let Some(finished_start) = self.finished_start {
                // We have finished running at least one module
                // Let's move on to the next one
                (finished_start.0 as usize) + 1
            } else {
                // This is the first module
                0
            };

            if let Some(m) = self.store.modules.get(next_module) {
                let start_ref = match m.start {
                    None => {
                        self.running_start = None;
                        self.finished_start = Some(ModuleRef(next_module as u8));
                        return Ok(ControlFlow::Continue(
                            InitializeContinue::FinishedModuleStart(self),
                        ));
                    }
                    Some(Ref::Module(fi)) => WasmRef {
                        module: ModuleRef(next_module as u8),
                        index: fi,
                    },
                    Some(Ref::Extern { module, index }) => WasmRef { module, index },
                    Some(Ref::Host { module, index }) => {
                        // We don't need to run the interpreter for host functions
                        // We can just invoke the function
                        return match self.store.host_modules[module.0 as usize].functions
                            [index as usize]
                            .call(state, &[])
                        {
                            HostFunctionResult::Continue(_) => {
                                self.running_start = None;
                                self.finished_start = Some(ModuleRef(next_module as u8));
                                Ok(ControlFlow::Continue(
                                    InitializeContinue::FinishedModuleStart(self),
                                ))
                            }
                            HostFunctionResult::Break(HostFunctionBreak::Trap) => {
                                Err(InitializeError::Trap(TrapReason::Host))
                            }
                            HostFunctionResult::Break(HostFunctionBreak::Pause) => {
                                Err(InitializeError::PauseDuringInitialization)
                            }
                        };
                    }
                };

                match state.invoke(&self.store, start_ref, &[]) {
                    Ok(_) => {}
                    Err(InterpreterBreak::Trap(t)) => return Err(InitializeError::Trap(t)),
                    _ => unreachable!(),
                };

                self.running_start = Some(start_ref.module);
                self.initialize_impl_run_interpreter(code, state, n_instructions)
            } else {
                // No more modules, we are done
                Ok(ControlFlow::Break(self.store))
            }
        }
    }

    /// Initialize this store into a full WebAssembly store.
    /// This function will execute the 'start' functions of each module (if there are any) in order.
    pub fn initialize(
        mut self,
        code: &[Box<TextPage>],
        state: &mut InterpreterState,
        n_instructions: usize,
    ) -> Result<InitializeResult, InitializeError> {
        loop {
            // FIXME(tumbar) We should be decrementing n_instructions on every impl run
            self = match self.initialize_impl(code, state, n_instructions) {
                Ok(ControlFlow::Continue(InitializeContinue::OutOfFuel(s))) => {
                    return Ok(InitializeResult::Continue(s));
                }
                Ok(ControlFlow::Continue(InitializeContinue::FinishedModuleStart(s))) => s,
                Ok(ControlFlow::Break(store)) => return Ok(InitializeResult::Finished(store)),
                Err(e) => return Err(e),
            }
        }
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
