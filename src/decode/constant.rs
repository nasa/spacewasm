use crate::{
    BaseVisitor, FuncIdx, GlobalIdx, LabelIdx, LocalIdx, MemArg, ResultType, TypeIdx, Value,
    WasmVisitor,
};

pub struct ConstantCompiler;

macro_rules! invalid_constant_fn {
    // No additional parameters
    ($name:ident) => {
        fn $name(&self, state: &mut Self::State) -> Result<(), Self::Error> {
            let _ = state;
            Err(ConstantExprError::InvalidConstantInstruction)
        }
    };

    // With additional parameters
    ($name:ident, $($param:ident : $ty:ty),+) => {
        fn $name(&self, $($param: $ty),+, state: &mut Self::State) -> Result<(), Self::Error> {
            $(let _ = $param;)+
            let _ = state;
            Err(ConstantExprError::InvalidConstantInstruction)
        }
    };
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConstantExprError {
    InvalidConstantInstruction,
    AlreadyHasValue,
    NoValue,
}

impl BaseVisitor for ConstantCompiler {
    type Error = ConstantExprError;
    type State = Option<Value>;

    // Control instructions
    invalid_constant_fn!(unreachable);
    invalid_constant_fn!(nop);

    // Control flow is not handled by the base visitor

    // Parametric instructions
    invalid_constant_fn!(drop);
    invalid_constant_fn!(select);

    // Memory instructions - loads
    invalid_constant_fn!(i32_load, m: MemArg);
    invalid_constant_fn!(i64_load, m: MemArg);
    invalid_constant_fn!(f32_load, m: MemArg);
    invalid_constant_fn!(f64_load, m: MemArg);
    invalid_constant_fn!(i32_load8_s, m: MemArg);
    invalid_constant_fn!(i32_load8_u, m: MemArg);
    invalid_constant_fn!(i32_load16_s, m: MemArg);
    invalid_constant_fn!(i32_load16_u, m: MemArg);
    invalid_constant_fn!(i64_load8_s, m: MemArg);
    invalid_constant_fn!(i64_load8_u, m: MemArg);
    invalid_constant_fn!(i64_load16_s, m: MemArg);
    invalid_constant_fn!(i64_load16_u, m: MemArg);
    invalid_constant_fn!(i64_load32_s, m: MemArg);
    invalid_constant_fn!(i64_load32_u, m: MemArg);

    // Memory instructions - stores
    invalid_constant_fn!(i32_store, m: MemArg);
    invalid_constant_fn!(i64_store, m: MemArg);
    invalid_constant_fn!(f32_store, m: MemArg);
    invalid_constant_fn!(f64_store, m: MemArg);
    invalid_constant_fn!(i32_store8, m: MemArg);
    invalid_constant_fn!(i32_store16, m: MemArg);
    invalid_constant_fn!(i64_store8, m: MemArg);
    invalid_constant_fn!(i64_store16, m: MemArg);
    invalid_constant_fn!(i64_store32, m: MemArg);

    // Memory instructions - size/grow
    invalid_constant_fn!(memory_size);
    invalid_constant_fn!(memory_grow);

    // Numeric instructions - const
    fn i32_const(&self, n: i32, state: &mut Self::State) -> Result<(), Self::Error> {
        let before = state.replace(Value::I32(n));
        if let Some(_) = before {
            Err(ConstantExprError::AlreadyHasValue)
        } else {
            Ok(())
        }
    }
    fn i64_const(&self, n: i64, state: &mut Self::State) -> Result<(), Self::Error> {
        let before = state.replace(Value::I64(n));
        if let Some(_) = before {
            Err(ConstantExprError::AlreadyHasValue)
        } else {
            Ok(())
        }
    }
    fn f32_const(&self, z: f32, state: &mut Self::State) -> Result<(), Self::Error> {
        let before = state.replace(Value::F32(z));
        if let Some(_) = before {
            Err(ConstantExprError::AlreadyHasValue)
        } else {
            Ok(())
        }
    }
    fn f64_const(&self, z: f64, state: &mut Self::State) -> Result<(), Self::Error> {
        let before = state.replace(Value::F64(z));
        if let Some(_) = before {
            Err(ConstantExprError::AlreadyHasValue)
        } else {
            Ok(())
        }
    }

    // Numeric instructions - i32 test/rel
    invalid_constant_fn!(i32_eqz);
    invalid_constant_fn!(i32_eq);
    invalid_constant_fn!(i32_ne);
    invalid_constant_fn!(i32_lt_s);
    invalid_constant_fn!(i32_lt_u);
    invalid_constant_fn!(i32_gt_s);
    invalid_constant_fn!(i32_gt_u);
    invalid_constant_fn!(i32_le_s);
    invalid_constant_fn!(i32_le_u);
    invalid_constant_fn!(i32_ge_s);
    invalid_constant_fn!(i32_ge_u);

    // Numeric instructions - i64 test/rel
    invalid_constant_fn!(i64_eqz);
    invalid_constant_fn!(i64_eq);
    invalid_constant_fn!(i64_ne);
    invalid_constant_fn!(i64_lt_s);
    invalid_constant_fn!(i64_lt_u);
    invalid_constant_fn!(i64_gt_s);
    invalid_constant_fn!(i64_gt_u);
    invalid_constant_fn!(i64_le_s);
    invalid_constant_fn!(i64_le_u);
    invalid_constant_fn!(i64_ge_s);
    invalid_constant_fn!(i64_ge_u);

    // Numeric instructions - f32 rel
    invalid_constant_fn!(f32_eq);
    invalid_constant_fn!(f32_ne);
    invalid_constant_fn!(f32_lt);
    invalid_constant_fn!(f32_gt);
    invalid_constant_fn!(f32_le);
    invalid_constant_fn!(f32_ge);

    // Numeric instructions - f64 rel
    invalid_constant_fn!(f64_eq);
    invalid_constant_fn!(f64_ne);
    invalid_constant_fn!(f64_lt);
    invalid_constant_fn!(f64_gt);
    invalid_constant_fn!(f64_le);
    invalid_constant_fn!(f64_ge);

    // Numeric instructions - i32 unary/binary
    invalid_constant_fn!(i32_clz);
    invalid_constant_fn!(i32_ctz);
    invalid_constant_fn!(i32_popcnt);
    invalid_constant_fn!(i32_add);
    invalid_constant_fn!(i32_sub);
    invalid_constant_fn!(i32_mul);
    invalid_constant_fn!(i32_div_s);
    invalid_constant_fn!(i32_div_u);
    invalid_constant_fn!(i32_rem_s);
    invalid_constant_fn!(i32_rem_u);
    invalid_constant_fn!(i32_and);
    invalid_constant_fn!(i32_or);
    invalid_constant_fn!(i32_xor);
    invalid_constant_fn!(i32_shl);
    invalid_constant_fn!(i32_shr_s);
    invalid_constant_fn!(i32_shr_u);
    invalid_constant_fn!(i32_rotl);
    invalid_constant_fn!(i32_rotr);

    // Numeric instructions - i64 unary/binary
    invalid_constant_fn!(i64_clz);
    invalid_constant_fn!(i64_ctz);
    invalid_constant_fn!(i64_popcnt);
    invalid_constant_fn!(i64_add);
    invalid_constant_fn!(i64_sub);
    invalid_constant_fn!(i64_mul);
    invalid_constant_fn!(i64_div_s);
    invalid_constant_fn!(i64_div_u);
    invalid_constant_fn!(i64_rem_s);
    invalid_constant_fn!(i64_rem_u);
    invalid_constant_fn!(i64_and);
    invalid_constant_fn!(i64_or);
    invalid_constant_fn!(i64_xor);
    invalid_constant_fn!(i64_shl);
    invalid_constant_fn!(i64_shr_s);
    invalid_constant_fn!(i64_shr_u);
    invalid_constant_fn!(i64_rotl);
    invalid_constant_fn!(i64_rotr);

    // Numeric instructions - f32 unary/binary
    invalid_constant_fn!(f32_abs);
    invalid_constant_fn!(f32_neg);
    invalid_constant_fn!(f32_ceil);
    invalid_constant_fn!(f32_floor);
    invalid_constant_fn!(f32_trunc);
    invalid_constant_fn!(f32_nearest);
    invalid_constant_fn!(f32_sqrt);
    invalid_constant_fn!(f32_add);
    invalid_constant_fn!(f32_sub);
    invalid_constant_fn!(f32_mul);
    invalid_constant_fn!(f32_div);
    invalid_constant_fn!(f32_min);
    invalid_constant_fn!(f32_max);
    invalid_constant_fn!(f32_copysign);

    // Numeric instructions - f64 unary/binary
    invalid_constant_fn!(f64_abs);
    invalid_constant_fn!(f64_neg);
    invalid_constant_fn!(f64_ceil);
    invalid_constant_fn!(f64_floor);
    invalid_constant_fn!(f64_trunc);
    invalid_constant_fn!(f64_nearest);
    invalid_constant_fn!(f64_sqrt);
    invalid_constant_fn!(f64_add);
    invalid_constant_fn!(f64_sub);
    invalid_constant_fn!(f64_mul);
    invalid_constant_fn!(f64_div);
    invalid_constant_fn!(f64_min);
    invalid_constant_fn!(f64_max);
    invalid_constant_fn!(f64_copysign);

    // Numeric instructions - conversions
    invalid_constant_fn!(i32_wrap_i64);
    invalid_constant_fn!(i32_trunc_f32_s);
    invalid_constant_fn!(i32_trunc_f32_u);
    invalid_constant_fn!(i32_trunc_f64_s);
    invalid_constant_fn!(i32_trunc_f64_u);
    invalid_constant_fn!(i64_extend_i32_s);
    invalid_constant_fn!(i64_extend_i32_u);
    invalid_constant_fn!(i64_trunc_f32_s);
    invalid_constant_fn!(i64_trunc_f32_u);
    invalid_constant_fn!(i64_trunc_f64_s);
    invalid_constant_fn!(i64_trunc_f64_u);
    invalid_constant_fn!(f32_convert_i32_s);
    invalid_constant_fn!(f32_convert_i32_u);
    invalid_constant_fn!(f32_convert_i64_s);
    invalid_constant_fn!(f32_convert_i64_u);
    invalid_constant_fn!(f32_demote_f64);
    invalid_constant_fn!(f64_convert_i32_s);
    invalid_constant_fn!(f64_convert_i32_u);
    invalid_constant_fn!(f64_convert_i64_s);
    invalid_constant_fn!(f64_convert_i64_u);
    invalid_constant_fn!(f64_promote_f32);
    invalid_constant_fn!(i32_reinterpret_f32);
    invalid_constant_fn!(i64_reinterpret_f64);
    invalid_constant_fn!(f32_reinterpret_i32);
    invalid_constant_fn!(f64_reinterpret_i64);
}

impl WasmVisitor for ConstantCompiler {
    // Exit the expression
    fn finish(&self, state: &mut Self::State) -> Result<(), Self::Error> {
        if let Some(_) = state {
            Ok(())
        } else {
            Err(ConstantExprError::NoValue)
        }
    }

    invalid_constant_fn!(enter_block, block_type: ResultType);
    invalid_constant_fn!(exit_block);
    invalid_constant_fn!(loop_, block_type: ResultType);
    invalid_constant_fn!(if_, block_type: ResultType);
    invalid_constant_fn!(else_);
    invalid_constant_fn!(br, l: LabelIdx);
    invalid_constant_fn!(br_if, l: LabelIdx);
    invalid_constant_fn!(br_table, lut: &[LabelIdx], default_: LabelIdx);

    invalid_constant_fn!(return_);
    invalid_constant_fn!(call, x: FuncIdx);
    invalid_constant_fn!(call_indirect, x: TypeIdx);

    // Variable instructions
    invalid_constant_fn!(local_get, x: LocalIdx);
    invalid_constant_fn!(local_set, x: LocalIdx);
    invalid_constant_fn!(local_tee, x: LocalIdx);
    invalid_constant_fn!(global_get, x: GlobalIdx);
    invalid_constant_fn!(global_set, x: GlobalIdx);
}
