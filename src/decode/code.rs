use crate::*;

pub struct Expr;

impl Expr {
    pub fn read(wasm: &mut Reader) -> Result<Self, ValidationError> {
        let e = Expr;
        wasm.visit_code(&CodeIndexer, &mut ())?;
        Ok(e)
    }
}

pub struct Func {
    pub locals: [u32; 4],
    pub expr: Expr,
}

impl Func {
    pub fn read(wasm: &mut Reader) -> Result<Self, ValidationError> {
        let size = wasm.read_u32()?;
        let start = wasm.offset();

        let mut locals = [0u32; 4];
        for _ in 0..wasm.read_u32()? {
            let n = wasm.read_u32()?;
            let t = ValType::read(wasm)?;
            let i = t as usize;
            locals[i] += n;
        }

        let expr = Expr::read(wasm)?;

        let end = wasm.offset();
        if (end - start) as u32 != size {
            Err(ValidationError::MalformedCodeSize)
        } else {
            Ok(Func { locals, expr })
        }
    }
}

pub struct MemArg {
    pub align: u32,
    pub offset: u32,
}

impl MemArg {
    pub fn read(wasm: &mut Reader) -> Result<Self, ValidationError> {
        let align = wasm.read_u32()?;
        let offset = wasm.read_u32()?;
        Ok(MemArg { align, offset })
    }
}

macro_rules! instruction {
    // Instruction with no immediate
    ($self:expr, $visitor:expr, $name:ident, $state:expr) => {{
        $visitor.$name($state)?;
    }};
    // Instruction with immediate
    ($self:expr, $visitor:expr, $name:ident, $t1:ty, $state:expr) => {{
        let p1 = <$t1>::read($self)?;
        $visitor.$name(p1, $state)?;
    }};
}

