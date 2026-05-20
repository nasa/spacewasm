use crate::exec::ir_reader::{IrReader, IrReaderError};
use crate::exec::m;
use crate::*;
use ::core::ops::ControlFlow;

pub struct InterpreterState {
    pub pc: JumpTarget,
    pub fp: u32,
    pub sp: usize,
    pub stack: Stack,
    pub ram: Memory,
}

impl LocalVariable {
    fn addr(&self, fp: u32) -> usize {
        (fp as i32 + self.frame_offset as i32) as usize
    }
}

impl InterpreterState {
    pub fn new(stack_size: usize, ram: Memory) -> Self {
        InterpreterState {
            pc: JumpTarget::SENTINEL,
            sp: 0x0,
            fp: 0x0,
            stack: Stack::new(stack_size),
            ram,
        }
    }

    /// Before executing code from a module, the global and data must be initialized into
    /// the state. This function will do the following for module [m]
    ///
    /// Assert the stack pointer and frame pointer are zero. This function should be called
    /// before any function invocation
    ///
    /// For each global, g, in [m]:
    ///    With init constant, c, of g:
    ///    Write c to the stack
    ///
    /// For each data, d, in [m]
    ///     For d with init data i and offset o:
    ///     Write i to the RAM at offset o.
    pub fn initialize(&mut self, m: &Module) -> Result<(), MemoryOutOfBounds> {
        // Globals must be initialized before any invocation
        assert_eq!(self.sp, 0);
        assert_eq!(self.fp, 0);

        for global in &m.globals {
            match global.type_.ty {
                ValType::I32 => {
                    let i = global.init as u32;
                    self.stack.write_u32(self.sp, i);
                    self.sp += 1;
                }
                ValType::I64 => {
                    let i = global.init;
                    self.stack.write_u64(self.sp, i);
                    self.sp += 2;
                }
                ValType::F32 => {
                    let z = f32::from_bits(global.init as u32);
                    self.stack.write_f32(self.sp, z);
                    self.sp += 1;
                }
                ValType::F64 => {
                    let z = f64::from_bits(global.init);
                    self.stack.write_f64(self.sp, z);
                    self.sp += 2;
                }
            }
        }

        for data in &m.data {
            self.ram.store(data.offset as usize, &data.init)?;
        }

        self.fp = self.sp as u32;
        Ok(())
    }
}

pub struct Interpreter<'store> {
    pub store: &'store Store,
    pub module: &'store Module,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterpreterResult {
    /// No more fuel (ran to instruction bound)
    OutOfFuel,
    /// An instruction requested a pause or failed
    Instruction(InterpreterBreak),
    /// Failed to read an instruction from memory
    ReaderError(IrReaderError),
}

impl<'store> Interpreter<'store> {
    pub fn new(store: &'store Store, module: &'store Module) -> Self {
        Interpreter { store, module }
    }

    fn call_impl(&self, f: &Func, state: &mut InterpreterState) -> Result<(), InterpreterBreak> {
        // Make sure we have enough stack space for the function call
        let required_stack_space = f.stack_usage as usize + 2 + f.local_size as usize;
        if state.stack.len() < state.sp + required_stack_space {
            return Err(InterpreterBreak::Trap(TrapReason::StackOverflow));
        }

        // The arguments are already at the top of the stack
        // We need to push the frame pointer and the return instruction pointer to the stack
        // We also encode the parameter size into the stack frame so that the return can unwind the stack
        let frame_length = (state.sp - state.fp as usize) as u32;
        assert!(frame_length <= 0xFFFFF);

        let frame_length = frame_length << 16;

        state
            .stack
            .write_u32(state.sp, frame_length | (f.parameter_size as u32));
        state.stack.write_u32(state.sp + 1, state.pc.0);
        state.fp = state.sp as u32;

        // Zero out the local variables
        for i in 0..(f.local_size as usize) {
            state.stack.write_u32(state.sp + 2 + i, 0);
        }

        // Allocate space for frame and the local variables
        state.sp += 2 + f.local_size as usize;

        // Jump to the function's execution point
        state.pc = f.expr.0;

        Ok(())
    }

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
    ///                      and this label target.
    /// * `addr`: Resolved label target to jump to
    /// * `state`: Current interpreter state
    ///
    /// returns: Result<(), InterpreterBreak>
    fn br_impl(
        &self,
        label_pc_offset: JumpOffset,
        addr: LabelTarget,
        state: &mut InterpreterState,
    ) -> Result<(), InterpreterBreak> {
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
        Ok(())
    }

