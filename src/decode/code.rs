use crate::*;

pub struct Expr<'wasm>(WasmReaderState<'wasm>);

impl<'wasm> Expr<'wasm> {
    pub fn read(wasm: &mut WasmReader<'wasm>) -> Result<Self, ValidationError> {
        let e = Expr(wasm.save());
        wasm.visit_code(&mut EmptyVisitor)?;
        Ok(e)
    }

    pub fn visit<E, V>(&self, wasm: &mut WasmReader<'wasm>, visitor: &mut V) -> Result<(), E>
    where
        E: From<ValidationError>,
        V: CodeVisitor<Error = E>,
    {
        wasm.restore(self.0);
        wasm.visit_code(visitor)
    }
}

pub struct Func<'wasm> {
    pub locals: Vec<ValType>,
    pub expr: Expr<'wasm>,
}

impl<'wasm> Func<'wasm> {
    pub fn read(wasm: &mut WasmReader<'wasm>) -> Result<Self, ValidationError> {
        let size = wasm.read_u32()?;

        let start = wasm.save();

        // Nested list of locals needs to be counted before allocating
        let mut n_locals = 0;
        let n_local_lists = wasm.read_u32()?;

        let start_locals = wasm.save();
        for _ in 0..n_local_lists as usize {
            let n = wasm.read_u32()?;
            wasm.read_u8()?;
            n_locals += n;
        }

        wasm.restore(start_locals);
        let mut locals = Vec::new(n_locals)?;
        for _ in 0..n_local_lists {
            let n = wasm.read_u32()?;
            let t = ValType::read(wasm)?;
            for _ in 0..n {
                locals.push(t)
            }
        }

        let expr = Expr::read(wasm)?;

        let end = wasm.save();
        if end - start != size {
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
    pub fn read(wasm: &mut WasmReader<'_>) -> Result<Self, ValidationError> {
        let align = wasm.read_u32()?;
        let offset = wasm.read_u32()?;
        Ok(MemArg { align, offset })
    }
}

macro_rules! instruction {
    // Instruction with no immediate
    ($self:expr, $visitor:expr, $name:ident) => {{
        $visitor.$name($self)?;
    }};
    // Instruction with immediate
    ($self:expr, $visitor:expr, $name:ident, $t1:ty) => {{
        let p1 = <$t1>::read($self)?;
        $visitor.$name($self, p1)?;
    }};
}

impl<'wasm> WasmReader<'wasm> {
    pub fn visit_code<B: From<ValidationError>, V: CodeVisitor<Error = B>>(
        &mut self,
        visitor: &mut V,
    ) -> Result<(), B> {
        let mut blocks: StackVec<ResultType, 64> = StackVec::new();

        use crate::decode::opcode::*;
        loop {
            let op = self.read_u8()?;
            match op {
                // Control instructions
                UNREACHABLE => instruction!(self, visitor, unreachable),
                NOP => instruction!(self, visitor, nop),
                BLOCK => {
                    let block_type = ResultType::read(self)?;
                    blocks.push(block_type);
                    visitor.enter_block(self, block_type)?;
                }
                LOOP => {
                    let block_type = ResultType::read(self)?;
                    blocks.push(block_type);
                    visitor.loop_(self, block_type)?;
                }
                IF => {
                    let block_type = ResultType::read(self)?;
                    blocks.push(block_type);
                    visitor.if_(self, block_type)?;
                }
                ELSE => instruction!(self, visitor, else_),
                END => {
                    let Some(block_type) = blocks.pop() else {
                        // No more blocks to nest in, this is the end of the code
                        break;
                    };

                    visitor.exit_block(self, block_type)?;
                }
                BR => instruction!(self, visitor, br, LabelIdx),
                BR_IF => instruction!(self, visitor, br_if, LabelIdx),
                BR_TABLE => {
                    // TODO(tumbar) How do we expose maximum switch cases?
                    //              I definitely don't want to support 2^32-1...
                    let lut: StackVec<_, 64> = self.read_vec_stack(LabelIdx::read)?;
                    let default_ = LabelIdx::read(self)?;
                    visitor.br_table(self, &lut, default_)?;
                }
                RETURN => instruction!(self, visitor, return_),
                CALL => instruction!(self, visitor, call, FuncIdx),
                CALL_INDIRECT => {
                    let x = TypeIdx::read(self)?;
                    self.expect_u8(0x00)?;
                    visitor.call_indirect(self, x)?;
                }

                // Parametric instructions
                DROP => instruction!(self, visitor, drop),
                SELECT => instruction!(self, visitor, select),

                // Variable instructions
                LOCAL_GET => instruction!(self, visitor, local_get, LocalIdx),
                LOCAL_SET => instruction!(self, visitor, local_set, LocalIdx),
                LOCAL_TEE => instruction!(self, visitor, local_tee, LocalIdx),
                GLOBAL_GET => instruction!(self, visitor, global_get, GlobalIdx),
                GLOBAL_SET => instruction!(self, visitor, global_set, GlobalIdx),

                // Memory instructions - loads
                I32_LOAD => instruction!(self, visitor, i32_load, MemArg),
                I64_LOAD => instruction!(self, visitor, i64_load, MemArg),
                F32_LOAD => instruction!(self, visitor, f32_load, MemArg),
                F64_LOAD => instruction!(self, visitor, f64_load, MemArg),
                I32_LOAD8_S => instruction!(self, visitor, i32_load8_s, MemArg),
                I32_LOAD8_U => instruction!(self, visitor, i32_load8_u, MemArg),
                I32_LOAD16_S => instruction!(self, visitor, i32_load16_s, MemArg),
                I32_LOAD16_U => instruction!(self, visitor, i32_load16_u, MemArg),
                I64_LOAD8_S => instruction!(self, visitor, i64_load8_s, MemArg),
                I64_LOAD8_U => instruction!(self, visitor, i64_load8_u, MemArg),
                I64_LOAD16_S => instruction!(self, visitor, i64_load16_s, MemArg),
                I64_LOAD16_U => instruction!(self, visitor, i64_load16_u, MemArg),
                I64_LOAD32_S => instruction!(self, visitor, i64_load32_s, MemArg),
                I64_LOAD32_U => instruction!(self, visitor, i64_load32_u, MemArg),

                // Memory instructions - stores
                I32_STORE => instruction!(self, visitor, i32_store, MemArg),
                I64_STORE => instruction!(self, visitor, i64_store, MemArg),
                F32_STORE => instruction!(self, visitor, f32_store, MemArg),
                F64_STORE => instruction!(self, visitor, f64_store, MemArg),
                I32_STORE8 => instruction!(self, visitor, i32_store8, MemArg),
                I32_STORE16 => instruction!(self, visitor, i32_store16, MemArg),
                I64_STORE8 => instruction!(self, visitor, i64_store8, MemArg),
                I64_STORE16 => instruction!(self, visitor, i64_store16, MemArg),
                I64_STORE32 => instruction!(self, visitor, i64_store32, MemArg),

                // Memory instructions - size/grow
                MEMORY_SIZE => instruction!(self, visitor, memory_size),
                MEMORY_GROW => instruction!(self, visitor, memory_grow),

                // Numeric instructions - const
                I32_CONST => {
                    let n = self.read_i32()?;
                    visitor.i32_const(self, n)?;
                }
                I64_CONST => {
                    let n = self.read_i64()?;
                    visitor.i64_const(self, n)?;
                }
                F32_CONST => {
                    let z = f32::from_bits(self.read_f32()?);
                    visitor.f32_const(self, z)?;
                }
                F64_CONST => {
                    let z = f64::from_bits(self.read_f64()?);
                    visitor.f64_const(self, z)?;
                }

                // Numeric instructions - i32 test/rel
                I32_EQZ => instruction!(self, visitor, i32_eqz),
                I32_EQ => instruction!(self, visitor, i32_eq),
                I32_NE => instruction!(self, visitor, i32_ne),
                I32_LT_S => instruction!(self, visitor, i32_lt_s),
                I32_LT_U => instruction!(self, visitor, i32_lt_u),
                I32_GT_S => instruction!(self, visitor, i32_gt_s),
                I32_GT_U => instruction!(self, visitor, i32_gt_u),
                I32_LE_S => instruction!(self, visitor, i32_le_s),
                I32_LE_U => instruction!(self, visitor, i32_le_u),
                I32_GE_S => instruction!(self, visitor, i32_ge_s),
                I32_GE_U => instruction!(self, visitor, i32_ge_u),

                // Numeric instructions - i64 test/rel
                I64_EQZ => instruction!(self, visitor, i64_eqz),
                I64_EQ => instruction!(self, visitor, i64_eq),
                I64_NE => instruction!(self, visitor, i64_ne),
                I64_LT_S => instruction!(self, visitor, i64_lt_s),
                I64_LT_U => instruction!(self, visitor, i64_lt_u),
                I64_GT_S => instruction!(self, visitor, i64_gt_s),
                I64_GT_U => instruction!(self, visitor, i64_gt_u),
                I64_LE_S => instruction!(self, visitor, i64_le_s),
                I64_LE_U => instruction!(self, visitor, i64_le_u),
                I64_GE_S => instruction!(self, visitor, i64_ge_s),
                I64_GE_U => instruction!(self, visitor, i64_ge_u),

                // Numeric instructions - f32 rel
                F32_EQ => instruction!(self, visitor, f32_eq),
                F32_NE => instruction!(self, visitor, f32_ne),
                F32_LT => instruction!(self, visitor, f32_lt),
                F32_GT => instruction!(self, visitor, f32_gt),
                F32_LE => instruction!(self, visitor, f32_le),
                F32_GE => instruction!(self, visitor, f32_ge),

                // Numeric instructions - f64 rel
                F64_EQ => instruction!(self, visitor, f64_eq),
                F64_NE => instruction!(self, visitor, f64_ne),
                F64_LT => instruction!(self, visitor, f64_lt),
                F64_GT => instruction!(self, visitor, f64_gt),
                F64_LE => instruction!(self, visitor, f64_le),
                F64_GE => instruction!(self, visitor, f64_ge),

                // Numeric instructions - i32 unary/binary
                I32_CLZ => instruction!(self, visitor, i32_clz),
                I32_CTZ => instruction!(self, visitor, i32_ctz),
                I32_POPCNT => instruction!(self, visitor, i32_popcnt),
                I32_ADD => instruction!(self, visitor, i32_add),
                I32_SUB => instruction!(self, visitor, i32_sub),
                I32_MUL => instruction!(self, visitor, i32_mul),
                I32_DIV_S => instruction!(self, visitor, i32_div_s),
                I32_DIV_U => instruction!(self, visitor, i32_div_u),
                I32_REM_S => instruction!(self, visitor, i32_rem_s),
                I32_REM_U => instruction!(self, visitor, i32_rem_u),
                I32_AND => instruction!(self, visitor, i32_and),
                I32_OR => instruction!(self, visitor, i32_or),
                I32_XOR => instruction!(self, visitor, i32_xor),
                I32_SHL => instruction!(self, visitor, i32_shl),
                I32_SHR_S => instruction!(self, visitor, i32_shr_s),
                I32_SHR_U => instruction!(self, visitor, i32_shr_u),
                I32_ROTL => instruction!(self, visitor, i32_rotl),
                I32_ROTR => instruction!(self, visitor, i32_rotr),

                // Numeric instructions - i64 unary/binary
                I64_CLZ => instruction!(self, visitor, i64_clz),
                I64_CTZ => instruction!(self, visitor, i64_ctz),
                I64_POPCNT => instruction!(self, visitor, i64_popcnt),
                I64_ADD => instruction!(self, visitor, i64_add),
                I64_SUB => instruction!(self, visitor, i64_sub),
                I64_MUL => instruction!(self, visitor, i64_mul),
                I64_DIV_S => instruction!(self, visitor, i64_div_s),
                I64_DIV_U => instruction!(self, visitor, i64_div_u),
                I64_REM_S => instruction!(self, visitor, i64_rem_s),
                I64_REM_U => instruction!(self, visitor, i64_rem_u),
                I64_AND => instruction!(self, visitor, i64_and),
                I64_OR => instruction!(self, visitor, i64_or),
                I64_XOR => instruction!(self, visitor, i64_xor),
                I64_SHL => instruction!(self, visitor, i64_shl),
                I64_SHR_S => instruction!(self, visitor, i64_shr_s),
                I64_SHR_U => instruction!(self, visitor, i64_shr_u),
                I64_ROTL => instruction!(self, visitor, i64_rotl),
                I64_ROTR => instruction!(self, visitor, i64_rotr),

                // Numeric instructions - f32 unary/binary
                F32_ABS => instruction!(self, visitor, f32_abs),
                F32_NEG => instruction!(self, visitor, f32_neg),
                F32_CEIL => instruction!(self, visitor, f32_ceil),
                F32_FLOOR => instruction!(self, visitor, f32_floor),
                F32_TRUNC => instruction!(self, visitor, f32_trunc),
                F32_NEAREST => instruction!(self, visitor, f32_nearest),
                F32_SQRT => instruction!(self, visitor, f32_sqrt),
                F32_ADD => instruction!(self, visitor, f32_add),
                F32_SUB => instruction!(self, visitor, f32_sub),
                F32_MUL => instruction!(self, visitor, f32_mul),
                F32_DIV => instruction!(self, visitor, f32_div),
                F32_MIN => instruction!(self, visitor, f32_min),
                F32_MAX => instruction!(self, visitor, f32_max),
                F32_COPYSIGN => instruction!(self, visitor, f32_copysign),

                // Numeric instructions - f64 unary/binary
                F64_ABS => instruction!(self, visitor, f64_abs),
                F64_NEG => instruction!(self, visitor, f64_neg),
                F64_CEIL => instruction!(self, visitor, f64_ceil),
                F64_FLOOR => instruction!(self, visitor, f64_floor),
                F64_TRUNC => instruction!(self, visitor, f64_trunc),
                F64_NEAREST => instruction!(self, visitor, f64_nearest),
                F64_SQRT => instruction!(self, visitor, f64_sqrt),
                F64_ADD => instruction!(self, visitor, f64_add),
                F64_SUB => instruction!(self, visitor, f64_sub),
                F64_MUL => instruction!(self, visitor, f64_mul),
                F64_DIV => instruction!(self, visitor, f64_div),
                F64_MIN => instruction!(self, visitor, f64_min),
                F64_MAX => instruction!(self, visitor, f64_max),
                F64_COPYSIGN => instruction!(self, visitor, f64_copysign),

                // Numeric instructions - conversions
                I32_WRAP_I64 => instruction!(self, visitor, i32_wrap_i64),
                I32_TRUNC_F32_S => instruction!(self, visitor, i32_trunc_f32_s),
                I32_TRUNC_F32_U => instruction!(self, visitor, i32_trunc_f32_u),
                I32_TRUNC_F64_S => instruction!(self, visitor, i32_trunc_f64_s),
                I32_TRUNC_F64_U => instruction!(self, visitor, i32_trunc_f64_u),
                I64_EXTEND_I32_S => instruction!(self, visitor, i64_extend_i32_s),
                I64_EXTEND_I32_U => instruction!(self, visitor, i64_extend_i32_u),
                I64_TRUNC_F32_S => instruction!(self, visitor, i64_trunc_f32_s),
                I64_TRUNC_F32_U => instruction!(self, visitor, i64_trunc_f32_u),
                I64_TRUNC_F64_S => instruction!(self, visitor, i64_trunc_f64_s),
                I64_TRUNC_F64_U => instruction!(self, visitor, i64_trunc_f64_u),
                F32_CONVERT_I32_S => instruction!(self, visitor, f32_convert_i32_s),
                F32_CONVERT_I32_U => instruction!(self, visitor, f32_convert_i32_u),
                F32_CONVERT_I64_S => instruction!(self, visitor, f32_convert_i64_s),
                F32_CONVERT_I64_U => instruction!(self, visitor, f32_convert_i64_u),
                F32_DEMOTE_F64 => instruction!(self, visitor, f32_demote_f64),
                F64_CONVERT_I32_S => instruction!(self, visitor, f64_convert_i32_s),
                F64_CONVERT_I32_U => instruction!(self, visitor, f64_convert_i32_u),
                F64_CONVERT_I64_S => instruction!(self, visitor, f64_convert_i64_s),
                F64_CONVERT_I64_U => instruction!(self, visitor, f64_convert_i64_u),
                F64_PROMOTE_F32 => instruction!(self, visitor, f64_promote_f32),
                I32_REINTERPRET_F32 => instruction!(self, visitor, i32_reinterpret_f32),
                I64_REINTERPRET_F64 => instruction!(self, visitor, i64_reinterpret_f64),
                F32_REINTERPRET_I32 => instruction!(self, visitor, f32_reinterpret_i32),
                F64_REINTERPRET_I64 => instruction!(self, visitor, f64_reinterpret_i64),

                op => Err(ValidationError::InvalidOpcode(op))?,
            }
        }

        Ok(())
    }
}

macro_rules! visitor_default_impl {
    // No additional parameters
    ($name:ident) => {
        fn $name(&mut self, pc: &mut WasmReader) -> Result<(), Self::Error> {
            let _ = pc;
            Ok(())
        }
    };

    // With additional parameters
    ($name:ident, $($param:ident : $ty:ty),+) => {
        fn $name(&mut self, pc: &mut WasmReader, $($param: $ty),+) -> Result<(), Self::Error> {
            let _ = pc;
            $(let _ = $param;)+
            Ok(())
        }
    };
}

pub trait CodeVisitor {
    type Error: From<ValidationError>;

    // Control instructions
    visitor_default_impl!(unreachable);
    visitor_default_impl!(nop);
    visitor_default_impl!(enter_block, block_type: ResultType);
    visitor_default_impl!(exit_block, block_type: ResultType);
    visitor_default_impl!(loop_, block_type: ResultType);
    visitor_default_impl!(if_, block_type: ResultType);
    visitor_default_impl!(else_);
    visitor_default_impl!(br, l: LabelIdx);
    visitor_default_impl!(br_if, l: LabelIdx);
    visitor_default_impl!(br_table, lut: &[LabelIdx], default_: LabelIdx);
    visitor_default_impl!(return_);
    visitor_default_impl!(call, x: FuncIdx);
    visitor_default_impl!(call_indirect, x: TypeIdx);

    // Parametric instructions
    visitor_default_impl!(drop);
    visitor_default_impl!(select);

    // Variable instructions
    visitor_default_impl!(local_get, x: LocalIdx);
    visitor_default_impl!(local_set, x: LocalIdx);
    visitor_default_impl!(local_tee, x: LocalIdx);
    visitor_default_impl!(global_get, x: GlobalIdx);
    visitor_default_impl!(global_set, x: GlobalIdx);

    // Memory instructions - loads
    visitor_default_impl!(i32_load, m: MemArg);
    visitor_default_impl!(i64_load, m: MemArg);
    visitor_default_impl!(f32_load, m: MemArg);
    visitor_default_impl!(f64_load, m: MemArg);
    visitor_default_impl!(i32_load8_s, m: MemArg);
    visitor_default_impl!(i32_load8_u, m: MemArg);
    visitor_default_impl!(i32_load16_s, m: MemArg);
    visitor_default_impl!(i32_load16_u, m: MemArg);
    visitor_default_impl!(i64_load8_s, m: MemArg);
    visitor_default_impl!(i64_load8_u, m: MemArg);
    visitor_default_impl!(i64_load16_s, m: MemArg);
    visitor_default_impl!(i64_load16_u, m: MemArg);
    visitor_default_impl!(i64_load32_s, m: MemArg);
    visitor_default_impl!(i64_load32_u, m: MemArg);

    // Memory instructions - stores
    visitor_default_impl!(i32_store, m: MemArg);
    visitor_default_impl!(i64_store, m: MemArg);
    visitor_default_impl!(f32_store, m: MemArg);
    visitor_default_impl!(f64_store, m: MemArg);
    visitor_default_impl!(i32_store8, m: MemArg);
    visitor_default_impl!(i32_store16, m: MemArg);
    visitor_default_impl!(i64_store8, m: MemArg);
    visitor_default_impl!(i64_store16, m: MemArg);
    visitor_default_impl!(i64_store32, m: MemArg);

    // Memory instructions - size/grow
    visitor_default_impl!(memory_size);
    visitor_default_impl!(memory_grow);

    // Numeric instructions - const
    visitor_default_impl!(i32_const, n: i32);
    visitor_default_impl!(i64_const, n: i64);
    visitor_default_impl!(f32_const, z: f32);
    visitor_default_impl!(f64_const, z: f64);

    // Numeric instructions - i32 test/rel
    visitor_default_impl!(i32_eqz);
    visitor_default_impl!(i32_eq);
    visitor_default_impl!(i32_ne);
    visitor_default_impl!(i32_lt_s);
    visitor_default_impl!(i32_lt_u);
    visitor_default_impl!(i32_gt_s);
    visitor_default_impl!(i32_gt_u);
    visitor_default_impl!(i32_le_s);
    visitor_default_impl!(i32_le_u);
    visitor_default_impl!(i32_ge_s);
    visitor_default_impl!(i32_ge_u);

    // Numeric instructions - i64 test/rel
    visitor_default_impl!(i64_eqz);
    visitor_default_impl!(i64_eq);
    visitor_default_impl!(i64_ne);
    visitor_default_impl!(i64_lt_s);
    visitor_default_impl!(i64_lt_u);
    visitor_default_impl!(i64_gt_s);
    visitor_default_impl!(i64_gt_u);
    visitor_default_impl!(i64_le_s);
    visitor_default_impl!(i64_le_u);
    visitor_default_impl!(i64_ge_s);
    visitor_default_impl!(i64_ge_u);

    // Numeric instructions - f32 rel
    visitor_default_impl!(f32_eq);
    visitor_default_impl!(f32_ne);
    visitor_default_impl!(f32_lt);
    visitor_default_impl!(f32_gt);
    visitor_default_impl!(f32_le);
    visitor_default_impl!(f32_ge);

    // Numeric instructions - f64 rel
    visitor_default_impl!(f64_eq);
    visitor_default_impl!(f64_ne);
    visitor_default_impl!(f64_lt);
    visitor_default_impl!(f64_gt);
    visitor_default_impl!(f64_le);
    visitor_default_impl!(f64_ge);

    // Numeric instructions - i32 unary/binary
    visitor_default_impl!(i32_clz);
    visitor_default_impl!(i32_ctz);
    visitor_default_impl!(i32_popcnt);
    visitor_default_impl!(i32_add);
    visitor_default_impl!(i32_sub);
    visitor_default_impl!(i32_mul);
    visitor_default_impl!(i32_div_s);
    visitor_default_impl!(i32_div_u);
    visitor_default_impl!(i32_rem_s);
    visitor_default_impl!(i32_rem_u);
    visitor_default_impl!(i32_and);
    visitor_default_impl!(i32_or);
    visitor_default_impl!(i32_xor);
    visitor_default_impl!(i32_shl);
    visitor_default_impl!(i32_shr_s);
    visitor_default_impl!(i32_shr_u);
    visitor_default_impl!(i32_rotl);
    visitor_default_impl!(i32_rotr);

    // Numeric instructions - i64 unary/binary
    visitor_default_impl!(i64_clz);
    visitor_default_impl!(i64_ctz);
    visitor_default_impl!(i64_popcnt);
    visitor_default_impl!(i64_add);
    visitor_default_impl!(i64_sub);
    visitor_default_impl!(i64_mul);
    visitor_default_impl!(i64_div_s);
    visitor_default_impl!(i64_div_u);
    visitor_default_impl!(i64_rem_s);
    visitor_default_impl!(i64_rem_u);
    visitor_default_impl!(i64_and);
    visitor_default_impl!(i64_or);
    visitor_default_impl!(i64_xor);
    visitor_default_impl!(i64_shl);
    visitor_default_impl!(i64_shr_s);
    visitor_default_impl!(i64_shr_u);
    visitor_default_impl!(i64_rotl);
    visitor_default_impl!(i64_rotr);

    // Numeric instructions - f32 unary/binary
    visitor_default_impl!(f32_abs);
    visitor_default_impl!(f32_neg);
    visitor_default_impl!(f32_ceil);
    visitor_default_impl!(f32_floor);
    visitor_default_impl!(f32_trunc);
    visitor_default_impl!(f32_nearest);
    visitor_default_impl!(f32_sqrt);
    visitor_default_impl!(f32_add);
    visitor_default_impl!(f32_sub);
    visitor_default_impl!(f32_mul);
    visitor_default_impl!(f32_div);
    visitor_default_impl!(f32_min);
    visitor_default_impl!(f32_max);
    visitor_default_impl!(f32_copysign);

    // Numeric instructions - f64 unary/binary
    visitor_default_impl!(f64_abs);
    visitor_default_impl!(f64_neg);
    visitor_default_impl!(f64_ceil);
    visitor_default_impl!(f64_floor);
    visitor_default_impl!(f64_trunc);
    visitor_default_impl!(f64_nearest);
    visitor_default_impl!(f64_sqrt);
    visitor_default_impl!(f64_add);
    visitor_default_impl!(f64_sub);
    visitor_default_impl!(f64_mul);
    visitor_default_impl!(f64_div);
    visitor_default_impl!(f64_min);
    visitor_default_impl!(f64_max);
    visitor_default_impl!(f64_copysign);

    // Numeric instructions - conversions
    visitor_default_impl!(i32_wrap_i64);
    visitor_default_impl!(i32_trunc_f32_s);
    visitor_default_impl!(i32_trunc_f32_u);
    visitor_default_impl!(i32_trunc_f64_s);
    visitor_default_impl!(i32_trunc_f64_u);
    visitor_default_impl!(i64_extend_i32_s);
    visitor_default_impl!(i64_extend_i32_u);
    visitor_default_impl!(i64_trunc_f32_s);
    visitor_default_impl!(i64_trunc_f32_u);
    visitor_default_impl!(i64_trunc_f64_s);
    visitor_default_impl!(i64_trunc_f64_u);
    visitor_default_impl!(f32_convert_i32_s);
    visitor_default_impl!(f32_convert_i32_u);
    visitor_default_impl!(f32_convert_i64_s);
    visitor_default_impl!(f32_convert_i64_u);
    visitor_default_impl!(f32_demote_f64);
    visitor_default_impl!(f64_convert_i32_s);
    visitor_default_impl!(f64_convert_i32_u);
    visitor_default_impl!(f64_convert_i64_s);
    visitor_default_impl!(f64_convert_i64_u);
    visitor_default_impl!(f64_promote_f32);
    visitor_default_impl!(i32_reinterpret_f32);
    visitor_default_impl!(i64_reinterpret_f64);
    visitor_default_impl!(f32_reinterpret_i32);
    visitor_default_impl!(f64_reinterpret_i64);
}

pub struct EmptyVisitor;
impl CodeVisitor for EmptyVisitor {
    type Error = ValidationError;
}
