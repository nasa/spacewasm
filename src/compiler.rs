use crate::*;
use ::core::marker::PhantomData;

#[derive(Debug, Default, Copy, Clone)]
pub struct CompilerOptions {
    /// Allow compiling memory.grow instructions into IR
    pub allow_memory_grow: bool,
}

pub struct Compiler<
    'a,
    const MAX_PAGES: usize,
    const MAX_CONTROL_FRAMES: usize,
    const MAX_STACK_DEPTH: usize,
> {
    _marker: PhantomData<&'a ()>,
    options: CompilerOptions,
}

impl<'a, const MAX_PAGES: usize, const MAX_CONTROL_FRAMES: usize, const MAX_STACK_DEPTH: usize>
    Compiler<'a, MAX_PAGES, MAX_CONTROL_FRAMES, MAX_STACK_DEPTH>
{
    pub fn new(
        options: CompilerOptions,
    ) -> Compiler<'a, MAX_PAGES, MAX_CONTROL_FRAMES, MAX_STACK_DEPTH> {
        Compiler {
            _marker: Default::default(),
            options,
        }
    }
}

macro_rules! validate {
    ($state:expr, ($($in_ty:ident)*) -> ($($out_ty:ident)*)) => {
        $(
            let _ = $state.pop_stack(ValType::$in_ty)?;
        )*

        $(
            $state.push_stack(ValType::$out_ty)?;
        )*
    };
}

macro_rules! alignment {
    (8) => {
        0
    };
    (16) => {
        1
    };
    (32) => {
        2
    };
    (64) => {
        3
    };
}

macro_rules! instruction {
    // No additional operands
    ($name:ident, $opcode:expr, ($($in_ty:ident)*) -> ($($out_ty:ident)*)) => {
        fn $name(&self, state: &mut Self::State) -> Result<(), Self::Error> {
            validate!(state, ($($in_ty)*) -> ($($out_ty)*));
            state.instr($opcode)?;
            Ok(())
        }
    };

    // An instruction with a MemArg operand
    ($name:ident, $opcode:expr, mem, $ty_align:tt, ($($in_ty:ident)*) -> ($($out_ty:ident)*)) => {
        fn $name(&self, m: MemArg, state: &mut Self::State) -> Result<(), Self::Error> {
            state.module().check_memory_defined()?;
            validate!(state, ($($in_ty)*) -> ($($out_ty)*));
            if m.align > alignment!($ty_align) {
                return Err(ValidationError::AlignmentLargerThanType);
            }

            state.instr_mem($opcode, m)?;
            Ok(())
        }
    };
}

