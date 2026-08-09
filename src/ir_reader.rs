use crate::{JumpTarget, Box, TextPage, IrVisitor, HostModuleRef, ModuleRef, TypeIdx, LocalVariable, MemArg};
use ::core::ops::AddAssign;

impl AddAssign<i32> for JumpTarget {
    fn add_assign(&mut self, rhs: i32) {
        let a = (self.0 as i32) + rhs;
        self.0 = a as u32;
    }
}

pub(crate) struct IrReader;
impl IrReader {
    fn read(code: &[Box<TextPage>], address: &mut JumpTarget) -> u16 {
        let page = address.page();
        let offset = address.offset();

        #[cfg(feature = "strict-assertions")]
        {
            if page >= code.len() || offset >= 256 {
                panic!("invalid read address");
            } else {
                *address += 1;
                code[page].0[offset]
            }
        }

        #[cfg(not(feature = "strict-assertions"))]
        {
            let v = unsafe { code.get_unchecked(page).0.get_unchecked(offset) };
            *address += 1;
            *v
        }
    }

    fn read_u32(code: &[Box<TextPage>], address: &mut JumpTarget) -> u32 {
        let w1 = IrReader::read(code, address);
        let w2 = IrReader::read(code, address);

        u32::from(w1) | (u32::from(w2) << 16)
    }

    fn read_u64(code: &[Box<TextPage>], address: &mut JumpTarget) -> u64 {
        let w1 = IrReader::read(code, address);
        let w2 = IrReader::read(code, address);
        let w3 = IrReader::read(code, address);
        let w4 = IrReader::read(code, address);

        let mut o = u64::from(w1);
        o |= u64::from(w2) << 16;
        o |= u64::from(w3) << 32;
        o |= u64::from(w4) << 48;

        o
    }

