use crate::*;
use core::ops::{AddAssign, ControlFlow};

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

    pub fn initialize(
        &mut self,
        globals: &[Global],
        data: &[Data],
    ) -> Result<(), MemoryOutOfBounds> {
        // Globals must be initialized before any invocation
        assert_eq!(self.sp, 0);
        assert_eq!(self.fp, 0);

        for global in globals {
            match global.init {
                Value::I32(i) => {
                    self.stack.write_u32(self.sp, i as u32);
                    self.sp += 1;
                }
                Value::I64(i) => {
                    self.stack.write_u64(self.sp, i as u64);
                    self.sp += 2;
                }
                Value::F32(z) => {
                    self.stack.write_f32(self.sp, z);
                    self.sp += 1;
                }
                Value::F64(z) => {
                    self.stack.write_f64(self.sp, z);
                    self.sp += 2;
                }
            }
        }

        for data in data {
            self.ram.store(data.offset, &data.init)?;
        }

        self.fp = self.sp as u32;
        Ok(())
    }
}

pub struct Interpreter<'module> {
    pub functions: &'module [Func],
    pub global_imports: &'module [GlobalImport<'module>],
    pub function_imports: &'module [HostFunction<'module>],
    pub memory_imports: &'module [MemoryImport<'module>],
    pub table: &'module [FuncRef],
    pub types: &'module [FuncType],
}

