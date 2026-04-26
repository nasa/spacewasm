use crate::*;

macro_rules! instruction {
    ($name:ident, f32 -> f32, $f:ident, $( $t:tt )*) => {
        fn $name(&self, state: &mut Self::State) -> Result<(), Self::Error> {
            let __f = &mut f32::from_bits(state.stack[state.sp - 1]);
            let $f = *__f;
            *__f = $($t)*;
            Ok(())
        }
    };
    ($name:ident, i32 -> i32, $i:ident, $( $t:tt )*) => {
        fn $name(&self, state: &mut Self::State) -> Result<(), Self::Error> {
            let __i = &mut state.stack[state.sp - 1];
            let $i = *__i as i32;
            *__i = ($($t)*) as u32;
            Ok(())
        }
    };
    ($name:ident, f64 -> f64, $f:ident, $( $t:tt )*) => {
        fn $name(&self, state: &mut Self::State) -> Result<(), Self::Error> {
            let lo = state.stack[state.sp - 2];
            let hi = state.stack[state.sp - 1];
            let $f = f64::from_bits((lo as u64) | ((hi as u64) << 32));
            let result = $($t)*;
            let bits = result.to_bits();
            state.stack[state.sp - 2] = bits as u32;
            state.stack[state.sp - 1] = (bits >> 32) as u32;
            Ok(())
        }
    };
    ($name:ident, i64 -> i64, $i:ident, $( $t:tt )*) => {
        fn $name(&self, state: &mut Self::State) -> Result<(), Self::Error> {
            let lo = state.stack[state.sp - 2];
            let hi = state.stack[state.sp - 1];
            let $i = ((lo as u64) | ((hi as u64) << 32)) as i64;
            let result = ($($t)*) as u64;
            state.stack[state.sp - 2] = result as u32;
            state.stack[state.sp - 1] = (result >> 32) as u32;
            Ok(())
        }
    };
    ($name:ident, f32, f32 -> f32, $a:ident, $b:ident, $( $t:tt )*) => {
        fn $name(&self, state: &mut Self::State) -> Result<(), Self::Error> {
            state.sp -= 1;
            let $b = f32::from_bits(state.stack[state.sp]);
            let __a = &mut f32::from_bits(state.stack[state.sp - 1]);
            let $a = *__a;
            *__a = $($t)*;
            Ok(())
        }
    };
    ($name:ident, i32, i32 -> i32, $a:ident, $b:ident, $( $t:tt )*) => {
        fn $name(&self, state: &mut Self::State) -> Result<(), Self::Error> {
            state.sp -= 1;
            let $b = state.stack[state.sp] as i32;
            let __a = &mut state.stack[state.sp - 1];
            let $a = *__a as i32;
            *__a = ($($t)*) as u32;
            Ok(())
        }
    };
    ($name:ident, i32, i32 -> bool, $a:ident, $b:ident, $( $t:tt )*) => {
        fn $name(&self, state: &mut Self::State) -> Result<(), Self::Error> {
            state.sp -= 1;
            let $b = state.stack[state.sp] as i32;
            let __a = &mut state.stack[state.sp - 1];
            let $a = *__a as i32;
            *__a = if $($t)* { 1 } else { 0 };
            Ok(())
        }
    };
    ($name:ident, f32, f32 -> bool, $a:ident, $b:ident, $( $t:tt )*) => {
        fn $name(&self, state: &mut Self::State) -> Result<(), Self::Error> {
            state.sp -= 1;
            let $b = f32::from_bits(state.stack[state.sp]);
            let __a = &mut state.stack[state.sp - 1];
            let $a = f32::from_bits(*__a);
            *__a = if $($t)* { 1 } else { 0 };
            Ok(())
        }
    };
    ($name:ident, f64, f64 -> f64, $a:ident, $b:ident, $( $t:tt )*) => {
        fn $name(&self, state: &mut Self::State) -> Result<(), Self::Error> {
            state.sp -= 2;
            let b_lo = state.stack[state.sp];
            let b_hi = state.stack[state.sp + 1];
            let $b = f64::from_bits((b_lo as u64) | ((b_hi as u64) << 32));

            let a_lo = state.stack[state.sp - 2];
            let a_hi = state.stack[state.sp - 1];
            let $a = f64::from_bits((a_lo as u64) | ((a_hi as u64) << 32));

            let result = $($t)*;
            let bits = result.to_bits();
            state.stack[state.sp - 2] = bits as u32;
            state.stack[state.sp - 1] = (bits >> 32) as u32;
            Ok(())
        }
    };
    ($name:ident, i64, i64 -> i64, $a:ident, $b:ident, $( $t:tt )*) => {
        fn $name(&self, state: &mut Self::State) -> Result<(), Self::Error> {
            state.sp -= 2;
            let b_lo = state.stack[state.sp];
            let b_hi = state.stack[state.sp + 1];
            let $b = ((b_lo as u64) | ((b_hi as u64) << 32)) as i64;

            let a_lo = state.stack[state.sp - 2];
            let a_hi = state.stack[state.sp - 1];
            let $a = ((a_lo as u64) | ((a_hi as u64) << 32)) as i64;

            let result = ($($t)*) as u64;
            state.stack[state.sp - 2] = result as u32;
            state.stack[state.sp - 1] = (result >> 32) as u32;
            Ok(())
        }
    };
    ($name:ident, i64, i64 -> bool, $a:ident, $b:ident, $( $t:tt )*) => {
        fn $name(&self, state: &mut Self::State) -> Result<(), Self::Error> {
            state.sp -= 3;
            let b_lo = state.stack[state.sp + 1];
            let b_hi = state.stack[state.sp + 2];
            let $b = ((b_lo as u64) | ((b_hi as u64) << 32)) as i64;

            let a_lo = state.stack[state.sp - 1];
            let a_hi = state.stack[state.sp];
            let $a = ((a_lo as u64) | ((a_hi as u64) << 32)) as i64;

            state.stack[state.sp] = if $($t)* { 1 } else { 0 };
            Ok(())
        }
    };
    ($name:ident, f64, f64 -> bool, $a:ident, $b:ident, $( $t:tt )*) => {
        fn $name(&self, state: &mut Self::State) -> Result<(), Self::Error> {
            state.sp -= 3;
            let b_lo = state.stack[state.sp + 1];
            let b_hi = state.stack[state.sp + 2];
            let $b = f64::from_bits((b_lo as u64) | ((b_hi as u64) << 32));

            let a_lo = state.stack[state.sp - 1];
            let a_hi = state.stack[state.sp];
            let $a = f64::from_bits((a_lo as u64) | ((a_hi as u64) << 32));

            state.stack[state.sp] = if $($t)* { 1 } else { 0 };
            Ok(())
        }
    };
}

