use crate::*;
use core::marker::PhantomData;

pub struct Compiler<'a, const N: usize> {
    _marker: PhantomData<&'a ()>,
}

impl<'a, const N: usize> Compiler<'a, N> {
    pub fn new() -> Compiler<'a, N> {
        Compiler {
            _marker: PhantomData,
        }
    }
}

macro_rules! instruction {
    // No additional operands
    ($name:ident, $opcode:expr) => {
        fn $name(&self, state: &mut Self::State) -> Result<(), Self::Error> {
            state.push_no_operand($opcode)?;
            Ok(())
        }
    };

    // An instruction with a MemArg operand
    ($name:ident, $opcode:expr, mem) => {
        fn $name(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
            state.push_mem($opcode, m)?;
            Ok(())
        }
    };
}

impl<'a, const N: usize> WasmVisitor for Compiler<'a, N> {
    fn enter_block(
        &self,
        block_type: ResultType,
        state: &mut Self::State,
    ) -> Result<(), Self::Error> {
        // TODO(tumbar) Verify the block type
        let _ = block_type;

        state.enter_forward_block()?;
        Ok(())
    }

    fn exit_block(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        state.exit_block()
    }

    fn loop_(&self, block_type: ResultType, state: &mut Self::State) -> Result<(), Self::Error> {
        // TODO(tumbar) Verify the block type
        let _ = block_type;

        state.enter_backward_block()?;
        Ok(())
    }

    fn if_(&self, block_type: ResultType, state: &mut Self::State) -> Result<(), Self::Error> {
        // TODO(tumbar) Verify the block type
        let _ = block_type;

        state.push_no_operand(IF)?;
        state.start_else()?;
        state.enter_forward_if_block()?;
        Ok(())
    }

    fn else_(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        // Perform an unconditional branch to the end of the 'if'
        state.push_no_operand(BR)?;
        state.push_jump_target(LabelIdx(0))?;

        // Fill in the else branch target
        state.finish_else()?;

        Ok(())
    }

    fn br(&self, l: LabelIdx, state: &mut Self::State) -> Result<(), Self::Error> {
        state.push_no_operand(BR)?;
        state.push_jump_target(l)
    }

    fn br_if(&self, l: LabelIdx, state: &mut Self::State) -> Result<(), Self::Error> {
        state.push_no_operand(BR_IF)?;
        state.push_jump_target(l)
    }

    fn br_table(
        &self,
        lut: &[LabelIdx],
        default_: LabelIdx,
        state: &mut Self::State,
    ) -> Result<(), Self::Error> {
        state.push_8_or_16(BR_TABLE, lut.len() as u32)?;
        state.push_jump_target(default_)?;
        for l in lut {
            state.push_jump_target(*l)?;
        }

        Ok(())
    }

    fn return_(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        match state.context() {
            TextContext::Constant => Err(ValidationError::InvalidConstInstruction),
            TextContext::Function(f) => {
                // Return instructions also encode the return size from their function's context
                state.push_with_operand(RETURN, f.return_size)?;
                Ok(())
            }
        }
    }

    fn call(&self, x: FuncIdx, state: &mut Self::State) -> Result<(), Self::Error> {
        state.push_8_or_16(CALL, x.0)
    }

    fn call_indirect(&self, x: TypeIdx, state: &mut Self::State) -> Result<(), Self::Error> {
        // FIXME(tumbar) How do I implement this?
        state.push_8_or_16(CALL_INDIRECT, x.0)
    }

    fn local_get(&self, x: LocalIdx, state: &mut Self::State) -> Result<(), Self::Error> {
        let l = state.get_local(x)?;
        state.push_local(LOCAL_GET, l)?;
        Ok(())
    }

    fn local_set(&self, x: LocalIdx, state: &mut Self::State) -> Result<(), Self::Error> {
        let l = state.get_local(x)?;
        state.push_local(LOCAL_SET, l)?;
        Ok(())
    }

    fn local_tee(&self, x: LocalIdx, state: &mut Self::State) -> Result<(), Self::Error> {
        let l = state.get_local(x)?;
        state.push_local(LOCAL_TEE, l)?;
        Ok(())
    }

    fn global_get(&self, x: GlobalIdx, state: &mut Self::State) -> Result<(), Self::Error> {
        let g = state.get_global(x)?;
        state.push_global(GLOBAL_GET, g)?;
        Ok(())
    }

    fn global_set(&self, x: GlobalIdx, state: &mut Self::State) -> Result<(), Self::Error> {
        let g = state.get_global(x)?;
        if !g.mutable {
            Err(ValidationError::GlobalIsNotMutable)
        } else {
            state.push_global(GLOBAL_SET, g)?;
            Ok(())
        }
    }
}

impl<'a, const N: usize> BaseVisitor for Compiler<'a, N> {
    type Error = ValidationError;
    type State = TextBuilder<'a, 'a, N>;

    instruction!(finish, END);
    instruction!(unreachable, UNREACHABLE);
    instruction!(nop, NOP);

    instruction!(drop, DROP);
    instruction!(select, SELECT);