impl AddAssign<u32> for JumpTarget {
    fn add_assign(&mut self, rhs: u32) {
        self.0.add_assign(rhs);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterpreterResult {
    /// Reached the end of the program
    Finished,
    /// No more fuel (ran to instruction bound)
    OutOfFuel,
    /// An instruction requested a pause or failed
    Instruction(InstructionError),
    /// Failed to read an instruction from memory
    ReaderError(IrReaderError),
}

impl<'module> Interpreter<'module> {
    pub fn new(
        functions: &'module [Func],
        global_imports: &'module [GlobalImport<'module>],
        function_imports: &'module [HostFunction<'module>],
        memory_imports: &'module [MemoryImport<'module>],
        table: &'module [FuncRef],
        types: &'module [FuncType],
    ) -> Self {
        Interpreter {
            functions,
            global_imports,
            function_imports,
            memory_imports,
            table,
            types,
        }
    }

    fn call_impl(&self, f: &Func, state: &mut InterpreterState) {
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
    }

    /// Invoke a function with some parameters
    /// Warning! If this is being used as an interrupt rather than an entry point,
    /// make sure that the function does not return any values as that will cause stack pollution!
    pub fn invoke(&self, state: &mut InterpreterState, f: &Func, params: &[Value]) {
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

        self.call_impl(f, state);
    }

    pub fn run(
        &self,
        code: &Code,
        state: &mut InterpreterState,
        n_instructions: usize,
    ) -> InterpreterResult {
        // Run up to n instructions
        for _ in 0..n_instructions {
            let old_pc = state.pc;
            let mut pc = state.pc;

            let i_res = code.visit_instruction(state, &mut pc, self);
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
pub enum InstructionError {
    /// The program has completed
    Finished(RawValue),
    /// The program has been aborted
    Trap,
    /// An instruction or host function has requested the interpreter to pause
    Pause,
    /// An indirect call tried to map to a table function out of range
    InvalidTableIndex,
    /// The function type in an indirect call does not match the function pointer's type
    InvalidTableFunctionType,
    /// A dynamic br_table lookup was out of bounds
    BrTableLookupFailed,
    /// An imported global could not be read
    GlobalGetFailed,
    /// An imported global could not be set
    GlobalSetFailed,
    /// A memory operation is out of bounds
    MemoryOutOfBounds,
}

impl From<MemoryOutOfBounds> for InstructionError {
    fn from(_: MemoryOutOfBounds) -> Self {
        InstructionError::MemoryOutOfBounds
    }
}

impl From<HostFunctionPause> for InstructionError {
    fn from(err: HostFunctionPause) -> Self {
        match err {
            HostFunctionPause::Trap => InstructionError::Trap,
            HostFunctionPause::Pause => InstructionError::Pause,
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
    type Error = InstructionError;
    type State = InterpreterState;

    fn unreachable(&self, _: &mut Self::State) -> Result<(), Self::Error> {
        Err(InstructionError::Trap)
    }

    fn nop(&self, _: &mut Self::State) -> Result<(), Self::Error> {
        Ok(())
    }

    fn drop(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 1;
        Ok(())
    }

    fn select(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 1;
        let c = state.stack.read_u32(state.sp);
        let val2 = state.stack.read_u32(state.sp - 1);
        let val1 = state.stack.read_u32(state.sp - 2);
        state.sp -= 2;
        state
            .stack
            .write_u32(state.sp, if c != 0 { val1 } else { val2 });
        state.sp += 1;
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
    instruction!(i32_div_s, i32, i32 -> i32, a, b, a.wrapping_div(b));
    instruction!(i32_div_u, i32, i32 -> i32, a, b, (a as u32).wrapping_div(b as u32) as i32);
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
    instruction!(i64_div_s, i64, i64 -> i64, a, b, a.wrapping_div(b));
    instruction!(i64_div_u, i64, i64 -> i64, a, b, (a as u64).wrapping_div(b as u64) as i64);
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
    instruction!(f32_ceil, f32 -> f32, f, libm::ceilf(f));
    instruction!(f32_floor, f32 -> f32, f, libm::floorf(f));
    instruction!(f32_trunc, f32 -> f32, f, libm::truncf(f));
    instruction!(f32_nearest, f32 -> f32, f, libm::roundf(f));
    instruction!(f32_sqrt, f32 -> f32, f, libm::sqrtf(f));
    instruction!(f32_add, f32, f32 -> f32, a, b, a + b);
    instruction!(f32_sub, f32, f32 -> f32, a, b, a - b);
    instruction!(f32_mul, f32, f32 -> f32, a, b, a * b);
    instruction!(f32_div, f32, f32 -> f32, a, b, a / b);
    instruction!(f32_min, f32, f32 -> f32, a, b, libm::fminf(a, b));
    instruction!(f32_max, f32, f32 -> f32, a, b, libm::fmaxf(a, b));
    instruction!(f32_copysign, f32, f32 -> f32, a, b, libm::copysignf(a, b));
    instruction!(f64_abs, f64 -> f64, f, if f < 0.0 { -f } else { f });
    instruction!(f64_neg, f64 -> f64, f, -f);
    instruction!(f64_ceil, f64 -> f64, f, libm::ceil(f));
    instruction!(f64_floor, f64 -> f64, f, libm::floor(f));
    instruction!(f64_trunc, f64 -> f64, f, libm::trunc(f));
    instruction!(f64_nearest, f64 -> f64, f, libm::round(f));
    instruction!(f64_sqrt, f64 -> f64, f, libm::sqrt(f));
    instruction!(f64_add, f64, f64 -> f64, a, b, a + b);
    instruction!(f64_sub, f64, f64 -> f64, a, b, a - b);
    instruction!(f64_mul, f64, f64 -> f64, a, b, a * b);
    instruction!(f64_div, f64, f64 -> f64, a, b, a / b);
    instruction!(f64_min, f64, f64 -> f64, a, b, libm::fmin(a, b));
    instruction!(f64_max, f64, f64 -> f64, a, b, libm::fmax(a, b));
    instruction!(f64_copysign, f64, f64 -> f64, a, b, libm::copysign(a, b));

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
    fn if_(&self, false_address: JumpTarget, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 1;
        let v = state.stack.read_u32(state.sp);
        if v == 0 {
            state.pc = false_address;
        }

        Ok(())
    }

    fn br(&self, addr: JumpTarget, state: &mut Self::State) -> Result<(), Self::Error> {
        state.pc = addr;
        Ok(())
    }

    fn br_if(&self, true_address: JumpTarget, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 1;
        let v = state.stack.read_u32(state.sp);
        if v != 0 {
            state.pc = true_address;
        }

        Ok(())
    }

    fn br_table(
        &self,
        cases: impl FnOnce(u16) -> Result<JumpTarget, ()>,
        state: &mut Self::State,
    ) -> Result<(), Self::Error> {
        state.sp -= 1;
        let v = state.stack.read_u32(state.sp);
        let addr = cases(v as u16).map_err(|_| InstructionError::BrTableLookupFailed)?;

        state.pc = addr;
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

            Err(InstructionError::Finished(return_value))
        } else {
            state.sp = parameter_start + return_size;
            state.pc = return_pc + 2; // +2 to skip over the call or call_indirect
            Ok(())
        }
    }

    fn call(&self, x: u16, state: &mut Self::State) -> Result<(), Self::Error> {
        // TODO(tumbar) Check stack usage
        let f = &self.functions[x as usize];
        self.call_impl(f, state);
        Ok(())
    }

    fn call_host(&self, x: u16, state: &mut Self::State) -> Result<(), Self::Error> {
        let f = &self.function_imports[x as usize];
        let mut sv: StaticVec<Value, 8> = StaticVec::new();

        state.sp -= f.param_size();
        let mut offset = 0;
        for p_ty in f.params() {
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
        let f_expected = &self.types[x.0 as usize];

        if i >= self.table.len() {
            return Err(InstructionError::InvalidTableIndex);
        }

        // Look up the internal or host function
        let f_ref = self.table[i];
        match f_ref {
            FuncRef::HostFunc(hfi) => {
                // Make sure the type matches our expectations (runtime validation)
                let f = &self.function_imports[hfi as usize];
                if f.params()[..] != f_expected.params[..]
                    || f.returns()[..] != f_expected.returns[..]
                {
                    return Err(InstructionError::InvalidTableFunctionType);
                }

                self.call_host(i as u16, state)
            }
            FuncRef::Func(fi) => {
                let f = &self.functions[fi as usize];

                // Validate the function is the proper type
                // This asserts that it's safe to call the function with the current stack
                let f_actual = &self.types[f.ty.0 as usize];

                if f_actual.params != f_expected.params || f_actual.returns != f_expected.returns {
                    return Err(InstructionError::InvalidTableFunctionType);
                }

                // Call the function
                self.call_impl(f, state);
                Ok(())
            }
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

    fn global_get(&self, g: GlobalVariable, state: &mut Self::State) -> Result<(), Self::Error> {
        match g.reference {
            GlobalVariableRef::Imported(i) => {
                let gi = &self.global_imports[i as usize];
                match gi.value.read().or(Err(InstructionError::GlobalGetFailed))? {
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
            }
            GlobalVariableRef::Internal(addr) => match g.ty {
                ValType::I32 | ValType::F32 => {
                    let val = state.stack.read_u32(addr as usize);
                    state.stack.write_u32(state.sp, val);
                    state.sp += 1;
                }
                ValType::I64 | ValType::F64 => {
                    let val = state.stack.read_u64(addr as usize);
                    state.stack.write_u64(state.sp, val);
                    state.sp += 2;
                }
            },
        }

        Ok(())
    }

    fn global_set(&self, g: GlobalVariable, state: &mut Self::State) -> Result<(), Self::Error> {
        match g.reference {
            GlobalVariableRef::Imported(i) => {
                let gi = &self.global_imports[i as usize];
                match g.ty {
                    ValType::I32 => {
                        state.sp -= 1;
                        let val = state.stack.read_u32(state.sp) as i32;
                        gi.value.write(Value::I32(val))
                    }
                    ValType::I64 => {
                        state.sp -= 2;
                        let val = state.stack.read_u64(state.sp) as i64;
                        gi.value.write(Value::I64(val))
                    }
                    ValType::F32 => {
                        state.sp -= 1;
                        let f = state.stack.read_f32(state.sp);
                        gi.value.write(Value::F32(f))
                    }
                    ValType::F64 => {
                        state.sp -= 2;
                        let f = state.stack.read_f64(state.sp);
                        gi.value.write(Value::F64(f))
                    }
                }
                .or(Err(InstructionError::GlobalSetFailed))?;
            }
            GlobalVariableRef::Internal(addr) => match g.ty {
                ValType::I32 | ValType::F32 => {
                    state.sp -= 1;
                    let val = state.stack.read_u32(state.sp);
                    state.stack.write_u32(addr as usize, val);
                }
                ValType::I64 | ValType::F64 => {
                    state.sp -= 2;
                    let val = state.stack.read_u64(state.sp);
                    state.stack.write_u64(addr as usize, val);
                }
            },
        }

        Ok(())
    }
}

#[cfg(test)]
#[path = "interpreter_tests.rs"]
mod interpreter_tests;
