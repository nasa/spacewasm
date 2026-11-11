//! Loading a module to run it: what `run` and `trace` share.
//!
//! Unlike `validate`, this is the ground interpreter, not the flight one: limits are
//! generous, and host functions are real.

use spacewasm::{
    CodeBuilder, CompilerOptions, Engine, ExportDesc, HostModule, InnerVec, Interpreter,
    InterpreterResult, InterpreterRunner, InvokeError, Module, ModuleRef, Ref, TrapReason, ValType,
    Value, WasmMemoryAllocator, WasmRef, WasmStream,
};
use spacewasm_tools::DecoderLimits;
use std::alloc::Layout;
use std::fmt;
use std::ptr::NonNull;

const MAX_CODE_PAGES: usize = 0x10_000;

/// A module decoded and instantiated, its start function not yet run.
pub struct Loaded {
    pub engine: Engine,
    pub code: CodeBuilder,
    pub module: ModuleRef,
}

#[derive(Debug)]
pub struct LoadError(String);

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for LoadError {}

/// Decode `bytes` against `hosts` within `limits`, with an operand stack of
/// `stack_words`.
pub fn load(
    bytes: &[u8],
    hosts: spacewasm::Vec<HostModule>,
    limits: DecoderLimits,
    stack_words: usize,
) -> Result<Loaded, LoadError> {
    let mut code = CodeBuilder::new(CompilerOptions::new(true, None, MAX_CODE_PAGES))
        .map_err(|err| LoadError(format!("could not allocate the code pages: {err:?}")))?;
    let mut engine = Engine::new(stack_words, 1, hosts)
        .map_err(|err| LoadError(format!("could not create the interpreter: {err:?}")))?;
    let memory = spacewasm::Rc::new(SystemMemory)
        .map_err(|err| LoadError(format!("out of memory: {err:?}")))?
        .into_wasm_memory_allocator();

    let module = limits
        .with_stack(|stack| {
            Module::new(
                "main",
                &mut Slice::new(bytes),
                &mut engine.store,
                &mut code,
                stack,
                memory,
            )
        })
        .map_err(|err| {
            // Name the proposal when that is why, as `validate` would.
            match spacewasm_tools::find_unsupported(bytes) {
                Some(unsupported) => LoadError(unsupported.to_string()),
                None => LoadError(format!(
                    "the module does not load at byte {:#x}: {:?}",
                    err.offset, err.err.err
                )),
            }
        })?;
    let module = engine
        .push_module(module)
        .map_err(|err| LoadError(format!("could not instantiate the module: {err:?}")))?;
    Ok(Loaded {
        engine,
        code,
        module,
    })
}

impl Loaded {
    /// Run the start function, if there is one, to completion.
    pub fn start(&mut self, fuel: usize) -> InterpreterResult {
        match self.engine.module_start(self.module) {
            None => InterpreterResult::Finished,
            Some(start) => match self.engine.invoke(start, &[]) {
                Ok(()) => Interpreter.run(self.code.pages(), &mut self.engine, fuel),
                Err(InvokeError::StackOverflow) => {
                    InterpreterResult::Trap(TrapReason::StackOverflow)
                }
                Err(err) => unreachable!("a fresh engine refused its start function: {err:?}"),
            },
        }
    }

    /// The exported function `name`, if it is one this module defines.
    pub fn export(&self, name: &str) -> Option<WasmRef> {
        let module = self.engine.store.modules().get(self.module.0 as usize)?;
        let export = module.exports.iter().find(|export| export.name == name)?;
        let ExportDesc::Func(index) = export.desc else {
            return None;
        };
        match module.get_func_ref(index)? {
            Ref::Module(index) => Some(WasmRef {
                module: self.module,
                index,
            }),
            _ => None,
        }
    }

    /// Every exported function this module defines, with zero for each argument.
    pub fn exports(&self) -> Vec<(String, WasmRef, Vec<Value>)> {
        let Some(module) = self.engine.store.modules().get(self.module.0 as usize) else {
            return Vec::new();
        };
        module
            .exports
            .iter()
            .filter_map(|export| {
                let ExportDesc::Func(index) = export.desc else {
                    return None;
                };
                let Ref::Module(index) = module.get_func_ref(index)? else {
                    return None;
                };
                let function = module.functions.get(index as usize)?;
                let ty = module.types.get(function.ty.0 as usize)?;
                let args = ty.params.iter().map(|ty| zero(*ty)).collect();
                Some((
                    export.name.to_string(),
                    WasmRef {
                        module: self.module,
                        index,
                    },
                    args,
                ))
            })
            .collect()
    }
}

fn zero(ty: ValType) -> Value {
    match ty {
        ValType::I32 => Value::I32(0),
        ValType::I64 => Value::I64(0),
        ValType::F32 => Value::F32(0.0),
        ValType::F64 => Value::F64(0.0),
    }
}

/// The whole module as one chunk.
struct Slice<'a> {
    bytes: &'a [u8],
    done: bool,
}

impl<'a> Slice<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Slice { bytes, done: false }
    }
}

impl WasmStream for Slice<'_> {
    fn read(&mut self) -> Result<Option<InnerVec<u8>>, u8> {
        if self.done {
            return Ok(None);
        }
        self.done = true;
        // SAFETY: the decoder only reads through the view, and `self.bytes` outlives it.
        Ok(Some(unsafe {
            InnerVec::from_raw_parts(
                self.bytes.as_ptr().cast_mut(),
                self.bytes.len(),
                self.bytes.len(),
            )
        }))
    }

    fn return_(&mut self, _: InnerVec<u8>) {}
}

/// Guest linear memory, from the system allocator.
struct SystemMemory;

impl WasmMemoryAllocator for SystemMemory {
    fn allocate(&self, layout: Layout) -> Result<NonNull<u8>, spacewasm::AllocError> {
        if layout.size() == 0 {
            return Ok(NonNull::dangling());
        }
        // SAFETY: non-zero size, checked above.
        NonNull::new(unsafe { std::alloc::alloc_zeroed(layout) })
            .ok_or(spacewasm::AllocError::AllocationFailed)
    }

    fn reallocate(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        layout: Layout,
    ) -> Result<NonNull<u8>, spacewasm::AllocError> {
        if old_layout.size() == 0 {
            return self.allocate(layout);
        }
        // SAFETY: `ptr` came from `allocate`/`reallocate` with `old_layout`. The new tail
        // need not be zeroed here: `Memory::grow` does that.
        NonNull::new(unsafe { std::alloc::realloc(ptr.as_ptr(), old_layout, layout.size()) })
            .ok_or(spacewasm::AllocError::AllocationFailed)
    }

    fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        if layout.size() != 0 {
            // SAFETY: `ptr` came from `allocate`/`reallocate` with `layout`.
            unsafe { std::alloc::dealloc(ptr.as_ptr(), layout) }
        }
    }
}
