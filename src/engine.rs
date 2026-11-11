use crate::{JumpTarget, Memory, ModuleRef, RawValue, Rc, ResultType, Stack, Store, TableElement};

/// The Wasm engine holds the state of the interpreter and the WebAssembly store
pub struct Engine {
    /// Current program counter
    pub pc: JumpTarget,

    /// Flag tracking if the current instruction jumps to another PC
    pub jumped: bool,

    /// Frame pointer. Base address of local variables in the current stack frame
    pub fp: u32,

    /// Stack pointer
    pub sp: usize,

    /// The stack
    pub stack: Stack,

    /// Linear memory from the active module
    pub memory: Rc<Memory>,

    /// Table from the active module
    pub table: Rc<[TableElement]>,

    /// Current module we are executing code for
    pub module: ModuleRef,

    /// The WebAssembly Store
    pub store: Store,

    /// A host function requested a pause, and [`Engine::resume`] has not yet supplied
    /// its result. While this is set [`Engine::pc`] stays on the calling instruction
    /// and [`run`](crate::InterpreterRunner::run) executes nothing.
    pub host_pause: Option<HostPause>,

    /// The interpreter result when finished executing
    pub result: Option<RawValue>,

    /// IR address of the instruction that caused the last trap.
    /// Cleared when a new call is invoked or the engine is reset.
    /// [`Engine::backtrace`] reports it.
    pub(crate) trap_pc: Option<JumpTarget>,

    /// Frame pointer of the function that caused the last trap, saved before the
    /// trap reset zeroes [`Engine::fp`]. Only meaningful while `trap_pc` is
    /// `Some`.
    pub(crate) trap_fp: u32,
}

/// A host call that paused the engine, waiting on [`Engine::resume`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostPause {
    /// The result type the paused host function declared, which the resume value
    /// must match.
    pub result: ResultType,
    /// The instruction after the call, where execution continues once resumed.
    pub resume_pc: JumpTarget,
}
