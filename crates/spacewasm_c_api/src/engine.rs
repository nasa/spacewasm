//! The store handle and its builder.

use core::ffi::c_void;

const MAX_CONTROL_FRAMES: usize = 64;
const MAX_STACK_DEPTH: usize = 256;

use spacewasm::{
    CodeBuilder, CompilerOptions, Engine, ExportDesc, HostModule, Interpreter, InterpreterResult,
    InterpreterRunner, Memory, Module, ModuleRef, Rc, Ref, StartInvocation, ValType, Value, Vec,
    WasmMemoryAllocator, WasmRef, WasmStream,
};

use crate::status::{self, spacewasm_run_status_t, spacewasm_status_t, spacewasm_trap_t};
use crate::value::spacewasm_value_t;

/// Execution phase of a [`SpacewasmStore`]. Guards the `invoke`/`run`
/// preconditions so misuse from C returns an error instead of panicking across
/// the FFI boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineState {
    /// Ready to `invoke` a new function or run a module's start function.
    Idle,
    /// A function has been invoked; `run` may be called to drive it. Must reach
    /// `Finished`/`Trap` before the next invoke.
    Running,
    /// A module's Wasm start function has been seeded; `run_start` may be called
    /// to drive it. Must reach `Finished`/`Trap` before invoking anything else.
    RunningStart,
}

/// Callback signature for a host function implemented in C. `caller` is an
/// opaque handle for `spacewasm_mem_*`; write `out_result` iff returning a value.
pub type spacewasm_host_fn_t = Option<
    unsafe extern "C" fn(
        caller: *mut SpacewasmCaller,
        userdata: *mut c_void,
        params: *const spacewasm_value_t,
        n_params: usize,
        out_result: *mut spacewasm_value_t,
    ) -> spacewasm_hostcall_result_t,
>;

/// Result of a C host function call.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum spacewasm_hostcall_result_t {
    /// Continue; populate `out_result` if the function has a result type.
    SPACEWASM_CONTINUE = 0,
    /// Trap the interpreter.
    SPACEWASM_TRAP = 1,
    /// Pause the interpreter (cooperative yield).
    SPACEWASM_PAUSE = 2,
}

/// Opaque handle passed to C host callbacks, wrapping a borrowed core
/// [`Engine`]. Valid only for the duration of the call.
#[repr(C)]
pub struct SpacewasmCaller;

impl SpacewasmCaller {
    /// Re-derive the borrowed engine from an opaque caller pointer.
    ///
    /// # Safety
    /// `ptr` must be a caller pointer from the trampoline for an in-progress call.
    pub(crate) unsafe fn state<'a>(ptr: *const SpacewasmCaller) -> Option<&'a Engine> {
        if ptr.is_null() {
            None
        } else {
            Some(unsafe { &*(ptr as *const Engine) })
        }
    }
}

/// SpaceWasm store handle (`spacewasm_store_t`).
///
/// Owns the core [`Engine`] (which owns the store and execution state) and the
/// persistent [`CodeBuilder`] that accumulates compiled text across successive
/// module loads. The interpreter reads code directly from the builder's pages,
/// so no separate copy is kept.
pub struct SpacewasmStore {
    engine: Engine,
    code_builder: CodeBuilder,
    phase: EngineState,
}

impl SpacewasmStore {
    /// Build an empty store from the accumulated host modules, allocating the
    /// core [`Engine`] (with a `stack_size`-byte guest stack) and a
    /// [`CodeBuilder`] bounded to `max_code_pages`. `max_modules` is the
    /// guest-module capacity (≤ 256). The store is ready to load guest modules
    /// onto with [`SpacewasmStore::load_module`].
    pub fn new(
        stack_size: usize,
        max_modules: usize,
        max_code_pages: u32,
        host_modules: Vec<HostModule>,
    ) -> Result<SpacewasmStore, spacewasm_status_t> {
        if max_modules > 256 {
            return Err(status::SPACEWASM_ERR_BAD_ARG);
        }

        let engine =
            Engine::new(stack_size, max_modules, host_modules).map_err(status::memory_status)?;
        let code_builder = CodeBuilder::new(max_code_pages).map_err(status::alloc_status)?;

        Ok(SpacewasmStore {
            engine,
            code_builder,
            phase: EngineState::Idle,
        })
    }

    pub fn load_module(
        &mut self,
        name: &str,
        stream: &mut dyn WasmStream,
        allocator: Rc<dyn WasmMemoryAllocator>,
    ) -> Result<u32, spacewasm_status_t> {
        if self.phase != EngineState::Idle {
            return Err(status::SPACEWASM_ERR_WRONG_STATE);
        }

        let module = Module::new::<MAX_CONTROL_FRAMES, MAX_STACK_DEPTH>(
            name,
            stream,
            &mut self.engine.store,
            &mut self.code_builder,
            allocator,
            CompilerOptions::default(),
        )
        .map_err(|e| status::parse_status(&e))?;

        let module_ref = self.engine.push_module(module);
        Ok(module_ref.0 as u32)
    }

    /// Returns `true` if the module at `module_idx` declares a start function
    /// that should be run (via [`SpacewasmStore::run_start`]) before use.
    pub fn module_needs_start(&self, module_idx: u32) -> Result<bool, spacewasm_status_t> {
        if module_idx as usize >= self.engine.store.modules().len() {
            return Err(status::SPACEWASM_ERR_NOT_FOUND);
        }
        Ok(self.engine.needs_start(ModuleRef(module_idx as u8)))
    }