pub struct InterpreterState {
    pc: JumpTarget,
    sp: usize,
    stack: Box<[u32]>,
}

impl InterpreterState {
    pub fn new(stack_size: usize) -> Self {
        InterpreterState {
            pc: JumpTarget(0x0),
            sp: stack_size,
            stack: unsafe { Vec::new(1024).unwrap().assume_init() }.into_boxed_slice(),
        }
    }

    pub fn pop_i32(&mut self) -> i32 {
        self.sp -= 1;
        self.stack[self.sp] as i32
    }

    pub fn pop_i64(&mut self) -> i64 {
        self.sp -= 2;
        let lo = self.stack[self.sp];
        let hi = self.stack[self.sp + 1];
        ((lo as u64) | ((hi as u64) << 32)) as i64
    }
    pub fn pop_f32(&mut self) -> f32 {
        self.sp -= 1;
        f32::from_bits(self.stack[self.sp])
    }

    pub fn pop_f64(&mut self) -> f64 {
        self.sp -= 2;
        let lo = self.stack[self.sp];
        let hi = self.stack[self.sp + 1];
        f64::from_bits((lo as u64) | ((hi as u64) << 32))
    }
}

pub struct Interpreter<'wasm> {
    code: Code<'wasm>,
}

impl<'wasm> Interpreter<'wasm> {}

pub enum InterpreterError {
    Trap,
    Finished,
    TableLookupFailed,
}

impl<'wasm> BaseVisitor for Interpreter<'wasm> {
    type Error = InterpreterError;
    type State = InterpreterState;

    fn finish(&self, _: &mut Self::State) -> Result<(), Self::Error> {
        Ok(())
    }

    fn unreachable(&self, _: &mut Self::State) -> Result<(), Self::Error> {
        Err(InterpreterError::Trap)
    }

    fn nop(&self, _: &mut Self::State) -> Result<(), Self::Error> {
        Ok(())
    }

    fn return_(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        todo!()
    }

    fn call(&self, x: FuncIdx, state: &mut Self::State) -> Result<(), Self::Error> {
        todo!()
    }

    fn call_indirect(&self, x: TypeIdx, state: &mut Self::State) -> Result<(), Self::Error> {
        todo!()
    }

    fn drop(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 1;
        Ok(())
    }

    fn select(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let c = state.pop_i32();
        state.sp -= 1;
        let val2 = state.stack[state.sp];
        let val1 = state.stack[state.sp - 1];
        state.stack[state.sp - 1] = if c != 0 { val1 } else { val2 };
        Ok(())
    }

    fn local_get(&self, x: LocalIdx, state: &mut Self::State) -> Result<(), Self::Error> {
        todo!()
    }