    /// Invoke a function with some parameters
    /// Warning! If this is being used as an interrupt rather than an entry point,
    /// make sure that the function does not return any values as that will cause stack pollution!
    pub fn invoke(
        &self,
        state: &mut InterpreterState,
        f: &Func,
        params: &[Value],
    ) -> Result<(), InterpreterBreak> {
        for p in params {
            // TODO(tumbar) Validate input parameters
            match p {
                Value::I32(i) => {
                    state.stack.write_u32(state.sp, *i as u32);
                    state.sp += 1;
                }
                Value::I64(i) => {
                    state.stack.write_u64(state.sp, *i as u64);
                    state.sp += 2;
                }
                Value::F32(z) => {
                    state.stack.write_f32(state.sp, *z);
                    state.sp += 1;
                }
                Value::F64(z) => {
                    state.stack.write_f64(state.sp, *z);
                    state.sp += 2;
                }
            }
        }

        self.call_impl(f, state)
    }
}

/// This is a meta-trait that provides an auto implementation of run() for all IrVisitors
/// of a certain shape.
///
/// For all types that implement [IrVisitor<State = InterpreterState, Error = InstructionError>],
/// this trait will be implemented to execute instructions given the state and store.
pub trait InterpreterRunner {
    fn run(
        &self,
        code: &[Box<TextPage>],
        state: &mut InterpreterState,
        n_instructions: usize,
    ) -> InterpreterResult;
}

impl<T: IrVisitor<State = InterpreterState, Error = InterpreterBreak>> InterpreterRunner for T {
    fn run(
        &self,
        code: &[Box<TextPage>],
        state: &mut InterpreterState,
        n_instructions: usize,
    ) -> InterpreterResult {
        let reader = IrReader::new(code);

        // Run up to n instructions
        for _ in 0..n_instructions {
            let old_pc = state.pc;
            let mut pc = state.pc;

            let i_res = reader.visit_instruction(state, &mut pc, self);
            if old_pc != state.pc {
                // We jumped, leave the PC
            } else {
                // Increment the program counter
                state.pc = pc;
            }

            match i_res {
                Ok(_) => {}
                Err(e) => return InterpreterResult::Instruction(e),
            }
        }

        InterpreterResult::OutOfFuel
    }
}

/// A raw value
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawValue(pub u64);