impl<'a, const MAX_PAGES: usize, const MAX_CONTROL_FRAMES: usize, const MAX_STACK_DEPTH: usize>
    WasmVisitor for Compiler<'a, MAX_PAGES, MAX_CONTROL_FRAMES, MAX_STACK_DEPTH>
{
    fn drop(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let ty = state.pop_stack_t()?;
        state.instr_imm_8(
            DROP,
            match ty {
                OperandType::Unknown => ValType::I32,
                OperandType::Known(t) => t,
            } as u8,
        )?;
        Ok(())
    }

    fn select(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let _ = state.pop_stack(ValType::I32)?;
        let t1 = state.pop_stack_t()?;
        let t2 = state.pop_stack(t1)?;
        state.push_stack(t2)?;
        state.instr_imm_8(SELECT, t2.into())?;
        Ok(())
    }

    fn enter_block(
        &self,
        block_type: ResultType,
        state: &mut Self::State,
    ) -> Result<(), Self::Error> {
        state.push_control(BlockKind::Block, block_type, block_type)
    }

    fn exit_block(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        let results = state.pop_control()?;
        state.push_result_type(results)?;
        Ok(())
    }

    fn finish(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        // Pop the implicit function control frame and validate
        // This validates that the function returns the correct type
        let ty = state.pop_control()?;

        // Emit a return instruction
        if let Some(ty) = ty.0 {
            state.instr_imm_8(RETURN, ty.size() as u8 / 4)?;
        } else {
            state.instr(RETURN)?;
        }

        Ok(())
    }

    fn loop_(&self, block_type: ResultType, state: &mut Self::State) -> Result<(), Self::Error> {
        state.push_control(BlockKind::Loop, ResultType(None), block_type)?;
        // Set the loop's target to the current PC (loop start)
        state.set_control_target(state.pc())?;
        Ok(())
    }

    fn if_(&self, block_type: ResultType, state: &mut Self::State) -> Result<(), Self::Error> {
        let _ = state.pop_stack(ValType::I32)?;
        state.instr(IF)?;
        state.push_control(BlockKind::If, block_type, block_type)?;
        // Emit a placeholder for the false-branch/else target that will be backpatched
        state.write_if_else_target()?;
        Ok(())
    }

    fn else_(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        // Emit an unconditional branch to skip the else block
        // This gets added to the if frame's target list (before we pop it)
        state.instr(BR)?;
        state.write_label_target(LabelIdx(0))?;

        // Pop the if control frame and patch its false-branch (tail of linked list) to point here
        // Returns the remaining br chain (including the BR we just emitted)
        let (results, br_chain) = state.pop_control_and_patch_if()?;

        // Push a new control frame for the else block, inheriting the br chain
        state.push_control(BlockKind::Block, results, results)?;
        state.set_control_target(br_chain)?;

        Ok(())
    }

    fn br(&self, l: LabelIdx, state: &mut Self::State) -> Result<(), Self::Error> {
        state.instr(BR)?;
        let lbl_types = state.write_label_target(l)?;
        state.pop_result_type(lbl_types)?;
        state.mark_unreachable();
        Ok(())
    }

    fn br_if(&self, l: LabelIdx, state: &mut Self::State) -> Result<(), Self::Error> {
        let _ = state.pop_stack(ValType::I32)?;
        state.instr(BR_IF)?;
        let lbl_types = state.write_label_target(l)?;
        state.pop_result_type(lbl_types)?;
        state.push_result_type(lbl_types)?;
        Ok(())
    }

    fn br_table_start(&self, len: u32, state: &mut Self::State) -> Result<(), Self::Error> {
        let _ = state.pop_stack(ValType::I32)?;
        state.instr_imm_8_or_16(BR_TABLE, len)?;
        Ok(())
    }

    fn br_table_branch(&self, br: LabelIdx, state: &mut Self::State) -> Result<(), Self::Error> {
        let case_return = state.write_label_target(br)?;
        state.check_br_table_result(case_return)?;
        Ok(())
    }

    fn br_table_finish(
        &self,
        default_: LabelIdx,
        state: &mut Self::State,
    ) -> Result<(), Self::Error> {
        let def_result = state.write_label_target(default_)?;
        state.check_and_clear_br_table_result(def_result)?;
        state.pop_result_type(def_result)?;
        state.mark_unreachable();
        Ok(())
    }

    fn return_(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        // Return instructions also encode the return size from their function's context
        let ty = state.func().return_ty;
        if let Some(ty) = ty {
            let _ = state.pop_stack(ty)?;
            state.instr_imm_8(RETURN, ty.size() as u8 / 4)?;
        } else {
            state.instr(RETURN)?;
        }

        state.mark_unreachable();
        Ok(())
    }

    fn call(&self, x: FuncIdx, state: &mut Self::State) -> Result<(), Self::Error> {
        let f_ref = state
            .module()
            .get_func_ref(x)
            .ok_or(ValidationError::FunctionIdxOutOfRange)?;

        match f_ref {
            Ref::Module(index) => {
                // Check the call signature
                let f = &state.module().functions[index as usize];
                let ty = &state.module().types[f.ty.0 as usize];

                for p in ty.params.iter().rev() {
                    let _ = state.pop_stack(*p)?;
                }

                for r in ty.returns.iter().rev() {
                    state.push_stack(r)?;
                }

                state.instr_imm_8(CALL, 0)?;
                state.write_16(index)?;
                Ok(())
            }
            Ref::Host { module, index } => {
                let hm = &state.store().host_modules()[module.0 as usize];
                let f = &hm.functions[index as usize];
                for p in f.params().iter().rev() {
                    let _ = state.pop_stack(p)?;
                }

                for r in f.returns().iter().rev() {
                    state.push_stack(r)?;
                }

                state.instr_imm_8(CALL_HOST, module.0)?;
                state.write_16(index)?;
                Ok(())
            }
            Ref::Extern { module, index } => {
                // Check the call signature
                let m = &state.store().modules()[module.0 as usize];
                let f = &m.functions[index as usize];
                let ty = &m.types[f.ty.0 as usize];

                for p in ty.params.iter().rev() {
                    let _ = state.pop_stack(p)?;
                }

                for r in ty.returns.iter().rev() {
                    state.push_stack(*r)?;
                }

                state.instr_imm_8(CALL_EXTERN, module.0)?;
                state.write_16(index)?;
                Ok(())
            }
        }
    }

    fn call_indirect(&self, x: TypeIdx, state: &mut Self::State) -> Result<(), Self::Error> {
        if state.module().table.is_none() {
            return Err(ValidationError::TableNotDefined);
        }

        let _ = state.pop_stack(ValType::I32)?;
        let ty = state
            .module()
            .types
            .get(x.0 as usize)
            .ok_or(ValidationError::TypeIdxOutOfRange)?;

        for p in ty.params.iter().rev() {
            let _ = state.pop_stack(p)?;
        }

        for r in &ty.returns {
            state.push_stack(*r)?;
        }

        state.instr(CALL_INDIRECT)?;
        state.write_16(x.0 as u16)?;
        Ok(())
    }

    fn local_get(&self, x: LocalIdx, state: &mut Self::State) -> Result<(), Self::Error> {
        let l = state.get_local(x)?;
        state.push_stack(l.ty)?;
        state.instr_imm_8(LOCAL_GET, l.ty as u8)?;
        state.write_16(l.frame_offset as u16)?;
        Ok(())
    }

    fn local_set(&self, x: LocalIdx, state: &mut Self::State) -> Result<(), Self::Error> {
        let l = state.get_local(x)?;
        let _ = state.pop_stack(l.ty)?;
        state.instr_imm_8(LOCAL_SET, l.ty as u8)?;
        state.write_16(l.frame_offset as u16)?;
        Ok(())
    }

    fn local_tee(&self, x: LocalIdx, state: &mut Self::State) -> Result<(), Self::Error> {
        let l = state.get_local(x)?;
        let _ = state.pop_stack(l.ty)?;
        state.push_stack(l.ty)?;
        state.instr_imm_8(LOCAL_TEE, l.ty as u8)?;
        state.write_16(l.frame_offset as u16)?;
        Ok(())
    }

    fn global_get(&self, x: GlobalIdx, state: &mut Self::State) -> Result<(), Self::Error> {
        let g = state.get_global(x)?;
        state.push_stack(g.ty)?;
        match g.reference {
            Ref::Module(idx) => {
                state.instr_imm_8_or_16(GLOBAL_GET, idx as u32)?;
                Ok(())
            }
            Ref::Extern { module, index } => {
                state.instr_imm_8(GLOBAL_GET_EXTERN, module.0)?;
                state.write_16(index)?;
                Ok(())
            }
            Ref::Host { module, index } => {
                state.instr_imm_8(GLOBAL_GET_HOST, module.0)?;
                state.write_16(index)?;
                Ok(())
            }
        }
    }

    fn global_set(&self, x: GlobalIdx, state: &mut Self::State) -> Result<(), Self::Error> {
        let g = state.get_global(x)?;
        if !g.mutable {
            Err(ValidationError::GlobalIsNotMutable)
        } else {
            let _ = state.pop_stack(g.ty)?;
            match g.reference {
                Ref::Module(idx) => {
                    state.instr_imm_8_or_16(GLOBAL_SET, idx as u32)?;
                    Ok(())
                }
                Ref::Extern { module, index } => {
                    state.instr_imm_8(GLOBAL_SET_EXTERN, module.0)?;
                    state.write_16(index)?;
                    Ok(())
                }
                Ref::Host { module, index } => {
                    state.instr_imm_8(GLOBAL_SET_HOST, module.0)?;
                    state.write_16(index)?;
                    Ok(())
                }
            }
        }
    }

    fn i32_reinterpret_f32(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        validate!(state, (F32) -> (I32));
        Ok(())
    }

    fn f64_reinterpret_i64(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        validate!(state, (I64) -> (F64));
        Ok(())
    }

    fn f32_reinterpret_i32(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        validate!(state, (I32) -> (F32));
        Ok(())
    }

    fn i64_reinterpret_f64(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        validate!(state, (F64) -> (I64));
        Ok(())
    }
}