    pub fn visit_instruction<S, E, V>(
        code: &[Box<TextPage>],
        state: &mut S,
        pc: &mut JumpTarget,
        visitor: &V,
    ) -> Result<(), E>
    where
        V: IrVisitor<State = S, Error = E>,
    {
        let first = IrReader::read(code, pc);
        let opcode = (first >> 8) as u8;
        let imm = (first & 0xFF) as u8;

        macro_rules! instruction {
            // Instruction with no operands
            ($name:ident) => {{
                visitor.$name(state)?;
            }};

            // An instruction with a local variable reference immediate
            ($name:ident, local) => {{
                let frame_offset = IrReader::read(code, pc) as i16;
                visitor.$name(
                    LocalVariable {
                        frame_offset,
                        ty: imm.into(),
                    },
                    state,
                )?;
            }};

            // An instruction with a MemArg operand
            ($name:ident, MemArg) => {{
                let align = imm;
                let offset = IrReader::read_u32(code, pc);
                visitor.$name(
                    MemArg {
                        align: u32::from(align),
                        offset,
                    },
                    state,
                )?;
            }};
        }

        use crate::opcode::{UNREACHABLE, IF, BR, BR_IF, BR_TABLE, RETURN, CALL, CALL_HOST, CALL_EXTERN, CALL_INDIRECT, DROP, SELECT, LOCAL_GET, LOCAL_SET, LOCAL_TEE, GLOBAL_GET, GLOBAL_SET, GLOBAL_GET_HOST, GLOBAL_SET_HOST, GLOBAL_GET_EXTERN, GLOBAL_SET_EXTERN, I32_LOAD, I64_LOAD, F32_LOAD, F64_LOAD, I32_LOAD8_S, I32_LOAD8_U, I32_LOAD16_S, I32_LOAD16_U, I64_LOAD8_S, I64_LOAD8_U, I64_LOAD16_S, I64_LOAD16_U, I64_LOAD32_S, I64_LOAD32_U, I32_STORE, I64_STORE, F32_STORE, F64_STORE, I32_STORE8, I32_STORE16, I64_STORE8, I64_STORE16, I64_STORE32, MEMORY_SIZE, MEMORY_GROW, I32_CONST, I64_CONST, F32_CONST, F64_CONST, I32_EQZ, I32_EQ, I32_NE, I32_LT_S, I32_LT_U, I32_GT_S, I32_GT_U, I32_LE_S, I32_LE_U, I32_GE_S, I32_GE_U, I64_EQZ, I64_EQ, I64_NE, I64_LT_S, I64_LT_U, I64_GT_S, I64_GT_U, I64_LE_S, I64_LE_U, I64_GE_S, I64_GE_U, F32_EQ, F32_NE, F32_LT, F32_GT, F32_LE, F32_GE, F64_EQ, F64_NE, F64_LT, F64_GT, F64_LE, F64_GE, I32_CLZ, I32_CTZ, I32_POPCNT, I32_ADD, I32_SUB, I32_MUL, I32_DIV_S, I32_DIV_U, I32_REM_S, I32_REM_U, I32_AND, I32_OR, I32_XOR, I32_SHL, I32_SHR_S, I32_SHR_U, I32_ROTL, I32_ROTR, I64_CLZ, I64_CTZ, I64_POPCNT, I64_ADD, I64_SUB, I64_MUL, I64_DIV_S, I64_DIV_U, I64_REM_S, I64_REM_U, I64_AND, I64_OR, I64_XOR, I64_SHL, I64_SHR_S, I64_SHR_U, I64_ROTL, I64_ROTR, F32_ABS, F32_NEG, F32_CEIL, F32_FLOOR, F32_TRUNC, F32_NEAREST, F32_SQRT, F32_ADD, F32_SUB, F32_MUL, F32_DIV, F32_MIN, F32_MAX, F32_COPYSIGN, F64_ABS, F64_NEG, F64_CEIL, F64_FLOOR, F64_TRUNC, F64_NEAREST, F64_SQRT, F64_ADD, F64_SUB, F64_MUL, F64_DIV, F64_MIN, F64_MAX, F64_COPYSIGN, I32_WRAP_I64, I32_TRUNC_F32_S, I32_TRUNC_F32_U, I32_TRUNC_F64_S, I32_TRUNC_F64_U, I64_EXTEND_I32_S, I64_EXTEND_I32_U, I64_TRUNC_F32_S, I64_TRUNC_F32_U, I64_TRUNC_F64_S, I64_TRUNC_F64_U, F32_CONVERT_I32_S, F32_CONVERT_I32_U, F32_CONVERT_I64_S, F32_CONVERT_I64_U, F32_DEMOTE_F64, F64_CONVERT_I32_S, F64_CONVERT_I32_U, F64_CONVERT_I64_S, F64_CONVERT_I64_U, F64_PROMOTE_F32};
        match opcode {
            // Control instructions
            UNREACHABLE => instruction!(unreachable),

            IF => {
                let false_address = IrReader::read_u32(code, pc);
                visitor.if_(false_address.into(), state)?;
            }

            BR => {
                let address = IrReader::read_u32(code, pc);
                visitor.br(address.into(), state)?;
            }

            BR_IF => {
                let true_address = IrReader::read_u32(code, pc);
                visitor.br_if(true_address.into(), state)?;
            }

            BR_TABLE => {
                let n = if imm == 0xFF {
                    u32::from(IrReader::read(code, pc))
                } else {
                    u32::from(imm)
                };

                visitor.br_table(
                    n,
                    |case_| {
                        if case_ < n {
                            // Read & dump the cases before the selected index
                            for _ in 0..case_ {
                                IrReader::read_u32(code, pc);
                            }

                            // Read the case target
                            let lt = IrReader::read_u32(code, pc);

                            // Dump the rest of the cases
                            if case_ + 1 < n {
                                for _ in (case_ + 1)..n {
                                    IrReader::read_u32(code, pc);
                                }
                            }

                            // Dump the default case
                            IrReader::read_u32(code, pc);

                            lt.into()
                        } else {
                            // Dump all the cases and return the default
                            for _ in 0..n {
                                IrReader::read_u32(code, pc);
                            }

                            let lt = IrReader::read_u32(code, pc);
                            lt.into()
                        }
                    },
                    state,
                )?;
            }

            RETURN => {
                visitor.return_(imm, state)?;
            }
            CALL => {
                let idx = IrReader::read(code, pc);
                visitor.call(idx, state)?;
            }
            CALL_HOST => {
                let idx = IrReader::read(code, pc);
                visitor.call_host(HostModuleRef(imm), idx, state)?;
            }
            CALL_EXTERN => {
                let idx = IrReader::read(code, pc);
                visitor.call_extern(ModuleRef(imm), idx, state)?;
            }
            CALL_INDIRECT => {
                let n = IrReader::read(code, pc);
                visitor.call_indirect(TypeIdx(u32::from(n)), state)?;
            }

            // Parametric instructions
            DROP => {
                visitor.drop(imm.into(), state)?;
            }
            SELECT => {
                visitor.select(imm.into(), state)?;
            }

            // Variable instructions
            LOCAL_GET => instruction!(local_get, local),
            LOCAL_SET => instruction!(local_set, local),
            LOCAL_TEE => instruction!(local_tee, local),
            GLOBAL_GET => {
                let index = if imm == 0xFF {
                    IrReader::read(code, pc)
                } else {
                    u16::from(imm)
                };

                visitor.global_get(index, state)?;
            }
            GLOBAL_SET => {
                let index = if imm == 0xFF {
                    IrReader::read(code, pc)
                } else {
                    u16::from(imm)
                };

                visitor.global_set(index, state)?;
            }
            GLOBAL_GET_HOST => {
                let module = HostModuleRef(imm);
                let index = IrReader::read(code, pc);
                visitor.global_get_host(module, index, state)?;
            }
            GLOBAL_SET_HOST => {
                let module = HostModuleRef(imm);
                let index = IrReader::read(code, pc);
                visitor.global_set_host(module, index, state)?;
            }
            GLOBAL_GET_EXTERN => {
                let module = ModuleRef(imm);
                let index = IrReader::read(code, pc);
                visitor.global_get_extern(module, index, state)?;
            }
            GLOBAL_SET_EXTERN => {
                let module = ModuleRef(imm);
                let index = IrReader::read(code, pc);
                visitor.global_set_extern(module, index, state)?;
            }
            // Memory instructions - loads
            I32_LOAD => instruction!(i32_load, MemArg),
            I64_LOAD => instruction!(i64_load, MemArg),
            F32_LOAD => instruction!(f32_load, MemArg),
            F64_LOAD => instruction!(f64_load, MemArg),
            I32_LOAD8_S => instruction!(i32_load8_s, MemArg),
            I32_LOAD8_U => instruction!(i32_load8_u, MemArg),
            I32_LOAD16_S => instruction!(i32_load16_s, MemArg),
            I32_LOAD16_U => instruction!(i32_load16_u, MemArg),
            I64_LOAD8_S => instruction!(i64_load8_s, MemArg),
            I64_LOAD8_U => instruction!(i64_load8_u, MemArg),
            I64_LOAD16_S => instruction!(i64_load16_s, MemArg),
            I64_LOAD16_U => instruction!(i64_load16_u, MemArg),
            I64_LOAD32_S => instruction!(i64_load32_s, MemArg),
            I64_LOAD32_U => instruction!(i64_load32_u, MemArg),

            // Memory instructions - stores
            I32_STORE => instruction!(i32_store, MemArg),
            I64_STORE => instruction!(i64_store, MemArg),
            F32_STORE => instruction!(f32_store, MemArg),
            F64_STORE => instruction!(f64_store, MemArg),
            I32_STORE8 => instruction!(i32_store8, MemArg),
            I32_STORE16 => instruction!(i32_store16, MemArg),
            I64_STORE8 => instruction!(i64_store8, MemArg),
            I64_STORE16 => instruction!(i64_store16, MemArg),
            I64_STORE32 => instruction!(i64_store32, MemArg),

            // Memory instructions - size/grow
            MEMORY_SIZE => instruction!(memory_size),
            MEMORY_GROW => instruction!(memory_grow),

            // Numeric instructions - const
            I32_CONST => {
                let n = if imm == 0xFF {
                    IrReader::read_u32(code, pc)
                } else {
                    u32::from(imm)
                };
                visitor.i32_const(n as i32, state)?;
            }
            I64_CONST => {
                let n = if imm == 0xFF {
                    IrReader::read_u64(code, pc)
                } else {
                    u64::from(imm)
                };
                visitor.i64_const(n as i64, state)?;
            }
            F32_CONST => {
                let z = IrReader::read_u32(code, pc);
                visitor.f32_const(f32::from_bits(z), state)?;
            }
            F64_CONST => {
                let z = IrReader::read_u64(code, pc);
                visitor.f64_const(f64::from_bits(z), state)?;
            }

            // Numeric instructions - i32 test/rel
            I32_EQZ => instruction!(i32_eqz),
            I32_EQ => instruction!(i32_eq),
            I32_NE => instruction!(i32_ne),
            I32_LT_S => instruction!(i32_lt_s),
            I32_LT_U => instruction!(i32_lt_u),
            I32_GT_S => instruction!(i32_gt_s),
            I32_GT_U => instruction!(i32_gt_u),
            I32_LE_S => instruction!(i32_le_s),
            I32_LE_U => instruction!(i32_le_u),
            I32_GE_S => instruction!(i32_ge_s),
            I32_GE_U => instruction!(i32_ge_u),

            // Numeric instructions - i64 test/rel
            I64_EQZ => instruction!(i64_eqz),
            I64_EQ => instruction!(i64_eq),
            I64_NE => instruction!(i64_ne),
            I64_LT_S => instruction!(i64_lt_s),
            I64_LT_U => instruction!(i64_lt_u),
            I64_GT_S => instruction!(i64_gt_s),
            I64_GT_U => instruction!(i64_gt_u),
            I64_LE_S => instruction!(i64_le_s),
            I64_LE_U => instruction!(i64_le_u),
            I64_GE_S => instruction!(i64_ge_s),
            I64_GE_U => instruction!(i64_ge_u),

            // Numeric instructions - f32 rel
            F32_EQ => instruction!(f32_eq),
            F32_NE => instruction!(f32_ne),
            F32_LT => instruction!(f32_lt),
            F32_GT => instruction!(f32_gt),
            F32_LE => instruction!(f32_le),
            F32_GE => instruction!(f32_ge),

            // Numeric instructions - f64 rel
            F64_EQ => instruction!(f64_eq),
            F64_NE => instruction!(f64_ne),
            F64_LT => instruction!(f64_lt),
            F64_GT => instruction!(f64_gt),
            F64_LE => instruction!(f64_le),
            F64_GE => instruction!(f64_ge),

            // Numeric instructions - i32 unary/binary
            I32_CLZ => instruction!(i32_clz),
            I32_CTZ => instruction!(i32_ctz),
            I32_POPCNT => instruction!(i32_popcnt),
            I32_ADD => instruction!(i32_add),
            I32_SUB => instruction!(i32_sub),
            I32_MUL => instruction!(i32_mul),
            I32_DIV_S => instruction!(i32_div_s),
            I32_DIV_U => instruction!(i32_div_u),
            I32_REM_S => instruction!(i32_rem_s),
            I32_REM_U => instruction!(i32_rem_u),
            I32_AND => instruction!(i32_and),
            I32_OR => instruction!(i32_or),
            I32_XOR => instruction!(i32_xor),
            I32_SHL => instruction!(i32_shl),
            I32_SHR_S => instruction!(i32_shr_s),
            I32_SHR_U => instruction!(i32_shr_u),
            I32_ROTL => instruction!(i32_rotl),
            I32_ROTR => instruction!(i32_rotr),

            // Numeric instructions - i64 unary/binary
            I64_CLZ => instruction!(i64_clz),
            I64_CTZ => instruction!(i64_ctz),
            I64_POPCNT => instruction!(i64_popcnt),
            I64_ADD => instruction!(i64_add),
            I64_SUB => instruction!(i64_sub),
            I64_MUL => instruction!(i64_mul),
            I64_DIV_S => instruction!(i64_div_s),
            I64_DIV_U => instruction!(i64_div_u),
            I64_REM_S => instruction!(i64_rem_s),
            I64_REM_U => instruction!(i64_rem_u),
            I64_AND => instruction!(i64_and),
            I64_OR => instruction!(i64_or),
            I64_XOR => instruction!(i64_xor),
            I64_SHL => instruction!(i64_shl),
            I64_SHR_S => instruction!(i64_shr_s),
            I64_SHR_U => instruction!(i64_shr_u),
            I64_ROTL => instruction!(i64_rotl),
            I64_ROTR => instruction!(i64_rotr),

            // Numeric instructions - f32 unary/binary
            F32_ABS => instruction!(f32_abs),
            F32_NEG => instruction!(f32_neg),
            F32_CEIL => instruction!(f32_ceil),
            F32_FLOOR => instruction!(f32_floor),
            F32_TRUNC => instruction!(f32_trunc),
            F32_NEAREST => instruction!(f32_nearest),
            F32_SQRT => instruction!(f32_sqrt),
            F32_ADD => instruction!(f32_add),
            F32_SUB => instruction!(f32_sub),
            F32_MUL => instruction!(f32_mul),
            F32_DIV => instruction!(f32_div),
            F32_MIN => instruction!(f32_min),
            F32_MAX => instruction!(f32_max),
            F32_COPYSIGN => instruction!(f32_copysign),

            // Numeric instructions - f64 unary/binary
            F64_ABS => instruction!(f64_abs),
            F64_NEG => instruction!(f64_neg),
            F64_CEIL => instruction!(f64_ceil),
            F64_FLOOR => instruction!(f64_floor),
            F64_TRUNC => instruction!(f64_trunc),
            F64_NEAREST => instruction!(f64_nearest),
            F64_SQRT => instruction!(f64_sqrt),
            F64_ADD => instruction!(f64_add),
            F64_SUB => instruction!(f64_sub),
            F64_MUL => instruction!(f64_mul),
            F64_DIV => instruction!(f64_div),
            F64_MIN => instruction!(f64_min),
            F64_MAX => instruction!(f64_max),
            F64_COPYSIGN => instruction!(f64_copysign),

            // Numeric instructions - conversions
            I32_WRAP_I64 => instruction!(i32_wrap_i64),
            I32_TRUNC_F32_S => instruction!(i32_trunc_f32_s),
            I32_TRUNC_F32_U => instruction!(i32_trunc_f32_u),
            I32_TRUNC_F64_S => instruction!(i32_trunc_f64_s),
            I32_TRUNC_F64_U => instruction!(i32_trunc_f64_u),
            I64_EXTEND_I32_S => instruction!(i64_extend_i32_s),
            I64_EXTEND_I32_U => instruction!(i64_extend_i32_u),
            I64_TRUNC_F32_S => instruction!(i64_trunc_f32_s),
            I64_TRUNC_F32_U => instruction!(i64_trunc_f32_u),
            I64_TRUNC_F64_S => instruction!(i64_trunc_f64_s),
            I64_TRUNC_F64_U => instruction!(i64_trunc_f64_u),
            F32_CONVERT_I32_S => instruction!(f32_convert_i32_s),
            F32_CONVERT_I32_U => instruction!(f32_convert_i32_u),
            F32_CONVERT_I64_S => instruction!(f32_convert_i64_s),
            F32_CONVERT_I64_U => instruction!(f32_convert_i64_u),
            F32_DEMOTE_F64 => instruction!(f32_demote_f64),
            F64_CONVERT_I32_S => instruction!(f64_convert_i32_s),
            F64_CONVERT_I32_U => instruction!(f64_convert_i32_u),
            F64_CONVERT_I64_S => instruction!(f64_convert_i64_s),
            F64_CONVERT_I64_U => instruction!(f64_convert_i64_u),
            F64_PROMOTE_F32 => instruction!(f64_promote_f32),
            _ => panic!("invalid opcode {opcode:?}"),
        }

        Ok(())
    }
}