impl RawValue {
    pub fn to_value(self, ty: ValType) -> Value {
        match ty {
            ValType::I32 => Value::I32((self.0 as u32) as i32),
            ValType::I64 => Value::I64(self.0 as i64),
            ValType::F32 => Value::F32(f32::from_bits(self.0 as u32)),
            ValType::F64 => Value::F64(f64::from_bits(self.0)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrapReason {
    /// Triggered by unreachable instruction
    Unreachable,
    /// A host function has noted an unrecoverable failure
    Host,
    ///
    DivideByZero,
    /// An indirect call tried to map to a table function out of range
    InvalidTableIndex,
    /// The function type in an indirect call does not match the function pointer's type
    InvalidTableFunctionType,
    /// An imported global could not be read
    GlobalGetFailed,
    /// An imported global could not be set
    GlobalSetFailed,
    /// A memory operation is out of bounds
    MemoryOutOfBounds,
    /// Ran out of stack space
    StackOverflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterpreterBreak {
    /// The program has completed
    Finished(RawValue),
    /// The program has been aborted
    Trap(TrapReason),
    /// An instruction or host function has requested the interpreter to pause
    Pause,
}

impl From<MemoryOutOfBounds> for InterpreterBreak {
    fn from(_: MemoryOutOfBounds) -> Self {
        InterpreterBreak::Trap(TrapReason::MemoryOutOfBounds)
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
    ($name:ident, unreachable) => {
        fn $name(&self, state: &mut Self::State) -> Result<(), Self::Error> {
            // This instruction does not exist in the IR
            let _ = state;
            unreachable!()
        }
    };
}

impl<'module> BaseVisitor for Interpreter<'module> {
    type Error = InterpreterBreak;
    type State = InterpreterState;

    fn unreachable(&self, _: &mut Self::State) -> Result<(), Self::Error> {
        Err(InterpreterBreak::Trap(TrapReason::Unreachable))
    }

    fn nop(&self, _: &mut Self::State) -> Result<(), Self::Error> {
        Ok(())
    }

    fn i32_load(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        let addr = state.stack.read_u32(state.sp - 1) as usize;
        let val = state.ram.load_u32(addr + m.offset as usize)?;
        state.stack.write_u32(state.sp - 1, val);
        Ok(())
    }

    fn i64_load(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        let addr = state.stack.read_u32(state.sp - 1) as usize;
        let val = state.ram.load_u64(addr + m.offset as usize)?;
        state.stack.write_u64(state.sp - 1, val);
        state.sp += 1;
        Ok(())
    }

    fn f32_load(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        let addr = state.stack.read_u32(state.sp - 1) as usize;
        let val = state.ram.load_u32(addr + m.offset as usize)?;
        state.stack.write_u32(state.sp - 1, val);
        Ok(())
    }

    fn f64_load(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        let addr = state.stack.read_u32(state.sp - 1) as usize;
        let val = state.ram.load_u64(addr + m.offset as usize)?;
        state.stack.write_u64(state.sp - 1, val);
        state.sp += 1;
        Ok(())
    }

    fn i32_load8_s(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        let addr = state.stack.read_u32(state.sp - 1) as usize;
        let val = state.ram.load_u8(addr + m.offset as usize)? as i8 as i32;
        state.stack.write_u32(state.sp - 1, val as u32);
        Ok(())
    }

    fn i32_load8_u(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        let addr = state.stack.read_u32(state.sp - 1) as usize;
        let val = state.ram.load_u8(addr + m.offset as usize)? as u32;
        state.stack.write_u32(state.sp - 1, val);
        Ok(())
    }

    fn i32_load16_s(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        let addr = state.stack.read_u32(state.sp - 1) as usize;
        let val = state.ram.load_u16(addr + m.offset as usize)? as i16 as i32;
        state.stack.write_u32(state.sp - 1, val as u32);
        Ok(())
    }

    fn i32_load16_u(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        let addr = state.stack.read_u32(state.sp - 1) as usize;
        let val = state.ram.load_u16(addr + m.offset as usize)? as u32;
        state.stack.write_u32(state.sp - 1, val);
        Ok(())
    }

    fn i64_load8_s(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        let addr = state.stack.read_u32(state.sp - 1) as usize;
        let val = state.ram.load_u8(addr + m.offset as usize)? as i8 as i64 as u64;
        state.stack.write_u64(state.sp - 1, val);
        state.sp += 1;
        Ok(())
    }

    fn i64_load8_u(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        let addr = state.stack.read_u32(state.sp - 1) as usize;
        let val = state.ram.load_u8(addr + m.offset as usize)? as u64;
        state.stack.write_u64(state.sp - 1, val);
        state.sp += 1;
        Ok(())
    }

    fn i64_load16_s(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        let addr = state.stack.read_u32(state.sp - 1) as usize;
        let val = state.ram.load_u16(addr + m.offset as usize)? as i16 as i64 as u64;
        state.stack.write_u64(state.sp - 1, val);
        state.sp += 1;
        Ok(())
    }

    fn i64_load16_u(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        let addr = state.stack.read_u32(state.sp - 1) as usize;
        let val = state.ram.load_u16(addr + m.offset as usize)? as u64;
        state.stack.write_u64(state.sp - 1, val);
        state.sp += 1;
        Ok(())
    }

    fn i64_load32_s(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        let addr = state.stack.read_u32(state.sp - 1) as usize;
        let val = state.ram.load_u32(addr + m.offset as usize)? as i32 as i64 as u64;
        state.stack.write_u64(state.sp - 1, val);
        state.sp += 1;
        Ok(())
    }

    fn i64_load32_u(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        let addr = state.stack.read_u32(state.sp - 1) as usize;
        let val = state.ram.load_u32(addr + m.offset as usize)? as u64;
        state.stack.write_u64(state.sp - 1, val);
        state.sp += 1;
        Ok(())
    }

    fn i32_store(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 2;
        let val = state.stack.read_u32(state.sp + 1);
        let addr = state.stack.read_u32(state.sp) as usize;
        state.ram.store_u32(addr + m.offset as usize, val)?;
        Ok(())
    }

    fn i64_store(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 3;
        let val = state.stack.read_u64(state.sp + 1);
        let addr = state.stack.read_u32(state.sp) as usize;
        state.ram.store_u64(addr + m.offset as usize, val)?;
        Ok(())
    }

    fn f32_store(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 2;
        let val = state.stack.read_u32(state.sp + 1);
        let addr = state.stack.read_u32(state.sp) as usize;
        state.ram.store_u32(addr + m.offset as usize, val)?;
        Ok(())
    }

    fn f64_store(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 3;
        let val = state.stack.read_u64(state.sp + 1);
        let addr = state.stack.read_u32(state.sp) as usize;
        state.ram.store_u64(addr + m.offset as usize, val)?;
        Ok(())
    }

    fn i32_store8(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 2;
        let val = state.stack.read_u32(state.sp + 1) as u8;
        let addr = state.stack.read_u32(state.sp) as usize;
        state.ram.store_u8(addr + m.offset as usize, val)?;
        Ok(())
    }

    fn i32_store16(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 2;
        let val = state.stack.read_u32(state.sp + 1) as u16;
        let addr = state.stack.read_u32(state.sp) as usize;
        state.ram.store_u16(addr + m.offset as usize, val)?;
        Ok(())
    }

    fn i64_store8(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 3;
        let val = state.stack.read_u64(state.sp + 1) as u8;
        let addr = state.stack.read_u32(state.sp) as usize;
        state.ram.store_u8(addr + m.offset as usize, val)?;
        Ok(())
    }

    fn i64_store16(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 3;
        let val = state.stack.read_u64(state.sp + 1) as u16;
        let addr = state.stack.read_u32(state.sp) as usize;
        state.ram.store_u16(addr + m.offset as usize, val)?;
        Ok(())
    }

    fn i64_store32(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 3;
        let val = state.stack.read_u64(state.sp + 1) as u32;
        let addr = state.stack.read_u32(state.sp) as usize;
        state.ram.store_u32(addr + m.offset as usize, val)?;
        Ok(())
    }

    fn memory_size(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        state.stack.write_u32(state.sp, state.ram.size_pages());
        state.sp += 1;
        Ok(())
    }

    instruction!(memory_grow, unreachable);

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

    instruction!(i32_eqz, i32 -> i32, i, if i == 0 { 1 } else { 0 });
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
    instruction!(i64_eqz, i64 -> i64, i, if i == 0 { 1 } else { 0 });
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
            return Err(InterpreterBreak::Trap(TrapReason::DivideByZero))
        } else {
            a.wrapping_div(b)
        }
    });
    instruction!(i32_div_u, i32, i32 -> i32, a, b, {
        if b == 0 {
            return Err(InterpreterBreak::Trap(TrapReason::DivideByZero))
        } else {
            (a as u32).wrapping_div(b as u32)
        }
    } as i32);
    instruction!(i32_rem_s, i32, i32 -> i32, a, b, a.wrapping_rem(b));
    instruction!(i32_rem_u, i32, i32 -> i32, a, b, (a as u32).wrapping_rem(b as u32) as i32);
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
            return Err(InterpreterBreak::Trap(TrapReason::DivideByZero))
        } else {
            a.wrapping_div(b)
        }
    });
    instruction!(i64_div_u, i64, i64 -> i64, a, b, {
        if b == 0 {
            return Err(InterpreterBreak::Trap(TrapReason::DivideByZero))
        } else {
            (a as u64).wrapping_div(b as u64) as i64
        }
    });
    instruction!(i64_rem_s, i64, i64 -> i64, a, b, a.wrapping_rem(b));
    instruction!(i64_rem_u, i64, i64 -> i64, a, b, (a as u64).wrapping_rem(b as u64) as i64);
    instruction!(i64_and, i64, i64 -> i64, a, b, a & b);
    instruction!(i64_or, i64, i64 -> i64, a, b, a | b);
    instruction!(i64_xor, i64, i64 -> i64, a, b, a ^ b);
    instruction!(i64_shl, i64, i64 -> i64, a, b, a.wrapping_shl(b as u32));
    instruction!(i64_shr_s, i64, i64 -> i64, a, b, a.wrapping_shr(b as u32));
    instruction!(i64_shr_u, i64, i64 -> i64, a, b, (a as u64).wrapping_shr(b as u32) as i64);
    instruction!(i64_rotl, i64, i64 -> i64, a, b, a.rotate_left(b as u32));
    instruction!(i64_rotr, i64, i64 -> i64, a, b, a.rotate_right(b as u32));
    instruction!(f32_abs, f32 -> f32, f, if f < 0.0 { -f } else { f });
    instruction!(f32_neg, f32 -> f32, f, -f);
    instruction!(f32_ceil, f32 -> f32, f, m::ceilf(f));
    instruction!(f32_floor, f32 -> f32, f, m::floorf(f));
    instruction!(f32_trunc, f32 -> f32, f, m::truncf(f));
    instruction!(f32_nearest, f32 -> f32, f, m::roundf(f));
    instruction!(f32_sqrt, f32 -> f32, f, m::sqrtf(f));
    instruction!(f32_add, f32, f32 -> f32, a, b, a + b);
    instruction!(f32_sub, f32, f32 -> f32, a, b, a - b);
    instruction!(f32_mul, f32, f32 -> f32, a, b, a * b);
    instruction!(f32_div, f32, f32 -> f32, a, b, a / b);
    instruction!(f32_min, f32, f32 -> f32, a, b, m::fminf(a, b));
    instruction!(f32_max, f32, f32 -> f32, a, b, m::fmaxf(a, b));
    instruction!(f32_copysign, f32, f32 -> f32, a, b, m::copysignf(a, b));
    instruction!(f64_abs, f64 -> f64, f, if f < 0.0 { -f } else { f });
    instruction!(f64_neg, f64 -> f64, f, -f);
    instruction!(f64_ceil, f64 -> f64, f, m::ceil(f));
    instruction!(f64_floor, f64 -> f64, f, m::floor(f));
    instruction!(f64_trunc, f64 -> f64, f, m::trunc(f));
    instruction!(f64_nearest, f64 -> f64, f, m::round(f));
    instruction!(f64_sqrt, f64 -> f64, f, m::sqrt(f));
    instruction!(f64_add, f64, f64 -> f64, a, b, a + b);
    instruction!(f64_sub, f64, f64 -> f64, a, b, a - b);
    instruction!(f64_mul, f64, f64 -> f64, a, b, a * b);
    instruction!(f64_div, f64, f64 -> f64, a, b, a / b);
    instruction!(f64_min, f64, f64 -> f64, a, b, m::fmin(a, b));
    instruction!(f64_max, f64, f64 -> f64, a, b, m::fmax(a, b));
    instruction!(f64_copysign, f64, f64 -> f64, a, b, m::copysign(a, b));