    /// Run the start function of module `module_idx`, if it declares one, for up
    /// to `fuel` instructions. The store must be [`EngineState::Idle`] on the
    /// first call. A Wasm start function that does not finish within `fuel`
    /// leaves the store in [`EngineState::RunningStart`]; call again to resume.
    /// On `Finished`/`Trap` the store returns to [`EngineState::Idle`].
    pub fn run_start(
        &mut self,
        module_idx: u32,
        fuel: usize,
    ) -> (spacewasm_run_status_t, spacewasm_trap_t) {
        if module_idx as usize >= self.engine.store.modules().len() {
            return (
                spacewasm_run_status_t::SPACEWASM_RUN_TRAP,
                status::SPACEWASM_TRAP_NONE,
            );
        }

        // Seed the start invocation on the first call; a subsequent call while
        // already `RunningStart` just resumes the interpreter loop below.
        if self.phase == EngineState::Idle {
            match self.engine.invoke_start(ModuleRef(module_idx as u8)) {
                StartInvocation::Finished => {
                    return (
                        spacewasm_run_status_t::SPACEWASM_RUN_FINISHED,
                        status::SPACEWASM_TRAP_NONE,
                    );
                }
                StartInvocation::Trap(t) => {
                    return (
                        spacewasm_run_status_t::SPACEWASM_RUN_TRAP,
                        status::trap_reason_code(t),
                    );
                }
                StartInvocation::Pause => {
                    return (
                        spacewasm_run_status_t::SPACEWASM_RUN_PAUSE,
                        status::SPACEWASM_TRAP_NONE,
                    );
                }
                StartInvocation::Running => {
                    self.phase = EngineState::RunningStart;
                }
            }
        } else if self.phase != EngineState::RunningStart {
            return (
                spacewasm_run_status_t::SPACEWASM_RUN_TRAP,
                status::SPACEWASM_TRAP_NONE,
            );
        }

        self.drive(fuel)
    }

    /// Resolve an exported function by name in module `module_idx` to an index
    /// usable with [`SpacewasmStore::invoke`].
    pub fn find_export_func(&self, module_idx: u32, name: &str) -> Result<u16, spacewasm_status_t> {
        let module = self
            .engine
            .store
            .modules()
            .get(module_idx as usize)
            .ok_or(status::SPACEWASM_ERR_NOT_FOUND)?;

        for e in &module.exports {
            if e.name == name {
                if let ExportDesc::Func(fi) = e.desc {
                    return match module.get_func_ref(fi) {
                        Some(Ref::Module(idx)) => Ok(idx),
                        _ => Err(status::SPACEWASM_ERR_NOT_FOUND),
                    };
                }
            }
        }
        Err(status::SPACEWASM_ERR_NOT_FOUND)
    }

    /// Invoke a function in module `module_idx` by resolved index, seeding it
    /// with `params`. The store must be [`EngineState::Idle`]; on success it
    /// becomes [`EngineState::Running`].
    pub fn invoke(
        &mut self,
        module_idx: u32,
        func_index: u16,
        params: &[Value],
    ) -> Result<(), spacewasm_status_t> {
        if self.phase != EngineState::Idle {
            return Err(status::SPACEWASM_ERR_WRONG_STATE);
        }

        if module_idx as usize >= self.engine.store.modules().len() {
            return Err(status::SPACEWASM_ERR_NOT_FOUND);
        }

        let f_ref = WasmRef {
            module: ModuleRef(module_idx as u8),
            index: func_index,
        };

        self.engine
            .invoke(f_ref, params)
            .map_err(status::invoke_status)?;
        self.phase = EngineState::Running;
        Ok(())
    }

    /// Run up to `fuel` instructions. Returns the run status and, on a trap, the
    /// trap reason (else [`status::SPACEWASM_TRAP_NONE`]). On `Finished`/`Trap`
    /// the store resets to [`EngineState::Idle`] so it can be invoked again.
    pub fn run(&mut self, fuel: usize) -> (spacewasm_run_status_t, spacewasm_trap_t) {
        if self.phase != EngineState::Running {
            return (
                spacewasm_run_status_t::SPACEWASM_RUN_TRAP,
                status::SPACEWASM_TRAP_NONE,
            );
        }
        self.drive(fuel)
    }

    fn drive(&mut self, fuel: usize) -> (spacewasm_run_status_t, spacewasm_trap_t) {
        let interpreter = Interpreter;
        let result = interpreter.run(self.code_builder.pages(), &mut self.engine, fuel);

        let (rs, trap) = status::run_status(&result);
        match result {
            InterpreterResult::Finished
            | InterpreterResult::Trap(_)
            | InterpreterResult::ReaderError(_) => {
                self.phase = EngineState::Idle;
            }
            InterpreterResult::Pause | InterpreterResult::OutOfFuel => {
                // Stay in the current running phase; caller resumes.
            }
        }
        (rs, trap)
    }

    /// The last invocation's result value, interpreted as `ty`, or `None` if
    /// none is recorded.
    pub fn get_result(&self, ty: ValType) -> Option<spacewasm_value_t> {
        self.engine
            .result
            .map(|raw| spacewasm_value_t::from_raw(raw, ty))
    }

    /// The active guest linear memory (from the most recent invocation context).
    pub fn memory(&self) -> &Rc<Memory> {
        &self.engine.memory
    }
}
