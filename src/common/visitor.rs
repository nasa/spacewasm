use crate::{FuncIdx, GlobalIdx, LabelIdx, LocalIdx, MemArg, ResultType, TypeIdx, ValidationError};

/// A convenience macro for defining the visitor function for a decoded
/// WebAssembly instruction from any intermediate representation.
/// FIXME(tumbar) This visitor currently depends on [WasmReader] which is
///               incorrect since the we should not be dependent on the IR type.
///               It is likely we can use a generic to represent the PC just pass in [&mut PC]
///               but I'll need to revisit this once we have more than one instruction
///               representation.
macro_rules! visitor_default_impl {
    // No additional parameters
    ($name:ident) => {
        fn $name(&self, state: &mut Self::State) -> Result<(), Self::Error> {
            let _ = state;
            Ok(())
        }
    };

    // With additional parameters
    ($name:ident, $($param:ident : $ty:ty),+) => {
        fn $name(&self, $($param: $ty),+, state: &mut Self::State) -> Result<(), Self::Error> {
            let _ = state;
            $(let _ = $param;)+
            Ok(())
        }
    };
}

/// An abstraction over WASM IR and internal IR.
/// This trait can be used to index, compile and execute either form of IR
/// with the same common implementation. The decoding and traversal code will
/// call into this visitor and is IR specific. This trait is purely for operating
/// on decoded WebAssembly instructions.
pub trait CodeVisitor {
    type Error: Into<ValidationError>;
    type State;

    // Control instructions
    visitor_default_impl!(unreachable);
    visitor_default_impl!(nop);
    visitor_default_impl!(enter_block, block_type: ResultType);
    visitor_default_impl!(exit_block);
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