impl<'a, const MAX_PAGES: usize, const MAX_CONTROL_FRAMES: usize, const MAX_STACK_DEPTH: usize>
    BaseVisitor for Compiler<'a, MAX_PAGES, MAX_CONTROL_FRAMES, MAX_STACK_DEPTH>
{
    type Error = ValidationError;
    type State = TextBuilder<'a, MAX_PAGES, MAX_CONTROL_FRAMES, MAX_STACK_DEPTH>;

    fn unreachable(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        state.instr_imm_8(UNREACHABLE, 0)?;
        state.mark_unreachable();
        Ok(())
    }

    fn nop(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        // No op!
        let _ = state;
        Ok(())
    }

    instruction!(i32_load, I32_LOAD, mem, 32, (I32) -> (I32));
    instruction!(i64_load, I64_LOAD, mem, 64, (I32) -> (I64));
    instruction!(f32_load, F32_LOAD, mem, 32, (I32) -> (F32));
    instruction!(f64_load, F64_LOAD, mem, 64, (I32) -> (F64));
    instruction!(i32_load8_s, I32_LOAD8_S, mem, 8, (I32) -> (I32));
    instruction!(i32_load8_u, I32_LOAD8_U, mem, 8, (I32) -> (I32));
    instruction!(i32_load16_s, I32_LOAD16_S, mem, 16, (I32) -> (I32));
    instruction!(i32_load16_u, I32_LOAD16_U, mem, 16, (I32) -> (I32));
    instruction!(i64_load8_s, I64_LOAD8_S, mem, 8, (I32) -> (I64));
    instruction!(i64_load8_u, I64_LOAD8_U, mem, 8, (I32) -> (I64));
    instruction!(i64_load16_s, I64_LOAD16_S, mem, 16, (I32) -> (I64));
    instruction!(i64_load16_u, I64_LOAD16_U, mem, 16, (I32) -> (I64));
    instruction!(i64_load32_s, I64_LOAD32_S, mem, 32, (I32) -> (I64));
    instruction!(i64_load32_u, I64_LOAD32_U, mem, 32, (I32) -> (I64));

    instruction!(i32_store, I32_STORE, mem, 32, (I32 I32) -> ());
    instruction!(i64_store, I64_STORE, mem, 64, (I64 I32) -> ());
    instruction!(f32_store, F32_STORE, mem, 32, (F32 I32) -> ());
    instruction!(f64_store, F64_STORE, mem, 64, (F64 I32) -> ());
    instruction!(i32_store8, I32_STORE8, mem, 8, (I32 I32) -> ());
    instruction!(i32_store16, I32_STORE16, mem, 16, (I32 I32) -> ());
    instruction!(i64_store8, I64_STORE8, mem, 8, (I64 I32) -> ());
    instruction!(i64_store16, I64_STORE16, mem, 16, (I64 I32) -> ());
    instruction!(i64_store32, I64_STORE32, mem, 32, (I64 I32) -> ());

    fn memory_size(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        state.module().check_memory_defined()?;
        validate!(state, () -> (I32));
        state.instr(MEMORY_SIZE)?;
        Ok(())
    }

    fn memory_grow(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        state.module().check_memory_defined()?;
        validate!(state, (I32) -> (I32));
        if self.options.allow_memory_grow {
            state.instr(MEMORY_GROW)?;
            Ok(())
        } else {
            Err(ValidationError::IllegalMemoryGrow)
        }
    }

    fn i32_const(&self, n: i32, state: &mut Self::State) -> Result<(), Self::Error> {
        validate!(state, () -> (I32));
        state.instr_imm_8_or_32(I32_CONST, n as u32)?;
        Ok(())
    }

    fn i64_const(&self, n: i64, state: &mut Self::State) -> Result<(), Self::Error> {
        validate!(state, () -> (I64));
        state.instr_imm_8_or_64(I64_CONST, n as u64)?;
        Ok(())
    }

    fn f32_const(&self, z: f32, state: &mut Self::State) -> Result<(), Self::Error> {
        validate!(state, () -> (F32));
        state.instr(F32_CONST)?;
        state.write_32(z.to_bits())?;
        Ok(())
    }

    fn f64_const(&self, z: f64, state: &mut Self::State) -> Result<(), Self::Error> {
        validate!(state, () -> (F64));
        state.instr(F64_CONST)?;
        state.write_64(z.to_bits())?;
        Ok(())
    }

    instruction!(i32_eqz, I32_EQZ, (I32) -> (I32));
    instruction!(i32_eq, I32_EQ, (I32 I32) -> (I32));
    instruction!(i32_ne, I32_NE, (I32 I32) -> (I32));
    instruction!(i32_lt_s, I32_LT_S, (I32 I32) -> (I32));
    instruction!(i32_lt_u, I32_LT_U, (I32 I32) -> (I32));
    instruction!(i32_gt_s, I32_GT_S, (I32 I32) -> (I32));
    instruction!(i32_gt_u, I32_GT_U, (I32 I32) -> (I32));
    instruction!(i32_le_s, I32_LE_S, (I32 I32) -> (I32));
    instruction!(i32_le_u, I32_LE_U, (I32 I32) -> (I32));
    instruction!(i32_ge_s, I32_GE_S, (I32 I32) -> (I32));
    instruction!(i32_ge_u, I32_GE_U, (I32 I32) -> (I32));

    instruction!(i64_eqz, I64_EQZ, (I64) -> (I32));
    instruction!(i64_eq, I64_EQ, (I64 I64) -> (I32));
    instruction!(i64_ne, I64_NE, (I64 I64) -> (I32));
    instruction!(i64_lt_s, I64_LT_S, (I64 I64) -> (I32));
    instruction!(i64_lt_u, I64_LT_U, (I64 I64) -> (I32));
    instruction!(i64_gt_s, I64_GT_S, (I64 I64) -> (I32));
    instruction!(i64_gt_u, I64_GT_U, (I64 I64) -> (I32));
    instruction!(i64_le_s, I64_LE_S, (I64 I64) -> (I32));
    instruction!(i64_le_u, I64_LE_U, (I64 I64) -> (I32));
    instruction!(i64_ge_s, I64_GE_S, (I64 I64) -> (I32));
    instruction!(i64_ge_u, I64_GE_U, (I64 I64) -> (I32));

    instruction!(f32_eq, F32_EQ, (F32 F32) -> (I32));
    instruction!(f32_ne, F32_NE, (F32 F32) -> (I32));
    instruction!(f32_lt, F32_LT, (F32 F32) -> (I32));
    instruction!(f32_gt, F32_GT, (F32 F32) -> (I32));
    instruction!(f32_le, F32_LE, (F32 F32) -> (I32));
    instruction!(f32_ge, F32_GE, (F32 F32) -> (I32));

    instruction!(f64_eq, F64_EQ, (F64 F64) -> (I32));
    instruction!(f64_ne, F64_NE, (F64 F64) -> (I32));
    instruction!(f64_lt, F64_LT, (F64 F64) -> (I32));
    instruction!(f64_gt, F64_GT, (F64 F64) -> (I32));
    instruction!(f64_le, F64_LE, (F64 F64) -> (I32));
    instruction!(f64_ge, F64_GE, (F64 F64) -> (I32));

    instruction!(i32_clz, I32_CLZ, (I32) -> (I32));
    instruction!(i32_ctz, I32_CTZ, (I32) -> (I32));
    instruction!(i32_popcnt, I32_POPCNT, (I32) -> (I32));
    instruction!(i32_add, I32_ADD, (I32 I32) -> (I32));
    instruction!(i32_sub, I32_SUB, (I32 I32) -> (I32));
    instruction!(i32_mul, I32_MUL, (I32 I32) -> (I32));
    instruction!(i32_div_s, I32_DIV_S, (I32 I32) -> (I32));
    instruction!(i32_div_u, I32_DIV_U, (I32 I32) -> (I32));
    instruction!(i32_rem_s, I32_REM_S, (I32 I32) -> (I32));
    instruction!(i32_rem_u, I32_REM_U, (I32 I32) -> (I32));
    instruction!(i32_and, I32_AND, (I32 I32) -> (I32));
    instruction!(i32_or, I32_OR, (I32 I32) -> (I32));
    instruction!(i32_xor, I32_XOR, (I32 I32) -> (I32));
    instruction!(i32_shl, I32_SHL, (I32 I32) -> (I32));
    instruction!(i32_shr_s, I32_SHR_S, (I32 I32) -> (I32));
    instruction!(i32_shr_u, I32_SHR_U, (I32 I32) -> (I32));
    instruction!(i32_rotl, I32_ROTL, (I32 I32) -> (I32));
    instruction!(i32_rotr, I32_ROTR, (I32 I32) -> (I32));

    instruction!(i64_clz, I64_CLZ, (I64) -> (I64));
    instruction!(i64_ctz, I64_CTZ, (I64) -> (I64));
    instruction!(i64_popcnt, I64_POPCNT, (I64) -> (I64));
    instruction!(i64_add, I64_ADD, (I64 I64) -> (I64));
    instruction!(i64_sub, I64_SUB, (I64 I64) -> (I64));
    instruction!(i64_mul, I64_MUL, (I64 I64) -> (I64));
    instruction!(i64_div_s, I64_DIV_S, (I64 I64) -> (I64));
    instruction!(i64_div_u, I64_DIV_U, (I64 I64) -> (I64));
    instruction!(i64_rem_s, I64_REM_S, (I64 I64) -> (I64));
    instruction!(i64_rem_u, I64_REM_U, (I64 I64) -> (I64));
    instruction!(i64_and, I64_AND, (I64 I64) -> (I64));
    instruction!(i64_or, I64_OR, (I64 I64) -> (I64));
    instruction!(i64_xor, I64_XOR, (I64 I64) -> (I64));
    instruction!(i64_shl, I64_SHL, (I64 I64) -> (I64));
    instruction!(i64_shr_s, I64_SHR_S, (I64 I64) -> (I64));
    instruction!(i64_shr_u, I64_SHR_U, (I64 I64) -> (I64));
    instruction!(i64_rotl, I64_ROTL, (I64 I64) -> (I64));
    instruction!(i64_rotr, I64_ROTR, (I64 I64) -> (I64));

    instruction!(f32_abs, F32_ABS, (F32) -> (F32));
    instruction!(f32_neg, F32_NEG, (F32) -> (F32));
    instruction!(f32_ceil, F32_CEIL, (F32) -> (F32));
    instruction!(f32_floor, F32_FLOOR, (F32) -> (F32));
    instruction!(f32_trunc, F32_TRUNC, (F32) -> (F32));
    instruction!(f32_nearest, F32_NEAREST, (F32) -> (F32));
    instruction!(f32_sqrt, F32_SQRT, (F32) -> (F32));
    instruction!(f32_add, F32_ADD, (F32 F32) -> (F32));
    instruction!(f32_sub, F32_SUB, (F32 F32) -> (F32));
    instruction!(f32_mul, F32_MUL, (F32 F32) -> (F32));
    instruction!(f32_div, F32_DIV, (F32 F32) -> (F32));
    instruction!(f32_min, F32_MIN, (F32 F32) -> (F32));
    instruction!(f32_max, F32_MAX, (F32 F32) -> (F32));
    instruction!(f32_copysign, F32_COPYSIGN, (F32 F32) -> (F32));

    instruction!(f64_abs, F64_ABS, (F64) -> (F64));
    instruction!(f64_neg, F64_NEG, (F64) -> (F64));
    instruction!(f64_ceil, F64_CEIL, (F64) -> (F64));
    instruction!(f64_floor, F64_FLOOR, (F64) -> (F64));
    instruction!(f64_trunc, F64_TRUNC, (F64) -> (F64));
    instruction!(f64_nearest, F64_NEAREST, (F64) -> (F64));
    instruction!(f64_sqrt, F64_SQRT, (F64) -> (F64));
    instruction!(f64_add, F64_ADD, (F64 F64) -> (F64));
    instruction!(f64_sub, F64_SUB, (F64 F64) -> (F64));
    instruction!(f64_mul, F64_MUL, (F64 F64) -> (F64));
    instruction!(f64_div, F64_DIV, (F64 F64) -> (F64));
    instruction!(f64_min, F64_MIN, (F64 F64) -> (F64));
    instruction!(f64_max, F64_MAX, (F64 F64) -> (F64));
    instruction!(f64_copysign, F64_COPYSIGN, (F64 F64) -> (F64));

    instruction!(i32_wrap_i64, I32_WRAP_I64, (I64) -> (I32));
    instruction!(i32_trunc_f32_s, I32_TRUNC_F32_S, (F32) -> (I32));
    instruction!(i32_trunc_f32_u, I32_TRUNC_F32_U, (F32) -> (I32));
    instruction!(i32_trunc_f64_s, I32_TRUNC_F64_S, (F64) -> (I32));
    instruction!(i32_trunc_f64_u, I32_TRUNC_F64_U, (F64) -> (I32));
    instruction!(i64_extend_i32_s, I64_EXTEND_I32_S, (I32) -> (I64));
    instruction!(i64_extend_i32_u, I64_EXTEND_I32_U, (I32) -> (I64));
    instruction!(i64_trunc_f32_s, I64_TRUNC_F32_S, (F32) -> (I64));
    instruction!(i64_trunc_f32_u, I64_TRUNC_F32_U, (F32) -> (I64));
    instruction!(i64_trunc_f64_s, I64_TRUNC_F64_S, (F64) -> (I64));
    instruction!(i64_trunc_f64_u, I64_TRUNC_F64_U, (F64) -> (I64));
    instruction!(f32_convert_i32_s, F32_CONVERT_I32_S, (I32) -> (F32));
    instruction!(f32_convert_i32_u, F32_CONVERT_I32_U, (I32) -> (F32));
    instruction!(f32_convert_i64_s, F32_CONVERT_I64_S, (I64) -> (F32));
    instruction!(f32_convert_i64_u, F32_CONVERT_I64_U, (I64) -> (F32));
    instruction!(f32_demote_f64, F32_DEMOTE_F64, (F64) -> (F32));
    instruction!(f64_convert_i32_s, F64_CONVERT_I32_S, (I32) -> (F64));
    instruction!(f64_convert_i32_u, F64_CONVERT_I32_U, (I32) -> (F64));
    instruction!(f64_convert_i64_s, F64_CONVERT_I64_S, (I64) -> (F64));
    instruction!(f64_convert_i64_u, F64_CONVERT_I64_U, (I64) -> (F64));
    instruction!(f64_promote_f32, F64_PROMOTE_F32, (F32) -> (F64));
}