    fn i32_wrap_i64(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        // i64 low word is at [sp-2], high word at [sp-1]
        // After decrement, low word is at [sp-1] (where we want the i32)
        state.sp -= 1;
        Ok(())
    }

    fn i32_trunc_f32_s(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let f = state.stack.read_f32(state.sp - 1);
        state.stack.write_u32(state.sp - 1, f as i32 as u32);
        Ok(())
    }

    fn i32_trunc_f32_u(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let f = state.stack.read_f32(state.sp - 1);
        state.stack.write_u32(state.sp - 1, f as u32);
        Ok(())
    }

    fn i32_trunc_f64_s(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 1;
        let f = state.stack.read_f64(state.sp - 1);
        state.stack.write_u32(state.sp - 1, f as i32 as u32);
        Ok(())
    }

    fn i32_trunc_f64_u(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 1;
        let f = state.stack.read_f64(state.sp - 1);
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
        let i = f as i64 as u64;
        state.stack.write_u64(state.sp - 1, i);
        state.sp += 1;
        Ok(())
    }

    fn i64_trunc_f32_u(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let f = state.stack.read_f32(state.sp - 1);
        let u = f as u64;
        state.stack.write_u64(state.sp - 1, u);
        state.sp += 1;
        Ok(())
    }

    fn i64_trunc_f64_s(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let f = state.stack.read_f64(state.sp - 2);
        let i = f as i64 as u64;
        state.stack.write_u64(state.sp - 2, i);
        Ok(())
    }

