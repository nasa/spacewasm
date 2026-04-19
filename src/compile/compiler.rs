use crate::*;

pub struct Compiler<const N: usize>;

macro_rules! compile_impl {
    // No additional parameters
    ($name:ident, $opcode:expr) => {
        fn $name(&self, state: &mut Self::State) -> Result<(), Self::Error> {
            state.push_no_operand($opcode)?;
            Ok(())
        }
    };

    // Compile an instruction with a 7-bit or 23-bit parameter
    ($name:ident, $opcode:expr, idx, $ty:ty) => {
        fn $name(&self, x: $ty, state: &mut Self::State) -> Result<(), Self::Error> {
            state.push_23($opcode, x.0)?;
            Ok(())
        }
    };

    // Compile an instruction with an 8-bit or 32-bit parameter
    ($name:ident, $opcode:expr, 32, $n:ident: $ty:ty, $e:expr) => {
        fn $name(&self, $n: $ty, state: &mut Self::State) -> Result<(), Self::Error> {
            state.push_8_or_32($opcode, $e)?;
            Ok(())
        }
    };

    // Compile an instruction with an 8-bit or 64-bit parameter
    ($name:ident, $opcode:expr, 64, $n:ident: $ty:ty, $e:expr) => {
        fn $name(&self, $n: $ty, state: &mut Self::State) -> Result<(), Self::Error> {
            state.push_8_or_64($opcode, $e)?;
            Ok(())
        }
    };

    ($name:ident, $opcode:expr, mem) => {
        fn $name(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
            state.push_mem($opcode, m)?;
            Ok(())
        }
    };
}

impl<const N: usize> CodeVisitor for Compiler<N> {
    type Error = ValidationError;
    type State = TextBuilder<N>;

    compile_impl!(unreachable, UNREACHABLE);
    compile_impl!(nop, NOP);

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
        state.enter_forward_block()?;
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
        state.push_23(BR_TABLE, lut.len() as u32)?;
        state.push_jump_target(default_)?;
        for l in lut {
            state.push_jump_target(*l)?;
        }