    instruction!(i32_load, I32_LOAD, mem);
    instruction!(i64_load, I64_LOAD, mem);
    instruction!(f32_load, F32_LOAD, mem);
    instruction!(f64_load, F64_LOAD, mem);
    instruction!(i32_load8_s, I32_LOAD8_S, mem);
    instruction!(i32_load8_u, I32_LOAD8_U, mem);
    instruction!(i32_load16_s, I32_LOAD16_S, mem);
    instruction!(i32_load16_u, I32_LOAD16_U, mem);
    instruction!(i64_load8_s, I64_LOAD8_S, mem);
    instruction!(i64_load8_u, I64_LOAD8_U, mem);
    instruction!(i64_load16_s, I64_LOAD16_S, mem);
    instruction!(i64_load16_u, I64_LOAD16_U, mem);
    instruction!(i64_load32_s, I64_LOAD32_S, mem);
    instruction!(i64_load32_u, I64_LOAD32_U, mem);

    instruction!(i32_store, I32_STORE, mem);
    instruction!(i64_store, I64_STORE, mem);
    instruction!(f32_store, F32_STORE, mem);
    instruction!(f64_store, F64_STORE, mem);
    instruction!(i32_store8, I32_STORE8, mem);
    instruction!(i32_store16, I32_STORE16, mem);
    instruction!(i64_store8, I64_STORE8, mem);
    instruction!(i64_store16, I64_STORE16, mem);
    instruction!(i64_store32, I64_STORE32, mem);

    instruction!(memory_size, MEMORY_SIZE);

    fn memory_grow(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let _ = state;

        // Memory grow is not a legal instruction in SpaceWASM
        Err(ValidationError::IllegalMemoryGrow)
    }

    fn i32_const(&self, n: i32, state: &mut Self::State) -> Result<(), Self::Error> {
        state.push_8_or_32(I32_CONST, n as u32)?;
        Ok(())
    }

    fn i64_const(&self, n: i64, state: &mut Self::State) -> Result<(), Self::Error> {
        state.push_8_or_64(I64_CONST, n as u64)?;
        Ok(())
    }

    fn f32_const(&self, z: f32, state: &mut Self::State) -> Result<(), Self::Error> {
        state.push_no_operand(I32_CONST)?;
        state.push_32(z.to_bits())?;
        Ok(())
    }

    fn f64_const(&self, z: f64, state: &mut Self::State) -> Result<(), Self::Error> {
        state.push_no_operand(I64_CONST)?;
        state.push_64(z.to_bits())?;
        Ok(())
    }

    instruction!(i32_eqz, I32_EQZ);
    instruction!(i32_eq, I32_EQ);
    instruction!(i32_ne, I32_NE);
    instruction!(i32_lt_s, I32_LT_S);
    instruction!(i32_lt_u, I32_LT_U);
    instruction!(i32_gt_s, I32_GT_S);
    instruction!(i32_gt_u, I32_GT_U);
    instruction!(i32_le_s, I32_LE_S);
    instruction!(i32_le_u, I32_LE_U);
    instruction!(i32_ge_s, I32_GE_S);
    instruction!(i32_ge_u, I32_GE_U);

    instruction!(i64_eqz, I64_EQZ);
    instruction!(i64_eq, I64_EQ);
    instruction!(i64_ne, I64_NE);
    instruction!(i64_lt_s, I64_LT_S);
    instruction!(i64_lt_u, I64_LT_U);
    instruction!(i64_gt_s, I64_GT_S);
    instruction!(i64_gt_u, I64_GT_U);
    instruction!(i64_le_s, I64_LE_S);
    instruction!(i64_le_u, I64_LE_U);
    instruction!(i64_ge_s, I64_GE_S);
    instruction!(i64_ge_u, I64_GE_U);

    instruction!(f32_eq, F32_EQ);
    instruction!(f32_ne, F32_NE);
    instruction!(f32_lt, F32_LT);
    instruction!(f32_gt, F32_GT);
    instruction!(f32_le, F32_LE);
    instruction!(f32_ge, F32_GE);

    instruction!(f64_eq, F64_EQ);
    instruction!(f64_ne, F64_NE);
    instruction!(f64_lt, F64_LT);
    instruction!(f64_gt, F64_GT);
    instruction!(f64_le, F64_LE);
    instruction!(f64_ge, F64_GE);

    instruction!(i32_clz, I32_CLZ);
    instruction!(i32_ctz, I32_CTZ);
    instruction!(i32_popcnt, I32_POPCNT);
    instruction!(i32_add, I32_ADD);
    instruction!(i32_sub, I32_SUB);
    instruction!(i32_mul, I32_MUL);
    instruction!(i32_div_s, I32_DIV_S);
    instruction!(i32_div_u, I32_DIV_U);
    instruction!(i32_rem_s, I32_REM_S);
    instruction!(i32_rem_u, I32_REM_U);
    instruction!(i32_and, I32_AND);
    instruction!(i32_or, I32_OR);
    instruction!(i32_xor, I32_XOR);
    instruction!(i32_shl, I32_SHL);
    instruction!(i32_shr_s, I32_SHR_S);
    instruction!(i32_shr_u, I32_SHR_U);
    instruction!(i32_rotl, I32_ROTL);
    instruction!(i32_rotr, I32_ROTR);