impl<'wasm> Reader<'wasm> {
    /// This function will decode WASM instructions and pass them to
    /// the visitor to handle. This is the primary entrypoint for reading
    /// WASM encoded code.
    pub fn visit_code<S, B: From<ValidationError>, V: CodeVisitor<State = S, Error = B>>(
        &mut self,
        visitor: &V,
        state: &mut S,
    ) -> Result<(), B> {
        // Keep track of the block depth so that we know when we are done decoding
        let mut block_depth = 0;

        use crate::decode::opcode::*;
        loop {
            match self.read_u8()? {
                // Control instructions
                UNREACHABLE => instruction!(self, visitor, unreachable, state),
                NOP => instruction!(self, visitor, nop, state),
                BLOCK => {
                    block_depth += 1;
                    let block_type = ResultType::read(self)?;
                    visitor.enter_block(block_type, state)?;
                }
                LOOP => {
                    block_depth += 1;
                    let block_type = ResultType::read(self)?;
                    visitor.loop_(block_type, state)?;
                }
                IF => {
                    block_depth += 1;
                    let block_type = ResultType::read(self)?;
                    visitor.if_(block_type, state)?;
                }
                ELSE => instruction!(self, visitor, else_, state),
                END => {
                    if block_depth > 0 {
                        block_depth -= 1;
                        visitor.exit_block(state)?
                    } else {
                        // This is the final END opcode at the end of the code expression
                        return Ok(());
                    }
                }
                BR => instruction!(self, visitor, br, LabelIdx, state),
                BR_IF => instruction!(self, visitor, br_if, LabelIdx, state),
                BR_TABLE => {
                    // TODO(tumbar) How do we expose maximum switch cases?
                    //              I definitely don't want to support 2^32-1...
                    let lut: StaticVec<_, 64> = self.read_vec_stack(LabelIdx::read)?;
                    let default_ = LabelIdx::read(self)?;
                    visitor.br_table(&lut, default_, state)?;
                }
                RETURN => instruction!(self, visitor, return_, state),
                CALL => instruction!(self, visitor, call, FuncIdx, state),
                CALL_INDIRECT => {
                    let x = TypeIdx::read(self)?;
                    self.expect_u8(0x00)?;
                    visitor.call_indirect(x, state)?;
                }

                // Parametric instructions
                DROP => instruction!(self, visitor, drop, state),
                SELECT => instruction!(self, visitor, select, state),

                // Variable instructions
                LOCAL_GET => instruction!(self, visitor, local_get, LocalIdx, state),
                LOCAL_SET => instruction!(self, visitor, local_set, LocalIdx, state),
                LOCAL_TEE => instruction!(self, visitor, local_tee, LocalIdx, state),
                GLOBAL_GET => instruction!(self, visitor, global_get, GlobalIdx, state),
                GLOBAL_SET => instruction!(self, visitor, global_set, GlobalIdx, state),

                // Memory instructions - loads
                I32_LOAD => instruction!(self, visitor, i32_load, MemArg, state),
                I64_LOAD => instruction!(self, visitor, i64_load, MemArg, state),
                F32_LOAD => instruction!(self, visitor, f32_load, MemArg, state),
                F64_LOAD => instruction!(self, visitor, f64_load, MemArg, state),
                I32_LOAD8_S => instruction!(self, visitor, i32_load8_s, MemArg, state),
                I32_LOAD8_U => instruction!(self, visitor, i32_load8_u, MemArg, state),
                I32_LOAD16_S => instruction!(self, visitor, i32_load16_s, MemArg, state),
                I32_LOAD16_U => instruction!(self, visitor, i32_load16_u, MemArg, state),
                I64_LOAD8_S => instruction!(self, visitor, i64_load8_s, MemArg, state),
                I64_LOAD8_U => instruction!(self, visitor, i64_load8_u, MemArg, state),
                I64_LOAD16_S => instruction!(self, visitor, i64_load16_s, MemArg, state),
                I64_LOAD16_U => instruction!(self, visitor, i64_load16_u, MemArg, state),
                I64_LOAD32_S => instruction!(self, visitor, i64_load32_s, MemArg, state),
                I64_LOAD32_U => instruction!(self, visitor, i64_load32_u, MemArg, state),

                // Memory instructions - stores
                I32_STORE => instruction!(self, visitor, i32_store, MemArg, state),
                I64_STORE => instruction!(self, visitor, i64_store, MemArg, state),
                F32_STORE => instruction!(self, visitor, f32_store, MemArg, state),
                F64_STORE => instruction!(self, visitor, f64_store, MemArg, state),
                I32_STORE8 => instruction!(self, visitor, i32_store8, MemArg, state),
                I32_STORE16 => instruction!(self, visitor, i32_store16, MemArg, state),
                I64_STORE8 => instruction!(self, visitor, i64_store8, MemArg, state),
                I64_STORE16 => instruction!(self, visitor, i64_store16, MemArg, state),
                I64_STORE32 => instruction!(self, visitor, i64_store32, MemArg, state),

                // Memory instructions - size/grow
                MEMORY_SIZE => instruction!(self, visitor, memory_size, state),
                MEMORY_GROW => instruction!(self, visitor, memory_grow, state),

                // Numeric instructions - const
                I32_CONST => {
                    let n = self.read_i32()?;
                    visitor.i32_const(n, state)?;
                }
                I64_CONST => {
                    let n = self.read_i64()?;
                    visitor.i64_const(n, state)?;
                }
                F32_CONST => {
                    let z = f32::from_bits(self.read_f32()?);
                    visitor.f32_const(z, state)?;
                }
                F64_CONST => {
                    let z = f64::from_bits(self.read_f64()?);
                    visitor.f64_const(z, state)?;
                }

                // Numeric instructions - i32 test/rel
                I32_EQZ => instruction!(self, visitor, i32_eqz, state),
                I32_EQ => instruction!(self, visitor, i32_eq, state),
                I32_NE => instruction!(self, visitor, i32_ne, state),
                I32_LT_S => instruction!(self, visitor, i32_lt_s, state),
                I32_LT_U => instruction!(self, visitor, i32_lt_u, state),
                I32_GT_S => instruction!(self, visitor, i32_gt_s, state),
                I32_GT_U => instruction!(self, visitor, i32_gt_u, state),
                I32_LE_S => instruction!(self, visitor, i32_le_s, state),
                I32_LE_U => instruction!(self, visitor, i32_le_u, state),
                I32_GE_S => instruction!(self, visitor, i32_ge_s, state),
                I32_GE_U => instruction!(self, visitor, i32_ge_u, state),

                // Numeric instructions - i64 test/rel
                I64_EQZ => instruction!(self, visitor, i64_eqz, state),
                I64_EQ => instruction!(self, visitor, i64_eq, state),
                I64_NE => instruction!(self, visitor, i64_ne, state),
                I64_LT_S => instruction!(self, visitor, i64_lt_s, state),
                I64_LT_U => instruction!(self, visitor, i64_lt_u, state),
                I64_GT_S => instruction!(self, visitor, i64_gt_s, state),
                I64_GT_U => instruction!(self, visitor, i64_gt_u, state),
                I64_LE_S => instruction!(self, visitor, i64_le_s, state),
                I64_LE_U => instruction!(self, visitor, i64_le_u, state),
                I64_GE_S => instruction!(self, visitor, i64_ge_s, state),
                I64_GE_U => instruction!(self, visitor, i64_ge_u, state),

                // Numeric instructions - f32 rel
                F32_EQ => instruction!(self, visitor, f32_eq, state),
                F32_NE => instruction!(self, visitor, f32_ne, state),
                F32_LT => instruction!(self, visitor, f32_lt, state),
                F32_GT => instruction!(self, visitor, f32_gt, state),
                F32_LE => instruction!(self, visitor, f32_le, state),
                F32_GE => instruction!(self, visitor, f32_ge, state),

                // Numeric instructions - f64 rel
                F64_EQ => instruction!(self, visitor, f64_eq, state),
                F64_NE => instruction!(self, visitor, f64_ne, state),
                F64_LT => instruction!(self, visitor, f64_lt, state),
                F64_GT => instruction!(self, visitor, f64_gt, state),
                F64_LE => instruction!(self, visitor, f64_le, state),
                F64_GE => instruction!(self, visitor, f64_ge, state),

                // Numeric instructions - i32 unary/binary
                I32_CLZ => instruction!(self, visitor, i32_clz, state),
                I32_CTZ => instruction!(self, visitor, i32_ctz, state),
                I32_POPCNT => instruction!(self, visitor, i32_popcnt, state),
                I32_ADD => instruction!(self, visitor, i32_add, state),
                I32_SUB => instruction!(self, visitor, i32_sub, state),
                I32_MUL => instruction!(self, visitor, i32_mul, state),
                I32_DIV_S => instruction!(self, visitor, i32_div_s, state),
                I32_DIV_U => instruction!(self, visitor, i32_div_u, state),
                I32_REM_S => instruction!(self, visitor, i32_rem_s, state),
                I32_REM_U => instruction!(self, visitor, i32_rem_u, state),
                I32_AND => instruction!(self, visitor, i32_and, state),
                I32_OR => instruction!(self, visitor, i32_or, state),
                I32_XOR => instruction!(self, visitor, i32_xor, state),
                I32_SHL => instruction!(self, visitor, i32_shl, state),
                I32_SHR_S => instruction!(self, visitor, i32_shr_s, state),
                I32_SHR_U => instruction!(self, visitor, i32_shr_u, state),
                I32_ROTL => instruction!(self, visitor, i32_rotl, state),
                I32_ROTR => instruction!(self, visitor, i32_rotr, state),

                // Numeric instructions - i64 unary/binary
                I64_CLZ => instruction!(self, visitor, i64_clz, state),
                I64_CTZ => instruction!(self, visitor, i64_ctz, state),
                I64_POPCNT => instruction!(self, visitor, i64_popcnt, state),
                I64_ADD => instruction!(self, visitor, i64_add, state),
                I64_SUB => instruction!(self, visitor, i64_sub, state),
                I64_MUL => instruction!(self, visitor, i64_mul, state),
                I64_DIV_S => instruction!(self, visitor, i64_div_s, state),
                I64_DIV_U => instruction!(self, visitor, i64_div_u, state),
                I64_REM_S => instruction!(self, visitor, i64_rem_s, state),
                I64_REM_U => instruction!(self, visitor, i64_rem_u, state),
                I64_AND => instruction!(self, visitor, i64_and, state),
                I64_OR => instruction!(self, visitor, i64_or, state),
                I64_XOR => instruction!(self, visitor, i64_xor, state),
                I64_SHL => instruction!(self, visitor, i64_shl, state),
                I64_SHR_S => instruction!(self, visitor, i64_shr_s, state),
                I64_SHR_U => instruction!(self, visitor, i64_shr_u, state),
                I64_ROTL => instruction!(self, visitor, i64_rotl, state),
                I64_ROTR => instruction!(self, visitor, i64_rotr, state),

                // Numeric instructions - f32 unary/binary
                F32_ABS => instruction!(self, visitor, f32_abs, state),
                F32_NEG => instruction!(self, visitor, f32_neg, state),
                F32_CEIL => instruction!(self, visitor, f32_ceil, state),
                F32_FLOOR => instruction!(self, visitor, f32_floor, state),
                F32_TRUNC => instruction!(self, visitor, f32_trunc, state),
                F32_NEAREST => instruction!(self, visitor, f32_nearest, state),
                F32_SQRT => instruction!(self, visitor, f32_sqrt, state),
                F32_ADD => instruction!(self, visitor, f32_add, state),
                F32_SUB => instruction!(self, visitor, f32_sub, state),
                F32_MUL => instruction!(self, visitor, f32_mul, state),
                F32_DIV => instruction!(self, visitor, f32_div, state),
                F32_MIN => instruction!(self, visitor, f32_min, state),
                F32_MAX => instruction!(self, visitor, f32_max, state),
                F32_COPYSIGN => instruction!(self, visitor, f32_copysign, state),

                // Numeric instructions - f64 unary/binary
                F64_ABS => instruction!(self, visitor, f64_abs, state),
                F64_NEG => instruction!(self, visitor, f64_neg, state),
                F64_CEIL => instruction!(self, visitor, f64_ceil, state),
                F64_FLOOR => instruction!(self, visitor, f64_floor, state),
                F64_TRUNC => instruction!(self, visitor, f64_trunc, state),
                F64_NEAREST => instruction!(self, visitor, f64_nearest, state),
                F64_SQRT => instruction!(self, visitor, f64_sqrt, state),
                F64_ADD => instruction!(self, visitor, f64_add, state),
                F64_SUB => instruction!(self, visitor, f64_sub, state),
                F64_MUL => instruction!(self, visitor, f64_mul, state),
                F64_DIV => instruction!(self, visitor, f64_div, state),
                F64_MIN => instruction!(self, visitor, f64_min, state),
                F64_MAX => instruction!(self, visitor, f64_max, state),
                F64_COPYSIGN => instruction!(self, visitor, f64_copysign, state),

                // Numeric instructions - conversions
                I32_WRAP_I64 => instruction!(self, visitor, i32_wrap_i64, state),
                I32_TRUNC_F32_S => instruction!(self, visitor, i32_trunc_f32_s, state),
                I32_TRUNC_F32_U => instruction!(self, visitor, i32_trunc_f32_u, state),
                I32_TRUNC_F64_S => instruction!(self, visitor, i32_trunc_f64_s, state),
                I32_TRUNC_F64_U => instruction!(self, visitor, i32_trunc_f64_u, state),
                I64_EXTEND_I32_S => instruction!(self, visitor, i64_extend_i32_s, state),
                I64_EXTEND_I32_U => instruction!(self, visitor, i64_extend_i32_u, state),
                I64_TRUNC_F32_S => instruction!(self, visitor, i64_trunc_f32_s, state),
                I64_TRUNC_F32_U => instruction!(self, visitor, i64_trunc_f32_u, state),
                I64_TRUNC_F64_S => instruction!(self, visitor, i64_trunc_f64_s, state),
                I64_TRUNC_F64_U => instruction!(self, visitor, i64_trunc_f64_u, state),
                F32_CONVERT_I32_S => instruction!(self, visitor, f32_convert_i32_s, state),
                F32_CONVERT_I32_U => instruction!(self, visitor, f32_convert_i32_u, state),
                F32_CONVERT_I64_S => instruction!(self, visitor, f32_convert_i64_s, state),
                F32_CONVERT_I64_U => instruction!(self, visitor, f32_convert_i64_u, state),
                F32_DEMOTE_F64 => instruction!(self, visitor, f32_demote_f64, state),
                F64_CONVERT_I32_S => instruction!(self, visitor, f64_convert_i32_s, state),
                F64_CONVERT_I32_U => instruction!(self, visitor, f64_convert_i32_u, state),
                F64_CONVERT_I64_S => instruction!(self, visitor, f64_convert_i64_s, state),
                F64_CONVERT_I64_U => instruction!(self, visitor, f64_convert_i64_u, state),
                F64_PROMOTE_F32 => instruction!(self, visitor, f64_promote_f32, state),
                I32_REINTERPRET_F32 => instruction!(self, visitor, i32_reinterpret_f32, state),
                I64_REINTERPRET_F64 => instruction!(self, visitor, i64_reinterpret_f64, state),
                F32_REINTERPRET_I32 => instruction!(self, visitor, f32_reinterpret_i32, state),
                F64_REINTERPRET_I64 => instruction!(self, visitor, f64_reinterpret_i64, state),

                op => Err(ValidationError::InvalidOpcode(op))?,
            }
        }
    }
}
