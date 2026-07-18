use crate::*;
use core::ops::ControlFlow;

impl LocalVariable {
    fn addr(&self, fp: u32) -> usize {
        (fp as i32 + self.frame_offset as i32) as usize
    }
}

pub struct InterpreterState<'store> {
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
    pub store: &'store mut Store,

    /// The interpreter result when finished executing
    pub result: Option<RawValue>,
}

struct CallFrame {
    frame_length: u16,
    module_delta: u8,
    parameter_size: u8,
}

impl CallFrame {
    pub fn from_bits(bits: u32) -> CallFrame {
        // SAFETY: We are converting from the serialized form. The sizes are checked at compile time.
        unsafe { core::mem::transmute(bits) }
    }

    // SAFETY: We are converting to the serialized form. The sizes are checked at compile time.
    pub fn into_bits(self) -> u32 {
        unsafe { core::mem::transmute(self) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvokeError {
    /// The number of parameters passed to the invocation does not match the definition
    ParamLenMismatch,
    /// A parameter value's type does not match the definition in the module
    ParamTypeMismatch,
    /// The function requires more stack space than the interpreter has remaining
    StackOverflow,
}

impl<'store> InterpreterState<'store> {
    fn call_impl_enter_module(&mut self, f_ref: WasmRef) -> Result<(), InterpreterBreak> {
        // If we are calling across module we need to swap out the current memory
        let module_delta = if f_ref.module == self.module {
            0
        } else {
            // Swap to the extern module's context
            let delta = f_ref.module.0.wrapping_sub(self.module.0);
            self.module = f_ref.module;
            self.memory = self.store.get_memory(self.module).clone();
            self.table = self.store.get_table(self.module).clone();
            delta
        };

        self.call_impl(module_delta, f_ref.index)
    }

    /// Call a function within the _current_ module
    fn call_impl(&mut self, module_delta: u8, index: u16) -> Result<(), InterpreterBreak> {
        // Make sure we have enough stack space for the function call
        let m = &self.store.modules()[self.module.0 as usize];
        let f = &m.functions[index as usize];
        let required_stack_space = f.stack_usage as usize + 2 + f.local_size as usize;
        if self.stack.len() < self.sp + required_stack_space {
            return Err(InterpreterBreak::Trap(TrapReason::StackOverflow));
        }

        // The arguments are already at the top of the stack
        // We need to push the frame pointer and the return instruction pointer to the stack
        // We also encode the parameter size into the stack frame so that the return can unwind the stack
        let frame_length = (self.sp - self.fp as usize) as u32;
        assert!(frame_length <= 0xFFFFF);
        let frame = CallFrame {
            frame_length: frame_length as u16,
            module_delta,
            parameter_size: f.parameter_size,
        };

        self.stack.write_u32(self.sp, frame.into_bits());
        self.stack.write_u32(self.sp + 1, self.pc.0);
        self.fp = self.sp as u32;

        // Zero out the local variables
        for i in 0..(f.local_size as usize) {
            self.stack.write_u32(self.sp + 2 + i, 0);
        }

        // Allocate space for frame and the local variables
        self.sp += 2 + f.local_size as usize;

        // Jump to the function's execution point
        self.pc = f.expr.0;
        self.jumped = true;

        Ok(())
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

    /// Invoke a function with some parameters.
    /// This function can only be used to kick off the interpreter.
    /// It cannot be invoked once the interpreter has started.
    pub fn invoke(&mut self, f_ref: WasmRef, params: &[Value]) -> Result<(), InvokeError> {
        // Make sure we are looking at the sentinel program counter
        // This is only the case when nothing is running
        assert_eq!(self.pc, JumpTarget::SENTINEL);
        assert_eq!(self.sp, 0);
        assert_eq!(self.fp, 0);

        let m = &self.store.modules()[f_ref.module.0 as usize];
        let f = &m.functions[f_ref.index as usize];

        let ty = &m.types[f.ty.0 as usize];

        if ty.params.len() != params.len() {
            return Err(InvokeError::ParamLenMismatch);
        }

        for (pi, pd) in params.iter().zip(&ty.params) {
            match (*pi, *pd) {
                (Value::I32(v), ValType::I32) => {
                    self.stack.write_u32(self.sp, v as u32);
                    self.sp += 1;
                }
                (Value::I64(v), ValType::I64) => {
                    self.stack.write_u64(self.sp, v as u64);
                    self.sp += 2;
                }
                (Value::F32(v), ValType::F32) => {
                    self.stack.write_f32(self.sp, v);
                    self.sp += 1;
                }
                (Value::F64(v), ValType::F64) => {
                    self.stack.write_f64(self.sp, v);
                    self.sp += 2;
                }
                _ => {
                    return Err(InvokeError::ParamTypeMismatch);
                }
            }
        }

        // Swap to the extern module's context
        self.module = f_ref.module;
        self.memory = self.store.get_memory(self.module).clone();
        self.table = self.store.get_table(self.module).clone();
        self.call_impl(0, f_ref.index)
            .map_err(|_| InvokeError::StackOverflow)?;
        self.jumped = false;
        self.result = None;

        Ok(())
    }
}

pub struct Interpreter<'store>(core::marker::PhantomData<&'store ()>);

impl<'store> Default for Interpreter<'store> {
    fn default() -> Self {
        Interpreter(core::marker::PhantomData)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrapReason {
    /// Triggered by unreachable instruction
    Unreachable,
    /// A host function has noted an unrecoverable failure
    Host,
    /// Integer or floating point division by zero
    DivideByZero,
    /// An indirect call tried to map to a table function out of range
    InvalidTableIndex,
    /// The function type in an indirect call does not match the function pointer's type
    InvalidTableFunctionType,
    /// An indirect call tried to map to a table function out of range
    UninitializedTableElement,
    /// An imported global could not be read
    GlobalGetFailed,
    /// An imported global could not be set
    GlobalSetFailed,
    /// A memory operation is out of bounds
    OutOfMemory,
    /// memory.grow failed because a host function has taken ownership of a memory
    MemoryRefNotUnique,
    /// A memory operation is out of bounds
    MemoryOutOfBounds,
    /// Ran out of stack space
    StackOverflow,
    /// Attempting to convert Inf to integer
    UnrepresentableResult,
    /// Signed division causes integer overflow
    IntegerOverflow,
    /// Attempting to convert NaN to integer
    BadConversionToInteger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterpreterBreak {
    /// The program has completed
    Finished,
    /// The program has been aborted
    Trap(TrapReason),
    /// An instruction or host function has requested the interpreter to pause
    Pause,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterpreterResult {
    /// The program has completed
    Finished,
    /// The program has been aborted
    Trap(TrapReason),
    /// An instruction or host function has requested the interpreter to pause
    Pause,
    /// No more fuel (ran to instruction bound)
    OutOfFuel,
    /// Failed to read an instruction from memory
    ReaderError(IrReaderError),
}

impl<'store> Interpreter<'store> {
    /// This function performs an unconditional branch to a resolved label target.
    ///
    /// Label targets use a relative offset from their location in the IR. The program
    /// counter is not in the same position as the label target offset is assuming.
    /// For this reason there is an additional label_pc_offset needed to compute the final
    /// program counter to jump to.
    ///
    /// # Arguments
    ///
    /// * `label_pc_offset`: Number of IR words between the current PC (instruction start)
    ///   and this label target.
    /// * `addr`: Resolved label target to jump to
    /// * `state`: Current interpreter state
    ///
    /// returns: Result<(), InterpreterBreak>
    fn br_impl(
        &self,
        label_pc_offset: JumpOffset,
        addr: LabelTarget,
        state: &mut InterpreterState<'store>,
    ) -> Result<(), InterpreterBreak> {
        if addr.is_sentinel() {
            return self.return_(addr.arity() as u8, state);
        }

        let depth = addr.depth() as usize;
        state.sp -= depth;

        // Copy the results to the stack location we are switching to
        match addr.arity() {
            LabelArity::None => {}
            LabelArity::I32 => {
                let w = state.stack.read_u32(state.sp + depth - 1);
                state.stack.write_u32(state.sp, w);
                state.sp += 1;
            }
            LabelArity::I64 => {
                let w1 = state.stack.read_u32(state.sp + depth - 2);
                let w2 = state.stack.read_u32(state.sp + depth - 1);

                state.stack.write_u32(state.sp, w1);
                state.stack.write_u32(state.sp + 1, w2);

                state.sp += 2;
            }
        }

        state.pc += label_pc_offset;
        state.pc += addr.jump();
        state.jumped = true;
        Ok(())
    }
}

/// This is a meta-trait that provides an auto implementation of run() for all IrVisitors
/// of a certain shape.
///
/// For all types that implement [IrVisitor<State = InterpreterState, Error = InstructionError>],
/// this trait will be implemented to execute instructions given the state and store.
pub trait InterpreterRunner<'store> {
    fn run(
        &self,
        code: &[Box<TextPage>],
        state: &mut InterpreterState<'store>,
        n_instructions: usize,
    ) -> InterpreterResult;
}

impl<'store, T: IrVisitor<State = InterpreterState<'store>, Error = InterpreterBreak>>
    InterpreterRunner<'store> for T
{
    fn run(
        &self,
        code: &[Box<TextPage>],
        state: &mut T::State,
        n_instructions: usize,
    ) -> InterpreterResult {
        let reader = IrReader::new(code);

        // Run up to n instructions
        for _ in 0..n_instructions {
            let mut pc = state.pc;

            // If PC is SENTINEL, we're not executing anything
            if pc == JumpTarget::SENTINEL {
                return InterpreterResult::Finished;
            }

            let i_res = reader.visit_instruction(state, &mut pc, self);
            if state.jumped {
                // We jumped, leave the PC
                state.jumped = false;
            } else {
                // Increment the program counter
                state.pc = pc;
            }

            match i_res {
                Ok(_) => {}
                Err(InterpreterBreak::Trap(trap_reason)) => {
                    // TODO(tumbar) How do we expose a backtrace?
                    // We trapped, we need to unwind and reset the state
                    state.sp = 0;
                    state.pc = JumpTarget::SENTINEL;
                    state.fp = 0;
                    state.jumped = false;
                    return InterpreterResult::Trap(trap_reason);
                }
                Err(InterpreterBreak::Pause) => return InterpreterResult::Pause,
                Err(InterpreterBreak::Finished) => return InterpreterResult::Finished,
            }
        }

        InterpreterResult::OutOfFuel
    }
}

impl From<MemoryError> for InterpreterBreak {
    fn from(e: MemoryError) -> Self {
        match e {
            MemoryError::OutOfBounds => InterpreterBreak::Trap(TrapReason::MemoryOutOfBounds),
            MemoryError::OutOfMemory => InterpreterBreak::Trap(TrapReason::OutOfMemory),
            _ => unreachable!(),
        }
    }
}

impl From<HostFunctionBreak> for InterpreterBreak {
    fn from(err: HostFunctionBreak) -> Self {
        match err {
            HostFunctionBreak::Trap => InterpreterBreak::Trap(TrapReason::Host),
            HostFunctionBreak::Pause => InterpreterBreak::Pause,
        }
    }
}

impl From<TrapReason> for InterpreterBreak {
    fn from(err: TrapReason) -> Self {
        InterpreterBreak::Trap(err)
    }
}

macro_rules! instruction {
    ($name:ident, f32 -> f32, $f:ident, $( $t:tt )*) => {
        fn $name(&self, state: &mut Self::State) -> Result<(), Self::Error> {
            let $f = state.stack.read_f32(state.sp - 1);
            state.stack.write_f32(state.sp - 1, $($t)*);
            Ok(())
        }
    };
    ($name:ident, i32 -> i32, $i:ident, $( $t:tt )*) => {
        fn $name(&self, state: &mut Self::State) -> Result<(), Self::Error> {
            let $i = state.stack.read_u32(state.sp - 1) as i32;
            state.stack.write_u32(state.sp - 1, ($($t)*) as u32);
            Ok(())
        }
    };
    ($name:ident, f64 -> f64, $f:ident, $( $t:tt )*) => {
        fn $name(&self, state: &mut Self::State) -> Result<(), Self::Error> {
            let $f = state.stack.read_f64(state.sp - 2);
            state.stack.write_f64(state.sp - 2, $($t)*);
            Ok(())
        }
    };
    ($name:ident, i64 -> i64, $i:ident, $( $t:tt )*) => {
        fn $name(&self, state: &mut Self::State) -> Result<(), Self::Error> {
            let $i = state.stack.read_u64(state.sp - 2) as i64;
            state.stack.write_u64(state.sp - 2, ($($t)*) as u64);
            Ok(())
        }
    };
    ($name:ident, f32, f32 -> f32, $a:ident, $b:ident, $( $t:tt )*) => {
        fn $name(&self, state: &mut Self::State) -> Result<(), Self::Error> {
            state.sp -= 1;
            let $b = state.stack.read_f32(state.sp);
            let $a = state.stack.read_f32(state.sp - 1);
            state.stack.write_f32(state.sp - 1, $($t)*);
            Ok(())
        }
    };
    ($name:ident, i32, i32 -> i32, $a:ident, $b:ident, $( $t:tt )*) => {
        fn $name(&self, state: &mut Self::State) -> Result<(), Self::Error> {
            state.sp -= 1;
            let $b = state.stack.read_u32(state.sp) as i32;
            let $a = state.stack.read_u32(state.sp - 1) as i32;
            state.stack.write_u32(state.sp - 1, ($($t)*) as u32);
            Ok(())
        }
    };
    ($name:ident, i32 -> bool, $i:ident, $( $t:tt )*) => {
        fn $name(&self, state: &mut Self::State) -> Result<(), Self::Error> {
            let $i = state.stack.read_u32(state.sp - 1) as i32;
            state.stack.write_u32(state.sp - 1, if $($t)* { 1 } else { 0 });
            Ok(())
        }
    };
    ($name:ident, i64 -> bool, $i:ident, $( $t:tt )*) => {
        fn $name(&self, state: &mut Self::State) -> Result<(), Self::Error> {
            state.sp -= 1;
            let $i = state.stack.read_u64(state.sp - 1) as i64;
            state.stack.write_u32(state.sp - 1, if $($t)* { 1 } else { 0 });
            Ok(())
        }
    };
    ($name:ident, i32, i32 -> bool, $a:ident, $b:ident, $( $t:tt )*) => {
        fn $name(&self, state: &mut Self::State) -> Result<(), Self::Error> {
            state.sp -= 1;
            let $b = state.stack.read_u32(state.sp) as i32;
            let $a = state.stack.read_u32(state.sp - 1) as i32;
            state.stack.write_u32(state.sp - 1, if $($t)* { 1 } else { 0 });
            Ok(())
        }
    };
    ($name:ident, f32, f32 -> bool, $a:ident, $b:ident, $( $t:tt )*) => {
        fn $name(&self, state: &mut Self::State) -> Result<(), Self::Error> {
            state.sp -= 1;
            let $b = state.stack.read_f32(state.sp);
            let $a = state.stack.read_f32(state.sp - 1);
            state.stack.write_u32(state.sp - 1, if $($t)* { 1 } else { 0 });
            Ok(())
        }
    };
    ($name:ident, f64, f64 -> f64, $a:ident, $b:ident, $( $t:tt )*) => {
        fn $name(&self, state: &mut Self::State) -> Result<(), Self::Error> {
            state.sp -= 2;
            let $b = state.stack.read_f64(state.sp);
            let $a = state.stack.read_f64(state.sp - 2);
            state.stack.write_f64(state.sp - 2, $($t)*);
            Ok(())
        }
    };
    ($name:ident, i64, i64 -> i64, $a:ident, $b:ident, $( $t:tt )*) => {
        fn $name(&self, state: &mut Self::State) -> Result<(), Self::Error> {
            state.sp -= 2;
            let $b = state.stack.read_u64(state.sp) as i64;
            let $a = state.stack.read_u64(state.sp - 2) as i64;
            state.stack.write_u64(state.sp - 2, ($($t)*) as u64);
            Ok(())
        }
    };
    ($name:ident, i64, i64 -> bool, $a:ident, $b:ident, $( $t:tt )*) => {
        fn $name(&self, state: &mut Self::State) -> Result<(), Self::Error> {
            state.sp -= 3;
            let $b = state.stack.read_u64(state.sp + 1) as i64;
            let $a = state.stack.read_u64(state.sp - 1) as i64;
            state.stack.write_u32(state.sp - 1, if $($t)* { 1 } else { 0 });
            Ok(())
        }
    };
    ($name:ident, f64, f64 -> bool, $a:ident, $b:ident, $( $t:tt )*) => {
        fn $name(&self, state: &mut Self::State) -> Result<(), Self::Error> {
            state.sp -= 3;
            let $b = state.stack.read_f64(state.sp + 1);
            let $a = state.stack.read_f64(state.sp - 1);
            state.stack.write_u32(state.sp - 1, if $($t)* { 1 } else { 0 });
            Ok(())
        }
    };
}

impl<'store> BaseVisitor for Interpreter<'store> {
    type Error = InterpreterBreak;
    type State = InterpreterState<'store>;

    fn unreachable(&self, _: &mut Self::State) -> Result<(), Self::Error> {
        Err(InterpreterBreak::Trap(TrapReason::Unreachable))
    }

    fn nop(&self, _: &mut Self::State) -> Result<(), Self::Error> {
        Ok(())
    }

    fn i32_load(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        let addr = Memory::effective_address(state.stack.read_u32(state.sp - 1), m.offset)?;
        let val = state.memory.load_u32(addr)?;
        state.stack.write_u32(state.sp - 1, val);
        Ok(())
    }

    fn i64_load(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        let addr = Memory::effective_address(state.stack.read_u32(state.sp - 1), m.offset)?;
        let val = state.memory.load_u64(addr)?;
        state.stack.write_u64(state.sp - 1, val);
        state.sp += 1;
        Ok(())
    }

    fn f32_load(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        let addr = Memory::effective_address(state.stack.read_u32(state.sp - 1), m.offset)?;
        let val = state.memory.load_u32(addr)?;
        state.stack.write_u32(state.sp - 1, val);
        Ok(())
    }

    fn f64_load(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        let addr = Memory::effective_address(state.stack.read_u32(state.sp - 1), m.offset)?;
        let val = state.memory.load_u64(addr)?;
        state.stack.write_u64(state.sp - 1, val);
        state.sp += 1;
        Ok(())
    }

    fn i32_load8_s(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        let addr = Memory::effective_address(state.stack.read_u32(state.sp - 1), m.offset)?;
        let val = state.memory.load_u8(addr)? as i8 as i32;
        state.stack.write_u32(state.sp - 1, val as u32);
        Ok(())
    }

    fn i32_load8_u(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        let addr = Memory::effective_address(state.stack.read_u32(state.sp - 1), m.offset)?;
        let val = state.memory.load_u8(addr)? as u32;
        state.stack.write_u32(state.sp - 1, val);
        Ok(())
    }

    fn i32_load16_s(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        let addr = Memory::effective_address(state.stack.read_u32(state.sp - 1), m.offset)?;
        let val = state.memory.load_u16(addr)? as i16 as i32;
        state.stack.write_u32(state.sp - 1, val as u32);
        Ok(())
    }

    fn i32_load16_u(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        let addr = Memory::effective_address(state.stack.read_u32(state.sp - 1), m.offset)?;
        let val = state.memory.load_u16(addr)? as u32;
        state.stack.write_u32(state.sp - 1, val);
        Ok(())
    }

    fn i64_load8_s(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        let addr = Memory::effective_address(state.stack.read_u32(state.sp - 1), m.offset)?;
        let val = state.memory.load_u8(addr)? as i8 as i64 as u64;
        state.stack.write_u64(state.sp - 1, val);
        state.sp += 1;
        Ok(())
    }

    fn i64_load8_u(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        let addr = Memory::effective_address(state.stack.read_u32(state.sp - 1), m.offset)?;
        let val = state.memory.load_u8(addr)? as u64;
        state.stack.write_u64(state.sp - 1, val);
        state.sp += 1;
        Ok(())
    }

    fn i64_load16_s(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        let addr = Memory::effective_address(state.stack.read_u32(state.sp - 1), m.offset)?;
        let val = state.memory.load_u16(addr)? as i16 as i64 as u64;
        state.stack.write_u64(state.sp - 1, val);
        state.sp += 1;
        Ok(())
    }

    fn i64_load16_u(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        let addr = Memory::effective_address(state.stack.read_u32(state.sp - 1), m.offset)?;
        let val = state.memory.load_u16(addr)? as u64;
        state.stack.write_u64(state.sp - 1, val);
        state.sp += 1;
        Ok(())
    }

    fn i64_load32_s(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        let addr = Memory::effective_address(state.stack.read_u32(state.sp - 1), m.offset)?;
        let val = state.memory.load_u32(addr)? as i32 as i64 as u64;
        state.stack.write_u64(state.sp - 1, val);
        state.sp += 1;
        Ok(())
    }

    fn i64_load32_u(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        let addr = Memory::effective_address(state.stack.read_u32(state.sp - 1), m.offset)?;
        let val = state.memory.load_u32(addr)? as u64;
        state.stack.write_u64(state.sp - 1, val);
        state.sp += 1;
        Ok(())
    }

    fn i32_store(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 2;
        let val = state.stack.read_u32(state.sp + 1);
        let addr = Memory::effective_address(state.stack.read_u32(state.sp), m.offset)?;
        state.memory.store_u32(addr, val)?;
        Ok(())
    }

    fn i64_store(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 3;
        let val = state.stack.read_u64(state.sp + 1);
        let addr = Memory::effective_address(state.stack.read_u32(state.sp), m.offset)?;
        state.memory.store_u64(addr, val)?;
        Ok(())
    }

    fn f32_store(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 2;
        let val = state.stack.read_u32(state.sp + 1);
        let addr = Memory::effective_address(state.stack.read_u32(state.sp), m.offset)?;
        state.memory.store_u32(addr, val)?;
        Ok(())
    }

    fn f64_store(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 3;
        let val = state.stack.read_u64(state.sp + 1);
        let addr = Memory::effective_address(state.stack.read_u32(state.sp), m.offset)?;
        state.memory.store_u64(addr, val)?;
        Ok(())
    }

    fn i32_store8(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 2;
        let val = state.stack.read_u32(state.sp + 1) as u8;
        let addr = Memory::effective_address(state.stack.read_u32(state.sp), m.offset)?;
        state.memory.store_u8(addr, val)?;
        Ok(())
    }

    fn i32_store16(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 2;
        let val = state.stack.read_u32(state.sp + 1) as u16;
        let addr = Memory::effective_address(state.stack.read_u32(state.sp), m.offset)?;
        state.memory.store_u16(addr, val)?;
        Ok(())
    }

    fn i64_store8(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 3;
        let val = state.stack.read_u64(state.sp + 1) as u8;
        let addr = Memory::effective_address(state.stack.read_u32(state.sp), m.offset)?;
        state.memory.store_u8(addr, val)?;
        Ok(())
    }

    fn i64_store16(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 3;
        let val = state.stack.read_u64(state.sp + 1) as u16;
        let addr = Memory::effective_address(state.stack.read_u32(state.sp), m.offset)?;
        state.memory.store_u16(addr, val)?;
        Ok(())
    }

    fn i64_store32(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 3;
        let val = state.stack.read_u64(state.sp + 1) as u32;
        let addr = Memory::effective_address(state.stack.read_u32(state.sp), m.offset)?;
        state.memory.store_u32(addr, val)?;
        Ok(())
    }

    fn memory_size(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        state.stack.write_u32(state.sp, state.memory.size());
        state.sp += 1;
        Ok(())
    }

    fn memory_grow(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let n = state.stack.read_u32(state.sp - 1);

        // To grow memory we need to mutate the inside of the Rc<Memory>
        // This means we need unique access to it
        // Theoretically there should only be two references to this memory:
        // 1. In the owning module/host module
        // 2. In the interpreter state

        // We need to drop the reference from the interpreter state and then try to
        // get mutable unqiue access to the Memory. If there is anything else holding
        // on to the reference (i.e. a host module took a pointer, we will not be
        // able to grow the memory)

        // Drop the memory reference
        state.clear_memory();

        // Look up what _should_ be the final unique reference to this memory
        let memory = state.store.get_memory_mut(state.module);
        let result: Result<u32, MemoryError> = if memory.is_zero() {
            Err(MemoryError::OutOfMemory)
        } else if let Some(memory) = memory.get_mut() {
            memory.grow(n)
        } else {
            // This is probably caused by a host function holding onto a memory reference
            // We could panic here... I'd rather just error gracefully.
            return Err(InterpreterBreak::Trap(TrapReason::MemoryRefNotUnique));
        };

        state.memory = memory.clone();

        match result {
            Ok(old_size) => {
                state.stack.write_u32(state.sp - 1, old_size);
            }
            Err(_) => {
                state.stack.write_u32(state.sp - 1, 0xFFFF_FFFF);
            }
        }

        Ok(())
    }

    fn i32_const(&self, n: i32, state: &mut Self::State) -> Result<(), Self::Error> {
        state.stack.write_u32(state.sp, n as u32);
        state.sp += 1;
        Ok(())
    }

    fn i64_const(&self, n: i64, state: &mut Self::State) -> Result<(), Self::Error> {
        state.stack.write_u64(state.sp, n as u64);
        state.sp += 2;
        Ok(())
    }

    fn f32_const(&self, z: f32, state: &mut Self::State) -> Result<(), Self::Error> {
        state.stack.write_f32(state.sp, z);
        state.sp += 1;
        Ok(())
    }

    fn f64_const(&self, z: f64, state: &mut Self::State) -> Result<(), Self::Error> {
        state.stack.write_f64(state.sp, z);
        state.sp += 2;
        Ok(())
    }

    instruction!(i32_eqz, i32 -> bool, i, i == 0);
    instruction!(i32_eq, i32, i32 -> bool, a, b, a == b);
    instruction!(i32_ne, i32, i32 -> bool, a, b, a != b);
    instruction!(i32_lt_s, i32, i32 -> bool, a, b, a < b);
    instruction!(i32_lt_u, i32, i32 -> bool, a, b, (a as u32) < (b as u32));
    instruction!(i32_gt_s, i32, i32 -> bool, a, b, a > b);
    instruction!(i32_gt_u, i32, i32 -> bool, a, b, (a as u32) > (b as u32));
    instruction!(i32_le_s, i32, i32 -> bool, a, b, a <= b);
    instruction!(i32_le_u, i32, i32 -> bool, a, b, (a as u32) <= (b as u32));
    instruction!(i32_ge_s, i32, i32 -> bool, a, b, a >= b);
    instruction!(i32_ge_u, i32, i32 -> bool, a, b, (a as u32) >= (b as u32));
    instruction!(i64_eqz, i64 -> bool, i, i == 0);
    instruction!(i64_eq, i64, i64 -> bool, a, b, a == b);
    instruction!(i64_ne, i64, i64 -> bool, a, b, a != b);
    instruction!(i64_lt_s, i64, i64 -> bool, a, b, a < b);
    instruction!(i64_lt_u, i64, i64 -> bool, a, b, (a as u64) < (b as u64));
    instruction!(i64_gt_s, i64, i64 -> bool, a, b, a > b);
    instruction!(i64_gt_u, i64, i64 -> bool, a, b, (a as u64) > (b as u64));
    instruction!(i64_le_s, i64, i64 -> bool, a, b, a <= b);
    instruction!(i64_le_u, i64, i64 -> bool, a, b, (a as u64) <= (b as u64));
    instruction!(i64_ge_s, i64, i64 -> bool, a, b, a >= b);
    instruction!(i64_ge_u, i64, i64 -> bool, a, b, (a as u64) >= (b as u64));
    instruction!(f32_eq, f32, f32 -> bool, a, b, a == b);
    instruction!(f32_ne, f32, f32 -> bool, a, b, a != b);
    instruction!(f32_lt, f32, f32 -> bool, a, b, a < b);
    instruction!(f32_gt, f32, f32 -> bool, a, b, a > b);
    instruction!(f32_le, f32, f32 -> bool, a, b, a <= b);
    instruction!(f32_ge, f32, f32 -> bool, a, b, a >= b);
    instruction!(f64_eq, f64, f64 -> bool, a, b, a == b);
    instruction!(f64_ne, f64, f64 -> bool, a, b, a != b);
    instruction!(f64_lt, f64, f64 -> bool, a, b, a < b);
    instruction!(f64_gt, f64, f64 -> bool, a, b, a > b);
    instruction!(f64_le, f64, f64 -> bool, a, b, a <= b);
    instruction!(f64_ge, f64, f64 -> bool, a, b, a >= b);
    instruction!(i32_clz, i32 -> i32, i, i.leading_zeros() as i32);
    instruction!(i32_ctz, i32 -> i32, i, i.trailing_zeros() as i32);
    instruction!(i32_popcnt, i32 -> i32, i, i.count_ones() as i32);
    instruction!(i32_add, i32, i32 -> i32, a, b, a.wrapping_add(b));
    instruction!(i32_sub, i32, i32 -> i32, a, b, a.wrapping_sub(b));
    instruction!(i32_mul, i32, i32 -> i32, a, b, a.wrapping_mul(b));
    instruction!(i32_div_s, i32, i32 -> i32, a, b, {
        if b == 0 {
            return Err(TrapReason::DivideByZero.into());
        }
        if a == i32::MIN && b == -1 {
            return Err(TrapReason::IntegerOverflow.into());
        }

        a / b
    });
    instruction!(i32_div_u, i32, i32 -> i32, a, b, {
        if b == 0 {
            return Err(InterpreterBreak::Trap(TrapReason::DivideByZero))
        } else {
           ((a as u32) / (b as u32)) as i32
        }
    });
    instruction!(i32_rem_s, i32, i32 -> i32, a, b, {
        if b == 0 {
            return Err(InterpreterBreak::Trap(TrapReason::DivideByZero))
        } else {
            a.wrapping_rem(b)
        }
    });
    instruction!(i32_rem_u, i32, i32 -> i32, a, b, {
        if b == 0 {
            return Err(InterpreterBreak::Trap(TrapReason::DivideByZero))
        } else {
            (a as u32).wrapping_rem(b as u32) as i32
        }
    });
    instruction!(i32_and, i32, i32 -> i32, a, b, a & b);
    instruction!(i32_or, i32, i32 -> i32, a, b, a | b);
    instruction!(i32_xor, i32, i32 -> i32, a, b, a ^ b);
    instruction!(i32_shl, i32, i32 -> i32, a, b, a.wrapping_shl(b as u32));
    instruction!(i32_shr_s, i32, i32 -> i32, a, b, a.wrapping_shr(b as u32));
    instruction!(i32_shr_u, i32, i32 -> i32, a, b, (a as u32).wrapping_shr(b as u32) as i32);
    instruction!(i32_rotl, i32, i32 -> i32, a, b, a.rotate_left(b as u32));
    instruction!(i32_rotr, i32, i32 -> i32, a, b, a.rotate_right(b as u32));
    instruction!(i64_clz, i64 -> i64, i, i.leading_zeros() as i64);
    instruction!(i64_ctz, i64 -> i64, i, i.trailing_zeros() as i64);
    instruction!(i64_popcnt, i64 -> i64, i, i.count_ones() as i64);
    instruction!(i64_add, i64, i64 -> i64, a, b, a.wrapping_add(b));
    instruction!(i64_sub, i64, i64 -> i64, a, b, a.wrapping_sub(b));
    instruction!(i64_mul, i64, i64 -> i64, a, b, a.wrapping_mul(b));
    instruction!(i64_div_s, i64, i64 -> i64, a, b, {
        if b == 0 {
            return Err(TrapReason::DivideByZero.into());
        }
        if a == i64::MIN && b == -1 {
            return Err(TrapReason::IntegerOverflow.into());
        }

        a / b
    });
    instruction!(i64_div_u, i64, i64 -> i64, a, b, {
        if b == 0 {
            return Err(InterpreterBreak::Trap(TrapReason::DivideByZero))
        } else {
            (a as u64).wrapping_div(b as u64) as i64
        }
    });
    instruction!(i64_rem_s, i64, i64 -> i64, a, b, {
        if b == 0 {
            return Err(InterpreterBreak::Trap(TrapReason::DivideByZero))
        } else {
            a.wrapping_rem(b)
        }
    });
    instruction!(i64_rem_u, i64, i64 -> i64, a, b, {
        if b == 0 {
            return Err(InterpreterBreak::Trap(TrapReason::DivideByZero))
        } else {
            (a as u64).wrapping_rem(b as u64) as i64
        }
    });
    instruction!(i64_and, i64, i64 -> i64, a, b, a & b);
    instruction!(i64_or, i64, i64 -> i64, a, b, a | b);
    instruction!(i64_xor, i64, i64 -> i64, a, b, a ^ b);
    instruction!(i64_shl, i64, i64 -> i64, a, b, a.wrapping_shl(b as u32));
    instruction!(i64_shr_s, i64, i64 -> i64, a, b, a.wrapping_shr(b as u32));
    instruction!(i64_shr_u, i64, i64 -> i64, a, b, (a as u64).wrapping_shr(b as u32) as i64);
    instruction!(i64_rotl, i64, i64 -> i64, a, b, a.rotate_left(b as u32));
    instruction!(i64_rotr, i64, i64 -> i64, a, b, a.rotate_right(b as u32));
    instruction!(f32_abs, f32 -> f32, f, libm::fabsf(f));
    instruction!(f32_neg, f32 -> f32, f, -f);
    instruction!(f32_ceil, f32 -> f32, f, {
        if f.is_nan() {
            f32::NAN
        } else {
            libm::ceilf(f)
        }
    });
    instruction!(f32_floor, f32 -> f32, f, {
        if f.is_nan() {
            f32::NAN
        } else {
            libm::floorf(f)
        }
    });
    instruction!(f32_trunc, f32 -> f32, f, {
        if f.is_nan() {
            f32::NAN
        } else {
            libm::truncf(f)
        }
    });
    instruction!(f32_nearest, f32 -> f32, f, {
        if f.is_nan() {
            f32::NAN
        } else {
            libm::rintf(f)
        }
    });
    instruction!(f32_sqrt, f32 -> f32, f, libm::sqrtf(f));
    instruction!(f32_add, f32, f32 -> f32, a, b, a + b);
    instruction!(f32_sub, f32, f32 -> f32, a, b, a - b);
    instruction!(f32_mul, f32, f32 -> f32, a, b, a * b);
    instruction!(f32_div, f32, f32 -> f32, a, b, a / b);
    instruction!(f32_min, f32, f32 -> f32, a, b, {
        if a.is_nan() || b.is_nan() {
            f32::NAN
        } else if a == 0.0 && b == 0.0 {
            if a.to_bits() >> 31 == 1 {
                a
            } else {
                b
            }
        } else {
            a.min(b)
        }
    });
    instruction!(f32_max, f32, f32 -> f32, a, b, {
        if a.is_nan() || b.is_nan() {
            f32::NAN
        } else if a == 0.0 && b == 0.0 {
            if a.to_bits() >> 31 == 1 {
                b
            } else {
                a
            }
        } else {
            a.max(b)
        }
    });
    instruction!(f32_copysign, f32, f32 -> f32, a, b, libm::copysignf(a, b));
    instruction!(f64_abs, f64 -> f64, f, libm::fabs(f));
    instruction!(f64_neg, f64 -> f64, f, -f);
    instruction!(f64_ceil, f64 -> f64, f, {
        if f.is_nan() {
            f64::NAN
        } else {
            libm::ceil(f)
        }
    });
    instruction!(f64_floor, f64 -> f64, f, {
        if f.is_nan() {
            f64::NAN
        } else {
            libm::floor(f)
        }
    });
    instruction!(f64_trunc, f64 -> f64, f, {
        if f.is_nan() {
            f64::NAN
        } else {
            libm::trunc(f)
        }
    });
    instruction!(f64_nearest, f64 -> f64, f, {
        if f.is_nan() {
            f64::NAN
        } else {
            libm::rint(f)
        }
    });
    instruction!(f64_sqrt, f64 -> f64, f, libm::sqrt(f));
    instruction!(f64_add, f64, f64 -> f64, a, b, a + b);
    instruction!(f64_sub, f64, f64 -> f64, a, b, a - b);
    instruction!(f64_mul, f64, f64 -> f64, a, b, a * b);
    instruction!(f64_div, f64, f64 -> f64, a, b, a / b);
    instruction!(f64_min, f64, f64 -> f64, a, b, {
        if a.is_nan() || b.is_nan() {
            f64::NAN
        } else if a == 0.0 && b == 0.0 {
            if a.to_bits() >> 63 == 1 {
                a
            } else {
                b
            }
        } else {
            a.min(b)
        }
    });
    instruction!(f64_max, f64, f64 -> f64, a, b, {
        if a.is_nan() || b.is_nan() {
            f64::NAN
        } else if a == 0.0 && b == 0.0 {
            if a.to_bits() >> 63 == 1 {
                b
            } else {
                a
            }
        } else {
            a.max(b)
        }
    });
    instruction!(f64_copysign, f64, f64 -> f64, a, b, libm::copysign(a, b));

    fn i32_wrap_i64(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        // i64 low word is at [sp-2], high word at [sp-1]
        // After decrement, low word is at [sp-1] (where we want the i32)
        state.sp -= 1;
        Ok(())
    }

    fn i32_trunc_f32_s(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let f = state.stack.read_f32(state.sp - 1);
        if f.is_infinite() {
            return Err(TrapReason::UnrepresentableResult.into());
        }
        if f.is_nan() {
            return Err(TrapReason::BadConversionToInteger.into());
        }
        if f >= 2147483648.0f32 || f <= -2147483904.0f32 {
            return Err(TrapReason::UnrepresentableResult.into());
        }

        state.stack.write_u32(state.sp - 1, f as i32 as u32);
        Ok(())
    }

    fn i32_trunc_f32_u(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let f = state.stack.read_f32(state.sp - 1);
        if f.is_infinite() {
            return Err(TrapReason::UnrepresentableResult.into());
        }
        if f.is_nan() {
            return Err(TrapReason::BadConversionToInteger.into());
        }
        if f >= 4294967296.0f32 || f <= -1.0f32 {
            return Err(TrapReason::UnrepresentableResult.into());
        }

        state.stack.write_u32(state.sp - 1, f as u32);
        Ok(())
    }

    fn i32_trunc_f64_s(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 1;
        let f = state.stack.read_f64(state.sp - 1);
        if f.is_infinite() {
            return Err(TrapReason::UnrepresentableResult.into());
        }
        if f.is_nan() {
            return Err(TrapReason::BadConversionToInteger.into());
        }
        if f >= 2147483648.0f64 || f <= -2147483649.0f64 {
            return Err(TrapReason::UnrepresentableResult.into());
        }
        state.stack.write_u32(state.sp - 1, f as i32 as u32);
        Ok(())
    }

    fn i32_trunc_f64_u(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 1;
        let f = state.stack.read_f64(state.sp - 1);
        if f.is_infinite() {
            return Err(TrapReason::UnrepresentableResult.into());
        }
        if f.is_nan() {
            return Err(TrapReason::BadConversionToInteger.into());
        }
        if f >= 4294967296.0f64 || f <= -1.0f64 {
            return Err(TrapReason::UnrepresentableResult.into());
        }

        state.stack.write_u32(state.sp - 1, f as u32);
        Ok(())
    }

    fn i64_extend_i32_s(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let i = state.stack.read_u32(state.sp - 1) as i32;
        let extended = i as i64 as u64;
        state.stack.write_u64(state.sp - 1, extended);
        state.sp += 1;
        Ok(())
    }

    fn i64_extend_i32_u(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        // Low word is already in place at [sp-1]
        // Just add high word as 0 for unsigned extension
        state.stack.write_u32(state.sp, 0);
        state.sp += 1;
        Ok(())
    }

    fn i64_trunc_f32_s(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let f = state.stack.read_f32(state.sp - 1);
        if f.is_infinite() {
            return Err(TrapReason::UnrepresentableResult.into());
        }
        if f.is_nan() {
            return Err(TrapReason::BadConversionToInteger.into());
        }
        if f >= 9223372036854775808.0f32 || f <= -9223373136366403584.0f32 {
            return Err(TrapReason::UnrepresentableResult.into());
        }

        let i = f as i64 as u64;
        state.stack.write_u64(state.sp - 1, i);
        state.sp += 1;
        Ok(())
    }

    fn i64_trunc_f32_u(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let f = state.stack.read_f32(state.sp - 1);
        if f.is_infinite() {
            return Err(TrapReason::UnrepresentableResult.into());
        }
        if f.is_nan() {
            return Err(TrapReason::BadConversionToInteger.into());
        }
        if f >= 18446744073709551616.0f32 || f <= -1.0f32 {
            return Err(TrapReason::UnrepresentableResult.into());
        }

        let u = f as u64;
        state.stack.write_u64(state.sp - 1, u);
        state.sp += 1;
        Ok(())
    }

    fn i64_trunc_f64_s(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let f = state.stack.read_f64(state.sp - 2);
        if f.is_infinite() {
            return Err(TrapReason::UnrepresentableResult.into());
        }
        if f.is_nan() {
            return Err(TrapReason::BadConversionToInteger.into());
        }
        if f >= 9223372036854775808.0f64 || f <= -9223372036854777856.0f64 {
            return Err(TrapReason::UnrepresentableResult.into());
        }

        let i = f as i64 as u64;
        state.stack.write_u64(state.sp - 2, i);
        Ok(())
    }

    fn i64_trunc_f64_u(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let f = state.stack.read_f64(state.sp - 2);
        if f.is_infinite() {
            return Err(TrapReason::UnrepresentableResult.into());
        }
        if f.is_nan() {
            return Err(TrapReason::BadConversionToInteger.into());
        }
        if f >= 18446744073709551616.0f64 || f <= -1.0f64 {
            return Err(TrapReason::UnrepresentableResult.into());
        }

        let u = f as u64;
        state.stack.write_u64(state.sp - 2, u);
        Ok(())
    }

    fn f32_convert_i32_s(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let i = state.stack.read_u32(state.sp - 1) as i32;
        state.stack.write_f32(state.sp - 1, i as f32);
        Ok(())
    }

    fn f32_convert_i32_u(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let u = state.stack.read_u32(state.sp - 1);
        state.stack.write_f32(state.sp - 1, u as f32);
        Ok(())
    }

    fn f32_convert_i64_s(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 1;
        let i = state.stack.read_u64(state.sp - 1) as i64;
        state.stack.write_f32(state.sp - 1, i as f32);
        Ok(())
    }

    fn f32_convert_i64_u(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 1;
        let u = state.stack.read_u64(state.sp - 1);
        state.stack.write_f32(state.sp - 1, u as f32);
        Ok(())
    }

    fn f32_demote_f64(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 1;
        let f = state.stack.read_f64(state.sp - 1);
        state.stack.write_f32(state.sp - 1, f as f32);
        Ok(())
    }

    fn f64_convert_i32_s(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let i = state.stack.read_u32(state.sp - 1) as i32;
        state.stack.write_f64(state.sp - 1, i as f64);
        state.sp += 1;
        Ok(())
    }

    fn f64_convert_i32_u(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let u = state.stack.read_u32(state.sp - 1);
        state.stack.write_f64(state.sp - 1, u as f64);
        state.sp += 1;
        Ok(())
    }

    fn f64_convert_i64_s(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let i = state.stack.read_u64(state.sp - 2) as i64;
        state.stack.write_f64(state.sp - 2, i as f64);
        Ok(())
    }

    fn f64_convert_i64_u(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let u = state.stack.read_u64(state.sp - 2);
        state.stack.write_f64(state.sp - 2, u as f64);
        Ok(())
    }

    fn f64_promote_f32(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let f = state.stack.read_f32(state.sp - 1);
        state.stack.write_f64(state.sp - 1, f as f64);
        state.sp += 1;
        Ok(())
    }

    // Non-trapping float-to-int conversions
    fn i32_trunc_sat_f32_s(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let f = state.stack.read_f32(state.sp - 1);
        state.stack.write_u32(state.sp - 1, f as i32 as u32);
        Ok(())
    }

    fn i32_trunc_sat_f32_u(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let f = state.stack.read_f32(state.sp - 1);
        state.stack.write_u32(state.sp - 1, f as u32);
        Ok(())
    }

    fn i32_trunc_sat_f64_s(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 1;
        let f = state.stack.read_f64(state.sp - 1);
        state.stack.write_u32(state.sp - 1, f as i32 as u32);
        Ok(())
    }

    fn i32_trunc_sat_f64_u(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 1;
        let f = state.stack.read_f64(state.sp - 1);
        state.stack.write_u32(state.sp - 1, f as u32);
        Ok(())
    }

    fn i64_trunc_sat_f32_s(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let f = state.stack.read_f32(state.sp - 1);
        state.stack.write_u64(state.sp - 1, f as i64 as u64);
        state.sp += 1;
        Ok(())
    }

    fn i64_trunc_sat_f32_u(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let f = state.stack.read_f32(state.sp - 1);
        state.stack.write_u64(state.sp - 1, f as u64);
        state.sp += 1;
        Ok(())
    }

    fn i64_trunc_sat_f64_s(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let f = state.stack.read_f64(state.sp - 2);
        state.stack.write_u64(state.sp - 2, f as i64 as u64);
        Ok(())
    }

    fn i64_trunc_sat_f64_u(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let f = state.stack.read_f64(state.sp - 2);
        state.stack.write_u64(state.sp - 2, f as u64);
        Ok(())
    }
}

impl<'store> IrVisitor for Interpreter<'store> {
    fn drop(&self, ty: ValType, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= match ty {
            ValType::I32 | ValType::F32 => 1,
            ValType::I64 | ValType::F64 => 2,
        };
        Ok(())
    }

    fn select(&self, ty: ValType, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 1;
        let c = state.stack.read_u32(state.sp);

        match ty {
            ValType::I32 | ValType::F32 => {
                if c != 0 {
                    // Use val1 which is already in the right spot
                    state.sp -= 1;
                } else {
                    // Move val2 to val1's spot
                    let val2 = state.stack.read_u32(state.sp - 1);
                    state.stack.write_u32(state.sp - 2, val2);
                    state.sp -= 1;
                }
            }
            ValType::I64 | ValType::F64 => {
                if c != 0 {
                    // Use val1 which is already in the right spot
                    state.sp -= 2;
                } else {
                    // Move val2 to val1's spot
                    let val2 = state.stack.read_u64(state.sp - 2);
                    state.stack.write_u64(state.sp - 4, val2);
                    state.sp -= 2;
                }
            }
        }

        Ok(())
    }

    fn if_(&self, false_address: LabelTarget, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 1;
        let v = state.stack.read_u32(state.sp);
        if v == 0 {
            // No need to perform any result copies or stack truncation
            state.pc += JumpOffset::offset(1);
            state.pc += false_address.jump();
            state.jumped = true;
        }

        Ok(())
    }

    fn br(&self, addr: LabelTarget, state: &mut Self::State) -> Result<(), Self::Error> {
        self.br_impl(JumpOffset::offset(1), addr, state)
    }

    fn br_if(&self, true_address: LabelTarget, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 1;
        let v = state.stack.read_u32(state.sp);
        if v != 0 {
            self.br_impl(JumpOffset::offset(1), true_address, state)?;
        }

        Ok(())
    }

    fn br_table(
        &self,
        n: u32,
        cases: impl FnOnce(u32) -> LabelTarget,
        state: &mut Self::State,
    ) -> Result<(), Self::Error> {
        state.sp -= 1;
        let v = state.stack.read_u32(state.sp);
        let label = cases(v);

        // If the count is greater than 255, the br_table was encoded as a 16-bit extended immediate.
        // We need to offset the jump by an additional word if this is the case.
        let op_offset = if n >= 0xFF { 2 } else { 1 };

        if v < n {
            // A standard case, compute the offset from the current PC
            // Instruction opcode offset + previous cases (each 2 words)
            self.br_impl(JumpOffset::offset(op_offset + (2 * v as i32)), label, state)?;
        } else {
            // The default case, all cases together
            self.br_impl(JumpOffset::offset(op_offset + (2 * n as i32)), label, state)?;
        }

        Ok(())
    }

    fn return_(&self, return_size: u8, state: &mut Self::State) -> Result<(), Self::Error> {
        let return_size = return_size as usize;

        let fp = state.fp as usize;
        let return_pc = JumpTarget(state.stack.read_u32(state.fp as usize + 1));

        // The frame pointer on the stack actually encodes ((sp - fp) << 16) | prm_size
        let call_frame = CallFrame::from_bits(state.stack.read_u32(state.fp as usize));
        let return_fp = (fp as u32) - (call_frame.frame_length as u32);
        let parameter_start = fp - (call_frame.parameter_size as usize);

        // Copy the return value over the parameters/frame information
        for i in 0..return_size {
            let val = state.stack.read_u32(state.sp - return_size + i);
            state.stack.write_u32(parameter_start + i, val);
        }

        state.fp = return_fp;

        // Check if we are leaving the context of this module
        if call_frame.module_delta != 0 {
            // Restore the old module context outside this frame
            let restore_module = state.module.0.wrapping_sub(call_frame.module_delta);
            state.module = ModuleRef(restore_module);
            state.memory = state.store.get_memory(state.module).clone();
            state.table = state.store.get_table(state.module).clone();
        }

        if return_pc == JumpTarget::SENTINEL {
            state.sp = parameter_start;
            state.pc = JumpTarget::SENTINEL;
            state.jumped = true;
            let return_value = match return_size {
                0 => RawValue::from_64(0),
                1 => RawValue::from_32(state.stack.read_u32(state.sp)),
                2 => RawValue::from_64(state.stack.read_u64(state.sp)),
                // TODO(tumbar) We need to verify that the entrypoint function does not return anything unexpected
                _ => unreachable!(),
            };

            state.result = Some(return_value);
            Err(InterpreterBreak::Finished)
        } else {
            state.sp = parameter_start + return_size;
            state.pc = return_pc + 2; // +2 to skip over the call or call_indirect
            state.jumped = true;
            Ok(())
        }
    }

    fn call(&self, x: u16, state: &mut Self::State) -> Result<(), Self::Error> {
        state.call_impl(0, x)
    }

    fn call_host(
        &self,
        module: HostModuleRef,
        x: u16,
        state: &mut Self::State,
    ) -> Result<(), Self::Error> {
        let m = &state.store.host_modules()[module.0 as usize];
        let f = &m.functions[x as usize];

        let mut sv: StaticVec<Value, 9> = StaticVec::new();

        state.sp -= f.param_size();
        let mut offset = 0;
        for p_ty in f.params().iter() {
            match p_ty {
                ValType::I32 => {
                    sv.push(Value::I32(state.stack.read_u32(state.sp + offset) as i32))
                        .unwrap();
                    offset += 1;
                }
                ValType::I64 => {
                    sv.push(Value::I64(state.stack.read_u64(state.sp + offset) as i64))
                        .unwrap();
                    offset += 2;
                }
                ValType::F32 => {
                    sv.push(Value::F32(state.stack.read_f32(state.sp + offset)))
                        .unwrap();
                    offset += 1;
                }
                ValType::F64 => {
                    sv.push(Value::F64(state.stack.read_f64(state.sp + offset)))
                        .unwrap();
                    offset += 2;
                }
            }
        }

        match f.call(state, &sv) {
            ControlFlow::Continue(v) => {
                match v {
                    None => {}
                    Some(Value::I32(i)) => {
                        state.stack.write_u32(state.sp, i as u32);
                        state.sp += 1;
                    }
                    Some(Value::I64(i)) => {
                        state.stack.write_u64(state.sp, i as u64);
                        state.sp += 2;
                    }
                    Some(Value::F32(f)) => {
                        state.stack.write_f32(state.sp, f);
                        state.sp += 1;
                    }
                    Some(Value::F64(f)) => {
                        state.stack.write_f64(state.sp, f);
                        state.sp += 2;
                    }
                }

                Ok(())
            }
            ControlFlow::Break(b) => Err(b.into()),
        }
    }

    fn call_extern(
        &self,
        module_ref: ModuleRef,
        x: u16,
        state: &mut Self::State,
    ) -> Result<(), Self::Error> {
        state.call_impl_enter_module(WasmRef {
            module: module_ref,
            index: x,
        })
    }

    fn call_indirect(&self, x: TypeIdx, state: &mut Self::State) -> Result<(), Self::Error> {
        // Pop the table pointer off the stack
        state.sp -= 1;
        let i = state.stack.read_u32(state.sp) as usize;
        let m = &state.store.modules()[state.module.0 as usize];
        let f_expected = &m.types[x.0 as usize];

        // Look up the internal or host function
        match state.table.get(i) {
            None => Err(InterpreterBreak::Trap(TrapReason::InvalidTableIndex)),
            Some(TableElement::Func { module, index }) => {
                let m = &state.store.modules()[module.0 as usize];
                let f = &m.functions[*index as usize];

                // Validate the function is the proper type
                // This asserts that it's safe to call the function with the current stack
                let f_actual = &m.types[f.ty.0 as usize];

                if f_actual.params != f_expected.params || f_actual.returns != f_expected.returns {
                    return Err(InterpreterBreak::Trap(TrapReason::InvalidTableFunctionType));
                }

                // Call the function
                state.call_impl_enter_module(WasmRef {
                    module: *module,
                    index: *index,
                })
            }
            Some(TableElement::Host { module, index }) => {
                // Make sure the type matches our expectations (runtime validation)
                let m = &state.store.host_modules()[module.0 as usize];
                let f = &m.functions[*index as usize];
                if f.params() != f_expected.params[..] || f.returns() != f_expected.returns[..] {
                    return Err(InterpreterBreak::Trap(TrapReason::InvalidTableFunctionType));
                }

                self.call_host(*module, *index, state)
            }
            Some(TableElement::Uninitialized) => Err(InterpreterBreak::Trap(
                TrapReason::UninitializedTableElement,
            )),
        }
    }

    fn local_get(&self, l: LocalVariable, state: &mut Self::State) -> Result<(), Self::Error> {
        let local_addr = l.addr(state.fp);
        match l.ty {
            ValType::I32 | ValType::F32 => {
                // Read/write a single word
                let val = state.stack.read_u32(local_addr);
                state.stack.write_u32(state.sp, val);
                state.sp += 1;
            }
            ValType::I64 | ValType::F64 => {
                // Read/write two words
                let val = state.stack.read_u64(local_addr);
                state.stack.write_u64(state.sp, val);
                state.sp += 2;
            }
        }

        Ok(())
    }

    fn local_set(&self, l: LocalVariable, state: &mut Self::State) -> Result<(), Self::Error> {
        let local_addr = l.addr(state.fp);
        match l.ty {
            ValType::I32 | ValType::F32 => {
                // Read/write a single word
                state.sp -= 1;
                let val = state.stack.read_u32(state.sp);
                state.stack.write_u32(local_addr, val);
            }
            ValType::I64 | ValType::F64 => {
                // Read/write two words
                state.sp -= 2;
                let val = state.stack.read_u64(state.sp);
                state.stack.write_u64(local_addr, val);
            }
        }

        Ok(())
    }

    fn local_tee(&self, l: LocalVariable, state: &mut Self::State) -> Result<(), Self::Error> {
        let local_addr = l.addr(state.fp);
        match l.ty {
            ValType::I32 | ValType::F32 => {
                // Read/write a single word
                let val = state.stack.read_u32(state.sp - 1);
                state.stack.write_u32(local_addr, val);
            }
            ValType::I64 | ValType::F64 => {
                // Read/write two words
                let val = state.stack.read_u64(state.sp - 2);
                state.stack.write_u64(local_addr, val);
            }
        }

        Ok(())
    }

    fn global_get(&self, idx: u16, state: &mut Self::State) -> Result<(), Self::Error> {
        let m = &state.store.modules()[state.module.0 as usize];
        let g = &m.globals[idx as usize];

        match g.type_.ty {
            ValType::I32 | ValType::F32 => {
                let val = g.value.read_32();
                state.stack.write_u32(state.sp, val);
                state.sp += 1;
            }
            ValType::I64 | ValType::F64 => {
                let val = g.value.read_64();
                state.stack.write_u64(state.sp, val);
                state.sp += 2;
            }
        }

        Ok(())
    }

    fn global_get_host(
        &self,
        module: HostModuleRef,
        index: u16,
        state: &mut Self::State,
    ) -> Result<(), Self::Error> {
        let m = &state.store.host_modules()[module.0 as usize];
        match m.globals[index as usize]
            .value
            .read()
            .or(Err(InterpreterBreak::Trap(TrapReason::GlobalGetFailed)))?
        {
            Value::I32(i) => {
                state.stack.write_u32(state.sp, i as u32);
                state.sp += 1;
            }
            Value::I64(i) => {
                state.stack.write_u64(state.sp, i as u64);
                state.sp += 2;
            }
            Value::F32(f) => {
                state.stack.write_f32(state.sp, f);
                state.sp += 1;
            }
            Value::F64(f) => {
                state.stack.write_f64(state.sp, f);
                state.sp += 2;
            }
        };

        Ok(())
    }

    fn global_set(&self, idx: u16, state: &mut Self::State) -> Result<(), Self::Error> {
        let m = &mut state.store.modules_mut()[state.module.0 as usize];
        let g = &mut m.globals[idx as usize];
        match g.type_.ty {
            ValType::I32 | ValType::F32 => {
                state.sp -= 1;
                let val = state.stack.read_u32(state.sp);
                g.value.write_32(val);
            }
            ValType::I64 | ValType::F64 => {
                state.sp -= 2;
                let val = state.stack.read_u64(state.sp);
                g.value.write_64(val);
            }
        }

        Ok(())
    }

    fn global_set_host(
        &self,
        module: HostModuleRef,
        index: u16,
        state: &mut Self::State,
    ) -> Result<(), Self::Error> {
        let m = &state.store.host_modules()[module.0 as usize];
        let g = &m.globals[index as usize];
        match g.value.ty() {
            ValType::I32 => {
                state.sp -= 1;
                let val = state.stack.read_u32(state.sp) as i32;
                g.value.write(Value::I32(val))
            }
            ValType::I64 => {
                state.sp -= 2;
                let val = state.stack.read_u64(state.sp) as i64;
                g.value.write(Value::I64(val))
            }
            ValType::F32 => {
                state.sp -= 1;
                let f = state.stack.read_f32(state.sp);
                g.value.write(Value::F32(f))
            }
            ValType::F64 => {
                state.sp -= 2;
                let f = state.stack.read_f64(state.sp);
                g.value.write(Value::F64(f))
            }
        }
        .or(Err(InterpreterBreak::Trap(TrapReason::GlobalSetFailed)))
    }

    fn global_get_extern(
        &self,
        module: ModuleRef,
        index: u16,
        state: &mut Self::State,
    ) -> Result<(), Self::Error> {
        let current = state.module;
        state.module = module;
        let res = self.global_get(index, state);
        state.module = current;
        res
    }

    fn global_set_extern(
        &self,
        module: ModuleRef,
        index: u16,
        state: &mut Self::State,
    ) -> Result<(), Self::Error> {
        let current = state.module;
        state.module = module;
        let res = self.global_set(index, state);
        state.module = current;
        res
    }
}

#[cfg(test)]
#[path = "interpreter_tests.rs"]
mod interpreter_tests;
