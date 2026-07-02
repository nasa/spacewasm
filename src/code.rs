use crate::constant::ConstantCompiler;
use crate::*;

#[derive(Debug, Clone)]
pub struct Expr(pub JumpTarget);

impl Expr {
    pub fn zero() -> Expr {
        Expr(JumpTarget(0))
    }

    pub fn read_constant(
        wasm: &mut Reader,
        store: &Store,
        module: &Module,
    ) -> Result<Value, ValidationError> {
        let mut value: Option<Value> = None;
        wasm.read_code(&ConstantCompiler::new(store, module), &mut value)?;
        Ok(value.unwrap())
    }

    pub fn read<const N: usize>(
        wasm: &mut Reader,
        builder: &mut CodeBuilder<N>,
        store: &Store,
        module: &Module,
        ctx: &Func,
        compiler_options: CompilerOptions,
    ) -> Result<(Self, u16), ValidationError> {
        let e = Expr(builder.pc());
        let tb = &mut TextBuilder::new(builder, store, module, ctx);
        wasm.read_code(&Compiler::<'_, N>::new(compiler_options), tb)?;

        Ok((e, tb.stack_usage() as u16))
    }
}

impl From<Expr> for JumpTarget {
    fn from(value: Expr) -> Self {
        value.0
    }
}

#[derive(Debug, Clone)]
pub struct Func {
    /// Function signature.
    pub ty: TypeIdx,

    /// Maximum shallow stack usage by this function (not including inner function calls)
    /// (determined from analysis)
    pub stack_usage: u16,

    /// Size of the local variables
    pub local_size: u16,

    /// Parameter size in 32-bit words
    pub parameter_size: u8,

    /// Return value size in 32-bit words
    pub return_ty: Option<ValType>,

    /// Local variables allocated in this functions frame
    /// Read in the code section
    pub locals: Vec<(u16, ValType)>,

    /// Functions entry point
    pub expr: Expr,
}

impl Module {
    pub fn read_function_code<const N: usize>(
        &mut self,
        wasm: &mut Reader,
        store: &Store,
        builder: &mut CodeBuilder<N>,
        i: usize,
        compiler_options: CompilerOptions,
    ) -> Result<(), ValidationError> {
        let size = wasm.read_u32()?;
        let start = wasm.offset();

        let empty_f = self.functions[i].clone();
        let mut f = core::mem::replace(&mut self.functions[i], empty_f);

        f.locals = wasm.read_vec(|w| {
            let n = w.read_u32()?;
            let t = ValType::read(w)?;

            if n > 0xFFFF {
                return Err(ValidationError::TooManyLocals);
            }

            Ok((n as u16, t))
        })?;

        // Compute the local size in words
        let size_in_words = f
            .locals
            .iter()
            .fold(0, |sum, (n, ty)| sum + (*n as usize) * ty.size())
            / 4;

        if size_in_words > 0xFFFF {
            return Err(ValidationError::TooManyLocals);
        }

        f.local_size = size_in_words as u16;
        (f.expr, f.stack_usage) = Expr::read(wasm, builder, store, self, &f, compiler_options)?;

        let _ = core::mem::replace(&mut self.functions[i], f);

        let end = wasm.offset();
        if (end - start) as u32 != size {
            Err(ValidationError::MalformedCodeSize)
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

impl<'wasm> Reader<'wasm> {
    /// This function will decode WASM instructions and pass them to
    /// the visitor to handle. This is the primary entrypoint for reading
    /// WASM encoded code.
    pub fn read_code<S, E, V>(&mut self, visitor: &V, state: &mut S) -> Result<(), ValidationError>
    where
        V: WasmVisitor<State = S, Error = E>,
        ValidationError: From<E>,
    {
        // Keep track of the block depth so that we know when we are done decoding
        let mut block_depth = 0;

        macro_rules! instruction {
            // Instruction with no immediate
            ($name:ident) => {{
                visitor.$name(state)?;
            }};
            // Instruction with immediate
            ($name:ident, $t1:ty) => {{
                let p1 = <$t1>::read(self)?;
                visitor.$name(p1, state)?;
            }};
        }

        use crate::opcode::*;
        loop {
            match self.read_u8()? {
                // Control instructions
                UNREACHABLE => instruction!(unreachable),
                NOP => instruction!(nop),
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
                ELSE => instruction!(else_),
                END => {
                    if block_depth > 0 {
                        block_depth -= 1;
                        visitor.exit_block(state)?
                    } else {
                        // This is the final END opcode at the end of the code expression
                        visitor.finish(state)?;
                        return Ok(());
                    }
                }
                BR => instruction!(br, LabelIdx),
                BR_IF => instruction!(br_if, LabelIdx),
                BR_TABLE => {
                    // FIXME(tumbar) Is it possible to use an iterator and not require up-front allocation?
                    let lut = self
                        .read_vec_stack::<256, LabelIdx>(LabelIdx::read)
                        .map_err(|e| match e {
                            ValidationError::VecTooLong => ValidationError::BrTableHasTooManyCases,
                            e => e,
                        })?;

                    let default_ = LabelIdx::read(self)?;
                    visitor.br_table(&lut, default_, state)?;
                }
                RETURN => instruction!(return_),
                CALL => instruction!(call, FuncIdx),
                CALL_INDIRECT => {
                    let x = TypeIdx::read(self)?;
                    self.expect_u8(0x00)?;
                    visitor.call_indirect(x, state)?;
                }

                // Parametric instructions
                DROP => instruction!(drop),
                SELECT => instruction!(select),

                // Variable instructions
                LOCAL_GET => instruction!(local_get, LocalIdx),
                LOCAL_SET => instruction!(local_set, LocalIdx),
                LOCAL_TEE => instruction!(local_tee, LocalIdx),
                GLOBAL_GET => instruction!(global_get, GlobalIdx),
                GLOBAL_SET => instruction!(global_set, GlobalIdx),

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
                MEMORY_SIZE => {
                    self.expect_u8(0x00)?;
                    visitor.memory_size(state)?;
                }
                MEMORY_GROW => {
                    self.expect_u8(0x00)?;
                    visitor.memory_grow(state)?;
                }

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

                EXTENDED => {

                }

                op => Err(ValidationError::InvalidOpcode(op))?,
            }
        }
    }
}