        Ok(())
    }

    compile_impl!(return_, RETURN);
    compile_impl!(call, CALL, idx, FuncIdx);
    compile_impl!(call_indirect, CALL_INDIRECT, idx, TypeIdx);

    compile_impl!(drop, DROP);
    compile_impl!(select, SELECT);

    compile_impl!(local_get, LOCAL_GET, idx, LocalIdx);
    compile_impl!(local_set, LOCAL_SET, idx, LocalIdx);
    compile_impl!(local_tee, LOCAL_TEE, idx, LocalIdx);
    compile_impl!(global_get, GLOBAL_GET, idx, GlobalIdx);
    compile_impl!(global_set, GLOBAL_SET, idx, GlobalIdx);

    compile_impl!(i32_load, I32_LOAD, mem);
    compile_impl!(i64_load, I64_LOAD, mem);
    compile_impl!(f32_load, F32_LOAD, mem);
    compile_impl!(f64_load, F64_LOAD, mem);
    compile_impl!(i32_load8_s, I32_LOAD8_S, mem);
    compile_impl!(i32_load8_u, I32_LOAD8_U, mem);
    compile_impl!(i32_load16_s, I32_LOAD16_S, mem);
    compile_impl!(i32_load16_u, I32_LOAD16_U, mem);
    compile_impl!(i64_load8_s, I64_LOAD8_S, mem);
    compile_impl!(i64_load8_u, I64_LOAD8_U, mem);
    compile_impl!(i64_load16_s, I64_LOAD16_S, mem);
    compile_impl!(i64_load16_u, I64_LOAD16_U, mem);
    compile_impl!(i64_load32_s, I64_LOAD32_S, mem);
    compile_impl!(i64_load32_u, I64_LOAD32_U, mem);

    compile_impl!(i32_store, I32_STORE, mem);
    compile_impl!(i64_store, I64_STORE, mem);
    compile_impl!(f32_store, F32_STORE, mem);
    compile_impl!(f64_store, F64_STORE, mem);
    compile_impl!(i32_store8, I32_STORE8, mem);
    compile_impl!(i32_store16, I32_STORE16, mem);
    compile_impl!(i64_store8, I64_STORE8, mem);
    compile_impl!(i64_store16, I64_STORE16, mem);
    compile_impl!(i64_store32, I64_STORE32, mem);

    compile_impl!(memory_size, MEMORY_SIZE);

    fn memory_grow(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let _ = state;

        // Memory grow is not a legal instruction in SpaceWASM
        Err(ValidationError::IllegalMemoryGrow)
    }

    compile_impl!(i32_const, I32_CONST, 32, n: i32, n as u32);
    compile_impl!(i64_const, I64_CONST, 64, n: i64, n as u64);
    compile_impl!(f32_const, F32_CONST, 32, z: f32, z.to_bits());
    compile_impl!(f64_const, F64_CONST, 64, z: f64, z.to_bits());

    compile_impl!(i32_eqz, I32_EQZ);
    compile_impl!(i32_eq, I32_EQ);
    compile_impl!(i32_ne, I32_NE);
    compile_impl!(i32_lt_s, I32_LT_S);
    compile_impl!(i32_lt_u, I32_LT_U);
    compile_impl!(i32_gt_s, I32_GT_S);
    compile_impl!(i32_gt_u, I32_GT_U);
    compile_impl!(i32_le_s, I32_LE_S);
    compile_impl!(i32_le_u, I32_LE_U);
    compile_impl!(i32_ge_s, I32_GE_S);
    compile_impl!(i32_ge_u, I32_GE_U);

    compile_impl!(i64_eqz, I64_EQZ);
    compile_impl!(i64_eq, I64_EQ);
    compile_impl!(i64_ne, I64_NE);
    compile_impl!(i64_lt_s, I64_LT_S);
    compile_impl!(i64_lt_u, I64_LT_U);
    compile_impl!(i64_gt_s, I64_GT_S);
    compile_impl!(i64_gt_u, I64_GT_U);
    compile_impl!(i64_le_s, I64_LE_S);
    compile_impl!(i64_le_u, I64_LE_U);
    compile_impl!(i64_ge_s, I64_GE_S);
    compile_impl!(i64_ge_u, I64_GE_U);

    compile_impl!(f32_eq, F32_EQ);
    compile_impl!(f32_ne, F32_NE);
    compile_impl!(f32_lt, F32_LT);
    compile_impl!(f32_gt, F32_GT);
    compile_impl!(f32_le, F32_LE);
    compile_impl!(f32_ge, F32_GE);

    compile_impl!(f64_eq, F64_EQ);
    compile_impl!(f64_ne, F64_NE);
    compile_impl!(f64_lt, F64_LT);
    compile_impl!(f64_gt, F64_GT);
    compile_impl!(f64_le, F64_LE);
    compile_impl!(f64_ge, F64_GE);

    compile_impl!(i32_clz, I32_CLZ);
    compile_impl!(i32_ctz, I32_CTZ);
    compile_impl!(i32_popcnt, I32_POPCNT);
    compile_impl!(i32_add, I32_ADD);
    compile_impl!(i32_sub, I32_SUB);
    compile_impl!(i32_mul, I32_MUL);
    compile_impl!(i32_div_s, I32_DIV_S);
    compile_impl!(i32_div_u, I32_DIV_U);
    compile_impl!(i32_rem_s, I32_REM_S);
    compile_impl!(i32_rem_u, I32_REM_U);
    compile_impl!(i32_and, I32_AND);
    compile_impl!(i32_or, I32_OR);
    compile_impl!(i32_xor, I32_XOR);
    compile_impl!(i32_shl, I32_SHL);
    compile_impl!(i32_shr_s, I32_SHR_S);
    compile_impl!(i32_shr_u, I32_SHR_U);
    compile_impl!(i32_rotl, I32_ROTL);
    compile_impl!(i32_rotr, I32_ROTR);

    compile_impl!(i64_clz, I64_CLZ);
    compile_impl!(i64_ctz, I64_CTZ);
    compile_impl!(i64_popcnt, I64_POPCNT);
    compile_impl!(i64_add, I64_ADD);
    compile_impl!(i64_sub, I64_SUB);
    compile_impl!(i64_mul, I64_MUL);
    compile_impl!(i64_div_s, I64_DIV_S);
    compile_impl!(i64_div_u, I64_DIV_U);
    compile_impl!(i64_rem_s, I64_REM_S);
    compile_impl!(i64_rem_u, I64_REM_U);
    compile_impl!(i64_and, I64_AND);
    compile_impl!(i64_or, I64_OR);
    compile_impl!(i64_xor, I64_XOR);
    compile_impl!(i64_shl, I64_SHL);
    compile_impl!(i64_shr_s, I64_SHR_S);
    compile_impl!(i64_shr_u, I64_SHR_U);
    compile_impl!(i64_rotl, I64_ROTL);
    compile_impl!(i64_rotr, I64_ROTR);

    compile_impl!(f32_abs, F32_ABS);
    compile_impl!(f32_neg, F32_NEG);
    compile_impl!(f32_ceil, F32_CEIL);
    compile_impl!(f32_floor, F32_FLOOR);
    compile_impl!(f32_trunc, F32_TRUNC);
    compile_impl!(f32_nearest, F32_NEAREST);
    compile_impl!(f32_sqrt, F32_SQRT);
    compile_impl!(f32_add, F32_ADD);
    compile_impl!(f32_sub, F32_SUB);
    compile_impl!(f32_mul, F32_MUL);
    compile_impl!(f32_div, F32_DIV);
    compile_impl!(f32_min, F32_MIN);
    compile_impl!(f32_max, F32_MAX);
    compile_impl!(f32_copysign, F32_COPYSIGN);

    compile_impl!(f64_abs, F64_ABS);
    compile_impl!(f64_neg, F64_NEG);
    compile_impl!(f64_ceil, F64_CEIL);
    compile_impl!(f64_floor, F64_FLOOR);
    compile_impl!(f64_trunc, F64_TRUNC);
    compile_impl!(f64_nearest, F64_NEAREST);
    compile_impl!(f64_sqrt, F64_SQRT);
    compile_impl!(f64_add, F64_ADD);
    compile_impl!(f64_sub, F64_SUB);
    compile_impl!(f64_mul, F64_MUL);
    compile_impl!(f64_div, F64_DIV);
    compile_impl!(f64_min, F64_MIN);
    compile_impl!(f64_max, F64_MAX);
    compile_impl!(f64_copysign, F64_COPYSIGN);

    compile_impl!(i32_wrap_i64, I32_WRAP_I64);
    compile_impl!(i32_trunc_f32_s, I32_TRUNC_F32_S);
    compile_impl!(i32_trunc_f32_u, I32_TRUNC_F32_U);
    compile_impl!(i32_trunc_f64_s, I32_TRUNC_F64_S);
    compile_impl!(i32_trunc_f64_u, I32_TRUNC_F64_U);
    compile_impl!(i64_extend_i32_s, I64_EXTEND_I32_S);
    compile_impl!(i64_extend_i32_u, I64_EXTEND_I32_U);
    compile_impl!(i64_trunc_f32_s, I64_TRUNC_F32_S);
    compile_impl!(i64_trunc_f32_u, I64_TRUNC_F32_U);
    compile_impl!(i64_trunc_f64_s, I64_TRUNC_F64_S);
    compile_impl!(i64_trunc_f64_u, I64_TRUNC_F64_U);
    compile_impl!(f32_convert_i32_s, F32_CONVERT_I32_S);
    compile_impl!(f32_convert_i32_u, F32_CONVERT_I32_U);
    compile_impl!(f32_convert_i64_s, F32_CONVERT_I64_S);
    compile_impl!(f32_convert_i64_u, F32_CONVERT_I64_U);
    compile_impl!(f32_demote_f64, F32_DEMOTE_F64);
    compile_impl!(f64_convert_i32_s, F64_CONVERT_I32_S);
    compile_impl!(f64_convert_i32_u, F64_CONVERT_I32_U);
    compile_impl!(f64_convert_i64_s, F64_CONVERT_I64_S);
    compile_impl!(f64_convert_i64_u, F64_CONVERT_I64_U);
    compile_impl!(f64_promote_f32, F64_PROMOTE_F32);
    compile_impl!(i32_reinterpret_f32, I32_REINTERPRET_F32);
    compile_impl!(i64_reinterpret_f64, I64_REINTERPRET_F64);
    compile_impl!(f32_reinterpret_i32, F32_REINTERPRET_I32);
    compile_impl!(f64_reinterpret_i64, F64_REINTERPRET_I64);
}