    fn i64_trunc_f64_u(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let f = state.stack.read_f64(state.sp - 2);
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

    instruction!(i32_reinterpret_f32, unreachable);
    instruction!(i64_reinterpret_f64, unreachable);
    instruction!(f32_reinterpret_i32, unreachable);
    instruction!(f64_reinterpret_i64, unreachable);
}

impl<'module> IrVisitor for Interpreter<'module> {
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
                    state
                        .stack
                        .write_u32(state.sp - 2, val2);
                    state.sp -= 1;
                }
            }
            ValType::I64 | ValType::F64=> {
                if c != 0 {
                    // Use val1 which is already in the right spot
                    state.sp -= 2;
                } else {
                    // Move val2 to val1's spot
                    let val2 = state.stack.read_u64(state.sp - 2);
                    state
                        .stack
                        .write_u64(state.sp - 4, val2);
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

        if v < n {
            // A standard case, compute the offset from the current PC
            // Instruction opcode + default case (2 words) + previous cases (each 2 words)
            self.br_impl(JumpOffset::offset(1 + 2 + (2 * v as i32)), label, state)?;
        } else {
            // The default case, constant offset
            self.br_impl(JumpOffset::offset(1), label, state)?;
        }

        Ok(())
    }

    fn return_(&self, return_size: u8, state: &mut Self::State) -> Result<(), Self::Error> {
        let return_size = return_size as usize;

        let fp = state.fp as usize;
        let return_pc = JumpTarget(state.stack.read_u32(state.fp as usize + 1));

        // The frame pointer on the stack actually encodes ((sp - fp) << 16) | prm_size
        let frame_length_and_prm_size = state.stack.read_u32(state.fp as usize);
        let frame_length = (frame_length_and_prm_size >> 16) as u16;
        let parameter_size = (frame_length_and_prm_size as u16) as usize;
        let return_fp = (fp as u32) - (frame_length as u32);

        let parameter_start = fp - parameter_size;

        // Copy the return value over the parameters/frame information
        for i in 0..return_size {
            let val = state.stack.read_u32(state.sp - return_size + i);
            state.stack.write_u32(parameter_start + i, val);
        }

        state.fp = return_fp;

        if return_pc == JumpTarget::SENTINEL {
            state.sp = parameter_start;
            state.pc = JumpTarget::SENTINEL;
            let return_value = match return_size {
                0 => RawValue(0),
                1 => RawValue(state.stack.read_u32(state.sp) as u64),
                2 => RawValue(state.stack.read_u64(state.sp)),
                // TODO(tumbar) We need to verify that the entrypoint function does not return anything unexpected
                _ => unreachable!(),
            };

            Err(InterpreterBreak::Finished(return_value))
        } else {
            state.sp = parameter_start + return_size;
            state.pc = return_pc + 2; // +2 to skip over the call or call_indirect
            Ok(())
        }
    }

    fn call(&self, x: u16, state: &mut Self::State) -> Result<(), Self::Error> {
        let f = &self.module.functions[x as usize];
        self.call_impl(f, state)
    }

    fn call_host(
        &self,
        module: HostModuleRef,
        x: u16,
        state: &mut Self::State,
    ) -> Result<(), Self::Error> {
        let m = &self.store.host_modules[module.0 as usize];
        let f = &m.functions[x as usize];

        let mut sv: StaticVec<Value, 8> = StaticVec::new();

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

    fn call_indirect(&self, x: TypeIdx, state: &mut Self::State) -> Result<(), Self::Error> {
        // Pop the table pointer off the stack
        state.sp -= 1;
        let i = state.stack.read_u32(state.sp) as usize;
        let f_expected = &self.module.types[x.0 as usize];

        if i >= self.module.table.len() {
            return Err(InterpreterBreak::Trap(TrapReason::InvalidTableIndex));
        }

        // Look up the internal or host function
        let f_ref = self.module.table[i];
        match f_ref {
            FuncRef::Func(fi) => {
                let f = &self.module.functions[fi as usize];

                // Validate the function is the proper type
                // This asserts that it's safe to call the function with the current stack
                let f_actual = &self.module.types[f.ty.0 as usize];

                if f_actual.params != f_expected.params || f_actual.returns != f_expected.returns {
                    return Err(InterpreterBreak::Trap(TrapReason::InvalidTableFunctionType));
                }

                // Call the function
                self.call_impl(f, state)
            }
            FuncRef::HostFunc { module, index } => {
                // Make sure the type matches our expectations (runtime validation)
                let m = &self.store.host_modules[module.0 as usize];
                let f = &m.functions[index as usize];
                if f.params() != f_expected.params[..] || f.returns() != f_expected.returns[..] {
                    return Err(InterpreterBreak::Trap(TrapReason::InvalidTableFunctionType));
                }

                self.call_host(module, i as u16, state)
            }
            // TODO(tumbar) This is currently disallowed at compile time
            FuncRef::ExternFunc { .. } => unreachable!(),
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
        let g = &self.module.globals[idx as usize];

        match g.type_.ty {
            ValType::I32 | ValType::F32 => {
                let val = state.stack.read_u32(g.addr as usize);
                state.stack.write_u32(state.sp, val);
                state.sp += 1;
            }
            ValType::I64 | ValType::F64 => {
                let val = state.stack.read_u64(g.addr as usize);
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
        let m = &self.store.host_modules[module.0 as usize];
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
        let g = &self.module.globals[idx as usize];
        match g.type_.ty {
            ValType::I32 | ValType::F32 => {
                state.sp -= 1;
                let val = state.stack.read_u32(state.sp);
                state.stack.write_u32(g.addr as usize, val);
            }
            ValType::I64 | ValType::F64 => {
                state.sp -= 2;
                let val = state.stack.read_u64(state.sp);
                state.stack.write_u64(g.addr as usize, val);
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
        let m = &self.store.host_modules[module.0 as usize];
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
}

#[cfg(test)]
#[path = "interpreter_tests.rs"]
mod interpreter_tests;