    instruction!(i64_clz, I64_CLZ);
    instruction!(i64_ctz, I64_CTZ);
    instruction!(i64_popcnt, I64_POPCNT);
    instruction!(i64_add, I64_ADD);
    instruction!(i64_sub, I64_SUB);
    instruction!(i64_mul, I64_MUL);
    instruction!(i64_div_s, I64_DIV_S);
    instruction!(i64_div_u, I64_DIV_U);
    instruction!(i64_rem_s, I64_REM_S);
    instruction!(i64_rem_u, I64_REM_U);
    instruction!(i64_and, I64_AND);
    instruction!(i64_or, I64_OR);
    instruction!(i64_xor, I64_XOR);
    instruction!(i64_shl, I64_SHL);
    instruction!(i64_shr_s, I64_SHR_S);
    instruction!(i64_shr_u, I64_SHR_U);
    instruction!(i64_rotl, I64_ROTL);
    instruction!(i64_rotr, I64_ROTR);

    instruction!(f32_abs, F32_ABS);
    instruction!(f32_neg, F32_NEG);
    instruction!(f32_ceil, F32_CEIL);
    instruction!(f32_floor, F32_FLOOR);
    instruction!(f32_trunc, F32_TRUNC);
    instruction!(f32_nearest, F32_NEAREST);
    instruction!(f32_sqrt, F32_SQRT);
    instruction!(f32_add, F32_ADD);
    instruction!(f32_sub, F32_SUB);
    instruction!(f32_mul, F32_MUL);
    instruction!(f32_div, F32_DIV);
    instruction!(f32_min, F32_MIN);
    instruction!(f32_max, F32_MAX);
    instruction!(f32_copysign, F32_COPYSIGN);

    instruction!(f64_abs, F64_ABS);
    instruction!(f64_neg, F64_NEG);
    instruction!(f64_ceil, F64_CEIL);
    instruction!(f64_floor, F64_FLOOR);
    instruction!(f64_trunc, F64_TRUNC);
    instruction!(f64_nearest, F64_NEAREST);
    instruction!(f64_sqrt, F64_SQRT);
    instruction!(f64_add, F64_ADD);
    instruction!(f64_sub, F64_SUB);
    instruction!(f64_mul, F64_MUL);
    instruction!(f64_div, F64_DIV);
    instruction!(f64_min, F64_MIN);
    instruction!(f64_max, F64_MAX);
    instruction!(f64_copysign, F64_COPYSIGN);

    instruction!(i32_wrap_i64, I32_WRAP_I64);
    instruction!(i32_trunc_f32_s, I32_TRUNC_F32_S);
    instruction!(i32_trunc_f32_u, I32_TRUNC_F32_U);
    instruction!(i32_trunc_f64_s, I32_TRUNC_F64_S);
    instruction!(i32_trunc_f64_u, I32_TRUNC_F64_U);
    instruction!(i64_extend_i32_s, I64_EXTEND_I32_S);
    instruction!(i64_extend_i32_u, I64_EXTEND_I32_U);
    instruction!(i64_trunc_f32_s, I64_TRUNC_F32_S);
    instruction!(i64_trunc_f32_u, I64_TRUNC_F32_U);
    instruction!(i64_trunc_f64_s, I64_TRUNC_F64_S);
    instruction!(i64_trunc_f64_u, I64_TRUNC_F64_U);
    instruction!(f32_convert_i32_s, F32_CONVERT_I32_S);
    instruction!(f32_convert_i32_u, F32_CONVERT_I32_U);
    instruction!(f32_convert_i64_s, F32_CONVERT_I64_S);
    instruction!(f32_convert_i64_u, F32_CONVERT_I64_U);
    instruction!(f32_demote_f64, F32_DEMOTE_F64);
    instruction!(f64_convert_i32_s, F64_CONVERT_I32_S);
    instruction!(f64_convert_i32_u, F64_CONVERT_I32_U);
    instruction!(f64_convert_i64_s, F64_CONVERT_I64_S);
    instruction!(f64_convert_i64_u, F64_CONVERT_I64_U);
    instruction!(f64_promote_f32, F64_PROMOTE_F32);

    fn i32_reinterpret_f32(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        // TODO(tumbar) Update the validator
        // This is a bitwise transmute and therefore we don't need this in the IR
        let _ = state;
        Ok(())
    }

    fn f64_reinterpret_i64(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        // TODO(tumbar) Update the validator
        // This is a bitwise transmute and therefore we don't need this in the IR
        let _ = state;
        Ok(())
    }

    fn f32_reinterpret_i32(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        // TODO(tumbar) Update the validator
        // This is a bitwise transmute and therefore we don't need this in the IR
        let _ = state;
        Ok(())
    }

    fn i64_reinterpret_f64(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        // TODO(tumbar) Update the validator
        // This is a bitwise transmute and therefore we don't need this in the IR
        let _ = state;
        Ok(())
    }
}