    fn local_set(&self, x: LocalIdx, state: &mut Self::State) -> Result<(), Self::Error> {
        todo!()
    }

    fn local_tee(&self, x: LocalIdx, state: &mut Self::State) -> Result<(), Self::Error> {
        todo!()
    }

    fn global_get(&self, x: GlobalIdx, state: &mut Self::State) -> Result<(), Self::Error> {
        todo!()
    }

    fn global_set(&self, x: GlobalIdx, state: &mut Self::State) -> Result<(), Self::Error> {
        todo!()
    }

    fn i32_load(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        todo!()
    }

    fn i64_load(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        todo!()
    }

    fn f32_load(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        todo!()
    }

    fn f64_load(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        todo!()
    }

    fn i32_load8_s(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        todo!()
    }

    fn i32_load8_u(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        todo!()
    }

    fn i32_load16_s(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        todo!()
    }

    fn i32_load16_u(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        todo!()
    }

    fn i64_load8_s(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        todo!()
    }

    fn i64_load8_u(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        todo!()
    }

    fn i64_load16_s(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        todo!()
    }

    fn i64_load16_u(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        todo!()
    }

    fn i64_load32_s(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        todo!()
    }

    fn i64_load32_u(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        todo!()
    }

    fn i32_store(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        todo!()
    }

    fn i64_store(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        todo!()
    }

    fn f32_store(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        todo!()
    }

    fn f64_store(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        todo!()
    }

    fn i32_store8(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        todo!()
    }

    fn i32_store16(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        todo!()
    }

    fn i64_store8(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        todo!()
    }

    fn i64_store16(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        todo!()
    }

    fn i64_store32(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
        todo!()
    }

    fn memory_size(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        todo!()
    }

    fn memory_grow(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        // This instruction should be disabled
        let _ = state;
        unreachable!()
    }

    fn i32_const(&self, n: i32, state: &mut Self::State) -> Result<(), Self::Error> {
        state.stack[state.sp] = n as u32;
        state.sp += 1;
        Ok(())
    }

    fn i64_const(&self, n: i64, state: &mut Self::State) -> Result<(), Self::Error> {
        let bits = n as u64;
        state.stack[state.sp] = bits as u32;
        state.stack[state.sp + 1] = (bits >> 32) as u32;
        state.sp += 2;
        Ok(())
    }

    fn f32_const(&self, z: f32, state: &mut Self::State) -> Result<(), Self::Error> {
        state.stack[state.sp] = z.to_bits();
        state.sp += 1;
        Ok(())
    }

    fn f64_const(&self, z: f64, state: &mut Self::State) -> Result<(), Self::Error> {
        let bits = z.to_bits();
        state.stack[state.sp] = bits as u32;
        state.stack[state.sp + 1] = (bits >> 32) as u32;
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
        let f = f32::from_bits(state.stack[state.sp - 1]);
        state.stack[state.sp - 1] = f as i32 as u32;
        Ok(())
    }

    fn i32_trunc_f32_u(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let f = f32::from_bits(state.stack[state.sp - 1]);
        state.stack[state.sp - 1] = f as u32;
        Ok(())
    }

    fn i32_trunc_f64_s(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 1;
        let lo = state.stack[state.sp - 1];
        let hi = state.stack[state.sp];
        let f = f64::from_bits((lo as u64) | ((hi as u64) << 32));
        state.stack[state.sp - 1] = f as i32 as u32;
        Ok(())
    }

    fn i32_trunc_f64_u(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 1;
        let lo = state.stack[state.sp - 1];
        let hi = state.stack[state.sp];
        let f = f64::from_bits((lo as u64) | ((hi as u64) << 32));
        state.stack[state.sp - 1] = f as u32;
        Ok(())
    }

    fn i64_extend_i32_s(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let i = state.stack[state.sp - 1] as i32;
        let extended = i as i64 as u64;
        state.stack[state.sp - 1] = extended as u32;
        state.stack[state.sp] = (extended >> 32) as u32;
        state.sp += 1;
        Ok(())
    }

    fn i64_extend_i32_u(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        // Low word is already in place at [sp-1]
        // Just add high word as 0 for unsigned extension
        state.stack[state.sp] = 0;
        state.sp += 1;
        Ok(())
    }

    fn i64_trunc_f32_s(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let f = f32::from_bits(state.stack[state.sp - 1]);
        let i = f as i64 as u64;
        state.stack[state.sp - 1] = i as u32;
        state.stack[state.sp] = (i >> 32) as u32;
        state.sp += 1;
        Ok(())
    }

    fn i64_trunc_f32_u(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let f = f32::from_bits(state.stack[state.sp - 1]);
        let u = f as u64;
        state.stack[state.sp - 1] = u as u32;
        state.stack[state.sp] = (u >> 32) as u32;
        state.sp += 1;
        Ok(())
    }

