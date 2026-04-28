use crate::*;

pub enum IrReaderError {
    InvalidAddress,
    InvalidOpcode(u8),
    InvalidType,
}

pub struct Code<'wasm>(&'wasm Vec<Box<TextPage>>);

impl<'wasm> Code<'wasm> {
    fn read(&self, address: JumpTarget) -> Result<u16, IrReaderError> {
        let page = address.page();
        let offset = address.offset();
        if page >= self.0.len() || offset >= 256 {
            Err(IrReaderError::InvalidAddress)
        } else {
            Ok(self.0[page].0[offset])
        }
    }

    fn read_u32(&self, address: JumpTarget) -> Result<u32, IrReaderError> {
        let w1 = self.read(address)?;
        let w2 = self.read(address + 1)?;

        Ok((w1 as u32) | ((w2 as u32) << 16))
    }

    fn read_u64(&self, address: JumpTarget) -> Result<u64, IrReaderError> {
        let w1 = self.read(address)?;
        let w2 = self.read(address + 1)?;
        let w3 = self.read(address + 2)?;
        let w4 = self.read(address + 3)?;

        let mut o = w1 as u64;
        o |= (w2 as u64) << 16;
        o |= (w3 as u64) << 32;
        o |= (w4 as u64) << 48;

        Ok(o)
    }

    pub fn visit_instruction<S, E, V>(
        &self,
        state: &mut S,
        pc: JumpTarget,
        visitor: V,
    ) -> Result<u32, IrReaderError>
    where
        V: IrVisitor<State = S, Error = E>,
        IrReaderError: From<E>,
    {
        let first = self.read(pc)?;
        let opcode = ((first >> 8) & 0xFF) as u8;

        let imm = (first & 0xFF) as u8;

        macro_rules! instruction {
            // Instruction with no operands
            ($name:ident) => {{
                visitor.$name(state)?;
                Ok(1)
            }};

            // An instruction with a local variable reference immediate
            ($name:ident, local) => {{
                let ty = match imm {
                    0 => ValType::I32,
                    1 => ValType::I64,
                    2 => ValType::F32,
                    3 => ValType::F64,
                    _ => return Err(IrReaderError::InvalidType),
                };

                let frame_offset = self.read(pc + 1)? as i32;

                visitor.$name(LocalVariable { frame_offset, ty }, state)?;
                Ok(2)
            }};

            // An instruction with a global variable reference immediate
            ($name:ident, global) => {{
                let ty = match (imm & 0x0F) {
                    0 => ValType::I32,
                    1 => ValType::I64,
                    2 => ValType::F32,
                    3 => ValType::F64,
                    _ => return Err(IrReaderError::InvalidType),
                };

                let is_imported = (imm & 0xF0) != 0;
                let index = self.read(pc + 1)? as u32;

                visitor.$name(
                    GlobalVariable {
                        reference: if is_imported {
                            GlobalVariableRef::Imported(index)
                        } else {
                            GlobalVariableRef::Internal(index)
                        },
                        ty,
                        mutable: true,
                    },
                    state,
                )?;

                Ok(2)
            }};

            // An instruction with a MemArg operand
            ($name:ident, MemArg) => {{
                let align = imm;
                let offset = self.read_u32(pc + 1)?;
                visitor.$name(
                    MemArg {
                        align: align as u32,
                        offset,
                    },
                    state,
                )?;
                Ok(3)
            }};
        }

        use crate::decode::opcode::*;
        match opcode {
            // Control instructions
            UNREACHABLE => instruction!(unreachable),
            NOP => instruction!(nop),

            IF => {
                let false_address = self.read_u32(pc + 1)?;
                visitor.if_(JumpTarget(false_address), state)?;
                Ok(3)
            }

            BR => {
                let address = self.read_u32(pc + 1)?;
                visitor.br(JumpTarget(address), state)?;
                Ok(3)
            }

            BR_IF => {
                let true_address = self.read_u32(pc + 1)?;
                visitor.br_if(JumpTarget(true_address), state)?;
                Ok(3)
            }

            BR_TABLE => {
                let (n, offset) = if imm == 0xFF {
                    (self.read(pc + 1)?, 2)
                } else {
                    (imm as u16, 1)
                };

                let default_ = self.read_u32(pc + offset)?;

                visitor.br_table(
                    |case_| {
                        if case_ < n {
                            let Ok(addr) = self.read_u32(pc + offset + 2 + (case_ as u32 * 2))
                            else {
                                return Err(());
                            };

                            Ok(JumpTarget(addr))
                        } else {
                            Ok(JumpTarget(default_))
                        }
                    },
                    state,
                )?;

                Ok(offset + 2 + (n as u32 * 2))
            }

            RETURN => {
                visitor.return_(imm, state)?;
                Ok(1)
            }
            CALL => instruction!(call, idx, FuncIdx),
            CALL_INDIRECT => instruction!(call_indirect, idx, TypeIdx),

            // Parametric instructions
            DROP => instruction!(drop),
            SELECT => instruction!(select),

            // Variable instructions
            LOCAL_GET => instruction!(local_get, local),
            LOCAL_SET => instruction!(local_set, local),
            LOCAL_TEE => instruction!(local_tee, local),
            GLOBAL_GET => instruction!(global_get, global),
            GLOBAL_SET => instruction!(global_set, global),

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

            // This operation is not allowed during compilation
            MEMORY_GROW => unreachable!(),

            // Numeric instructions - const
            I32_CONST => {
                let n = self.read_u32(pc + 1)?;
                visitor.i32_const(n as i32, state)?;
                Ok(3)
            }
            I64_CONST => {
                let n = self.read_u64(pc + 1)?;
                visitor.i64_const(n as i64, state)?;
                Ok(5)
            }
            F32_CONST => {
                let z = self.read_u32(pc + 1)?;
                visitor.f32_const(f32::from_bits(z), state)?;
                Ok(3)
            }
            F64_CONST => {
                let z = self.read_u64(pc + 1)?;
                visitor.f64_const(f64::from_bits(z), state)?;
                Ok(5)
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
            I32_REINTERPRET_F32 => instruction!(i32_reinterpret_f32),
            I64_REINTERPRET_F64 => instruction!(i64_reinterpret_f64),
            F32_REINTERPRET_I32 => instruction!(f32_reinterpret_i32),
            F64_REINTERPRET_I64 => instruction!(f64_reinterpret_i64),

            _ => Err(IrReaderError::InvalidOpcode(opcode)),
        }
    }
}
