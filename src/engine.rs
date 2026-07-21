use crate::{JumpTarget, Memory, ModuleRef, RawValue, Rc, Stack, Store, TableElement};

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

    /// The interpreter result when finished executing
    pub result: Option<RawValue>,
}