    fn i64_trunc_f64_s(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let lo = state.stack[state.sp - 2];
        let hi = state.stack[state.sp - 1];
        let f = f64::from_bits((lo as u64) | ((hi as u64) << 32));
        let i = f as i64 as u64;
        state.stack[state.sp - 2] = i as u32;
        state.stack[state.sp - 1] = (i >> 32) as u32;
        Ok(())
    }

    fn i64_trunc_f64_u(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let lo = state.stack[state.sp - 2];
        let hi = state.stack[state.sp - 1];
        let f = f64::from_bits((lo as u64) | ((hi as u64) << 32));
        let u = f as u64;
        state.stack[state.sp - 2] = u as u32;
        state.stack[state.sp - 1] = (u >> 32) as u32;
        Ok(())
    }

    fn f32_convert_i32_s(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let i = state.stack[state.sp - 1] as i32;
        state.stack[state.sp - 1] = (i as f32).to_bits();
        Ok(())
    }

    fn f32_convert_i32_u(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let u = state.stack[state.sp - 1];
        state.stack[state.sp - 1] = (u as f32).to_bits();
        Ok(())
    }

    fn f32_convert_i64_s(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 1;
        let lo = state.stack[state.sp - 1];
        let hi = state.stack[state.sp];
        let i = ((lo as u64) | ((hi as u64) << 32)) as i64;
        state.stack[state.sp - 1] = (i as f32).to_bits();
        Ok(())
    }

    fn f32_convert_i64_u(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 1;
        let lo = state.stack[state.sp - 1];
        let hi = state.stack[state.sp];
        let u = (lo as u64) | ((hi as u64) << 32);
        state.stack[state.sp - 1] = (u as f32).to_bits();
        Ok(())
    }

    fn f32_demote_f64(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        state.sp -= 1;
        let lo = state.stack[state.sp - 1];
        let hi = state.stack[state.sp];
        let f = f64::from_bits((lo as u64) | ((hi as u64) << 32));
        state.stack[state.sp - 1] = (f as f32).to_bits();
        Ok(())
    }

    fn f64_convert_i32_s(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let i = state.stack[state.sp - 1] as i32;
        let f = (i as f64).to_bits();
        state.stack[state.sp - 1] = f as u32;
        state.stack[state.sp] = (f >> 32) as u32;
        state.sp += 1;
        Ok(())
    }

    fn f64_convert_i32_u(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let u = state.stack[state.sp - 1];
        let f = (u as f64).to_bits();
        state.stack[state.sp - 1] = f as u32;
        state.stack[state.sp] = (f >> 32) as u32;
        state.sp += 1;
        Ok(())
    }

    fn f64_convert_i64_s(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let lo = state.stack[state.sp - 2];
        let hi = state.stack[state.sp - 1];
        let i = ((lo as u64) | ((hi as u64) << 32)) as i64;
        let f = (i as f64).to_bits();
        state.stack[state.sp - 2] = f as u32;
        state.stack[state.sp - 1] = (f >> 32) as u32;
        Ok(())
    }

    fn f64_convert_i64_u(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let lo = state.stack[state.sp - 2];
        let hi = state.stack[state.sp - 1];
        let u = (lo as u64) | ((hi as u64) << 32);
        let f = (u as f64).to_bits();
        state.stack[state.sp - 2] = f as u32;
        state.stack[state.sp - 1] = (f >> 32) as u32;
        Ok(())
    }

    fn f64_promote_f32(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let f32_val = f32::from_bits(state.stack[state.sp - 1]);
        let f64_val = (f32_val as f64).to_bits();
        state.stack[state.sp - 1] = f64_val as u32;
        state.stack[state.sp] = (f64_val >> 32) as u32;
        state.sp += 1;
        Ok(())
    }

    fn i32_reinterpret_f32(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        // This instruction does not exist in the IR
        let _ = state;
        unreachable!()
    }

    fn i64_reinterpret_f64(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        // This instruction does not exist in the IR
        let _ = state;
        unreachable!()
    }

    fn f32_reinterpret_i32(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        // This instruction does not exist in the IR
        let _ = state;
        unreachable!()
    }

    fn f64_reinterpret_i64(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        // This instruction does not exist in the IR
        let _ = state;
        unreachable!()
    }
}

impl<'wasm> IrVisitor for Interpreter<'wasm> {
    fn if_(&self, false_address: JumpTarget, state: &mut Self::State) -> Result<(), Self::Error> {
        let v = state.pop_i32();
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
        let v = state.pop_i32();
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
        let v = state.pop_i32();
        let Ok(addr) = cases(v as u16) else {
            return Err(InterpreterError::TableLookupFailed);
        };

        state.pc = addr;
        Ok(())
    }
}
