use crate::util::Vec;
use crate::{
    AllocError, Box, HostFunctionBreak, HostFunctionResult, HostModule, Interpreter,
    InterpreterBreak, InterpreterResult, InterpreterRunner, InterpreterState, IrReaderError,
    Memory, MemoryError, MemoryKind, Module, ModuleRef, Rc, Ref, TextPage, TrapReason, WasmRef,
};
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
pub struct Store {
    pub modules: Vec<Box<Module>>,
    pub host_modules: Vec<HostModule>,
    pub zero_memory: Rc<Memory>,
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

    /// Finish linking WASM modules and generate the next stage of store
    pub fn allocate(self, stack_size: usize) -> Result<UninitializedStore, MemoryError> {
        Ok(UninitializedStore {
            state: InterpreterState::new(
                Store {
                    modules: self.modules,
                    host_modules: self.host_modules,
                    zero_memory: Rc::new(Memory::zero())?,
                },
                stack_size,
            )?,
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
    state: InterpreterState,
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

type InitializeImplResult =
    Result<ControlFlow<InterpreterState, InitializeContinue>, InitializeError>;
pub enum InitializeResult {
    Finished(InterpreterState),
    Continue(UninitializedStore),
}

impl UninitializedStore {
    fn initialize_impl_run_interpreter(
        mut self,
        code: &[Box<TextPage>],
        n_instructions: usize,
    ) -> InitializeImplResult {
        let interpreter = Interpreter;
        match interpreter.run(code, &mut self.state, n_instructions) {
            InterpreterResult::OutOfFuel => {
                Ok(ControlFlow::Continue(InitializeContinue::OutOfFuel(self)))
            }
            InterpreterResult::Instruction(InterpreterBreak::Finished) => {
                // Finished this function
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
        n_instructions: usize,
    ) -> InitializeImplResult {
        if let Some(_) = self.running_start {
            self.initialize_impl_run_interpreter(code, n_instructions)
        } else {
            let next_module = if let Some(finished_start) = self.finished_start {
                // We have finished running at least one module
                // Let's move on to the next one
                (finished_start.0 as usize) + 1
            } else {
                // This is the first module
                0
            };

            if let Some(m) = self.state.store.modules.get(next_module) {
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
                        return match self.state.store.host_modules[module.0 as usize].functions
                            [index as usize]
                            .call(&self.state, &[])
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

                match self.state.invoke(start_ref, &[]) {
                    Ok(_) => {}
                    Err(InterpreterBreak::Trap(t)) => return Err(InitializeError::Trap(t)),
                    _ => unreachable!(),
                };

                self.running_start = Some(start_ref.module);
                self.initialize_impl_run_interpreter(code, n_instructions)
            } else {
                // No more modules, we are done
                Ok(ControlFlow::Break(self.state))
            }
        }
    }

    /// Initialize this store into a full WebAssembly store.
    /// This function will execute the 'start' functions of each module (if there are any) in order.
    pub fn initialize(
        mut self,
        code: &[Box<TextPage>],
        n_instructions: usize,
    ) -> Result<InitializeResult, InitializeError> {
        loop {
            // FIXME(tumbar) We should be decrementing n_instructions on every impl run
            self = match self.initialize_impl(code, n_instructions) {
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

impl Store {
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
            Some(MemoryKind::ImportHost(host_import)) => self.host_modules[host_import.0 as usize]
                .memory
                .as_ref()
                .unwrap(),
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
            Some(MemoryKind::ImportHost(host_import)) => self.host_modules[host_import.0 as usize]
                .memory
                .as_mut()
                .unwrap(),
        }
    }
}
