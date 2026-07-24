use crate::{
    FuncIdx, GlobalIdx, LabelIdx, LabelTarget, LocalIdx, MemArg, ResultType, TypeIdx, ValType,
};

/// A convenience macro for defining the visitor function for a decoded
/// WebAssembly instruction from any intermediate representation.
macro_rules! visit_fn {
    // No additional parameters
    ($name:ident) => {
        fn $name(&self, state: &mut Self::State) -> Result<(), Self::Error>;
    };

    // With additional parameters
    ($name:ident, $($param:ident : $ty:ty),+) => {
        fn $name(&self, $($param: $ty),+, state: &mut Self::State) -> Result<(), Self::Error>;
    };
}

/// An abstraction over Wasm IR and internal IR.
/// This trait can be used to index, compile and execute either form of IR
/// with the same common implementation. The decoding and traversal code will
/// call into this visitor and is IR specific. This trait is purely for operating
/// on decoded WebAssembly instructions.
///
/// Note: This visitor does not handle the control-flow instructions since those are
///       IR-specific. See [WasmVisitor] and [IrVisitor]
pub trait BaseVisitor {
    type Error;
    type State;

    // Control instructions
    visit_fn!(unreachable);
    visit_fn!(nop);

    // Control flow & parametric instructions are not handled by the base visitor

    // Memory instructions - loads
    visit_fn!(i32_load, m: MemArg);
    visit_fn!(i64_load, m: MemArg);
    visit_fn!(f32_load, m: MemArg);
    visit_fn!(f64_load, m: MemArg);
    visit_fn!(i32_load8_s, m: MemArg);
    visit_fn!(i32_load8_u, m: MemArg);
    visit_fn!(i32_load16_s, m: MemArg);
    visit_fn!(i32_load16_u, m: MemArg);
    visit_fn!(i64_load8_s, m: MemArg);
    visit_fn!(i64_load8_u, m: MemArg);
    visit_fn!(i64_load16_s, m: MemArg);
    visit_fn!(i64_load16_u, m: MemArg);
    visit_fn!(i64_load32_s, m: MemArg);
    visit_fn!(i64_load32_u, m: MemArg);

    // Memory instructions - stores
    visit_fn!(i32_store, m: MemArg);
    visit_fn!(i64_store, m: MemArg);
    visit_fn!(f32_store, m: MemArg);
    visit_fn!(f64_store, m: MemArg);
    visit_fn!(i32_store8, m: MemArg);
    visit_fn!(i32_store16, m: MemArg);
    visit_fn!(i64_store8, m: MemArg);
    visit_fn!(i64_store16, m: MemArg);
    visit_fn!(i64_store32, m: MemArg);

    // Memory instructions - size/grow
    visit_fn!(memory_size);
    visit_fn!(memory_grow);

    // Numeric instructions - const
    visit_fn!(i32_const, n: i32);
    visit_fn!(i64_const, n: i64);
    visit_fn!(f32_const, z: f32);
    visit_fn!(f64_const, z: f64);

    // Numeric instructions - i32 test/rel
    visit_fn!(i32_eqz);
    visit_fn!(i32_eq);
    visit_fn!(i32_ne);
    visit_fn!(i32_lt_s);
    visit_fn!(i32_lt_u);
    visit_fn!(i32_gt_s);
    visit_fn!(i32_gt_u);
    visit_fn!(i32_le_s);
    visit_fn!(i32_le_u);
    visit_fn!(i32_ge_s);
    visit_fn!(i32_ge_u);

    // Numeric instructions - i64 test/rel
    visit_fn!(i64_eqz);
    visit_fn!(i64_eq);
    visit_fn!(i64_ne);
    visit_fn!(i64_lt_s);
    visit_fn!(i64_lt_u);
    visit_fn!(i64_gt_s);
    visit_fn!(i64_gt_u);
    visit_fn!(i64_le_s);
    visit_fn!(i64_le_u);
    visit_fn!(i64_ge_s);
    visit_fn!(i64_ge_u);

    // Numeric instructions - f32 rel
    visit_fn!(f32_eq);
    visit_fn!(f32_ne);
    visit_fn!(f32_lt);
    visit_fn!(f32_gt);
    visit_fn!(f32_le);
    visit_fn!(f32_ge);

    // Numeric instructions - f64 rel
    visit_fn!(f64_eq);
    visit_fn!(f64_ne);
    visit_fn!(f64_lt);
    visit_fn!(f64_gt);
    visit_fn!(f64_le);
    visit_fn!(f64_ge);

    // Numeric instructions - i32 unary/binary
    visit_fn!(i32_clz);
    visit_fn!(i32_ctz);
    visit_fn!(i32_popcnt);
    visit_fn!(i32_add);
    visit_fn!(i32_sub);
    visit_fn!(i32_mul);
    visit_fn!(i32_div_s);
    visit_fn!(i32_div_u);
    visit_fn!(i32_rem_s);
    visit_fn!(i32_rem_u);
    visit_fn!(i32_and);
    visit_fn!(i32_or);
    visit_fn!(i32_xor);
    visit_fn!(i32_shl);
    visit_fn!(i32_shr_s);
    visit_fn!(i32_shr_u);
    visit_fn!(i32_rotl);
    visit_fn!(i32_rotr);

    // Numeric instructions - i64 unary/binary
    visit_fn!(i64_clz);
    visit_fn!(i64_ctz);
    visit_fn!(i64_popcnt);
    visit_fn!(i64_add);
    visit_fn!(i64_sub);
    visit_fn!(i64_mul);
    visit_fn!(i64_div_s);
    visit_fn!(i64_div_u);
    visit_fn!(i64_rem_s);
    visit_fn!(i64_rem_u);
    visit_fn!(i64_and);
    visit_fn!(i64_or);
    visit_fn!(i64_xor);
    visit_fn!(i64_shl);
    visit_fn!(i64_shr_s);
    visit_fn!(i64_shr_u);
    visit_fn!(i64_rotl);
    visit_fn!(i64_rotr);

    // Numeric instructions - f32 unary/binary
    visit_fn!(f32_abs);
    visit_fn!(f32_neg);
    visit_fn!(f32_ceil);
    visit_fn!(f32_floor);
    visit_fn!(f32_trunc);
    visit_fn!(f32_nearest);
    visit_fn!(f32_sqrt);
    visit_fn!(f32_add);
    visit_fn!(f32_sub);
    visit_fn!(f32_mul);
    visit_fn!(f32_div);
    visit_fn!(f32_min);
    visit_fn!(f32_max);
    visit_fn!(f32_copysign);

    // Numeric instructions - f64 unary/binary
    visit_fn!(f64_abs);
    visit_fn!(f64_neg);
    visit_fn!(f64_ceil);
    visit_fn!(f64_floor);
    visit_fn!(f64_trunc);
    visit_fn!(f64_nearest);
    visit_fn!(f64_sqrt);
    visit_fn!(f64_add);
    visit_fn!(f64_sub);
    visit_fn!(f64_mul);
    visit_fn!(f64_div);
    visit_fn!(f64_min);
    visit_fn!(f64_max);
    visit_fn!(f64_copysign);

    // Numeric instructions - conversions
    visit_fn!(i32_wrap_i64);
    visit_fn!(i32_extend8_s);
    visit_fn!(i32_extend16_s);
    visit_fn!(i64_extend8_s);
    visit_fn!(i64_extend16_s);
    visit_fn!(i64_extend32_s);
    visit_fn!(i32_trunc_f32_s);
    visit_fn!(i32_trunc_f32_u);
    visit_fn!(i32_trunc_f64_s);
    visit_fn!(i32_trunc_f64_u);
    visit_fn!(i64_extend_i32_s);
    visit_fn!(i64_extend_i32_u);
    visit_fn!(i64_trunc_f32_s);
    visit_fn!(i64_trunc_f32_u);
    visit_fn!(i64_trunc_f64_s);
    visit_fn!(i64_trunc_f64_u);
    visit_fn!(f32_convert_i32_s);
    visit_fn!(f32_convert_i32_u);
    visit_fn!(f32_convert_i64_s);
    visit_fn!(f32_convert_i64_u);
    visit_fn!(f32_demote_f64);
    visit_fn!(f64_convert_i32_s);
    visit_fn!(f64_convert_i32_u);
    visit_fn!(f64_convert_i64_s);
    visit_fn!(f64_convert_i64_u);
    visit_fn!(f64_promote_f32);
}

/// An abstraction over Wasm Bytecode.
/// Used to implement validation and compilation of Wasm bytecode.
pub trait WasmVisitor: BaseVisitor {
    // Parametric instructions
    visit_fn!(drop);
    visit_fn!(select);

    visit_fn!(enter_block, block_type: ResultType);
    visit_fn!(exit_block);
    visit_fn!(finish);
    visit_fn!(loop_, block_type: ResultType);
    visit_fn!(if_, block_type: ResultType);
    visit_fn!(else_);
    visit_fn!(br, l: LabelIdx);
    visit_fn!(br_if, l: LabelIdx);
    visit_fn!(br_table_start, len: u32);
    visit_fn!(br_table_branch, br: LabelIdx);
    visit_fn!(br_table_finish, default_: LabelIdx);

    visit_fn!(return_);
    visit_fn!(call, x: FuncIdx);
    visit_fn!(call_indirect, x: TypeIdx);

    // Variable instructions
    visit_fn!(local_get, x: LocalIdx);
    visit_fn!(local_set, x: LocalIdx);
    visit_fn!(local_tee, x: LocalIdx);
    visit_fn!(global_get, x: GlobalIdx);
    visit_fn!(global_set, x: GlobalIdx);

    // Validation instructions
    visit_fn!(i32_reinterpret_f32);
    visit_fn!(i64_reinterpret_f64);
    visit_fn!(f32_reinterpret_i32);
    visit_fn!(f64_reinterpret_i64);
}

#[derive(Debug)]
pub struct LocalVariable {
    // Offset of the local variable in 32-bit words
    // Function parameters are negative relative to the FP
    // Locals are positive relative to FP + 2
    pub frame_offset: i16,

    // Variable's type
    pub ty: ValType,
}

/// An index offset from the current module.
/// Module imports can only refer to modules loaded before it.
/// References store a relative offset from the 'current' module. 0 indicates the current module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModuleRef(pub u8);

#[derive(Debug, Clone, Copy)]
pub struct HostModuleRef(pub u8);

impl HostModuleRef {
    /// Construct a new module reference given the absolute index to the module and the store.
    pub fn new(module_index: usize) -> HostModuleRef {
        HostModuleRef(module_index as u8)
    }
}

/// A reference to a symbol in the Wasm store relative to an implicit 'self' module
#[derive(Debug, Clone, Copy)]
pub enum Ref {
    /// A symbol in the current Wasm module
    Module(u16),
    /// A symbol in an external host module
    Host { module: HostModuleRef, index: u16 },
    /// A symbol in another Wasm module
    Extern { module: ModuleRef, index: u16 },
}

/// An abstraction over IR Bytecode.
/// Used to implement the interpreter.
pub trait IrVisitor: BaseVisitor {
    // Parametric instructions
    visit_fn!(drop, ty: ValType);
    visit_fn!(select, ty: ValType);

    visit_fn!(if_, false_address: LabelTarget);
    visit_fn!(br, addr: LabelTarget);
    visit_fn!(br_if, true_address: LabelTarget);
    visit_fn!(br_table, n: u32, cases: impl FnOnce(u32) -> LabelTarget);

    visit_fn!(return_, return_size: u8);
    visit_fn!(call, x: u16);
    visit_fn!(call_host, module: HostModuleRef, x: u16);
    visit_fn!(call_extern, module: ModuleRef, x: u16);
    visit_fn!(call_indirect, x: TypeIdx);

    // Variable instructions
    visit_fn!(local_get, l: LocalVariable);
    visit_fn!(local_set, l: LocalVariable);
    visit_fn!(local_tee, l: LocalVariable);
    visit_fn!(global_get, idx: u16);
    visit_fn!(global_set, idx: u16);
    visit_fn!(global_get_host, module: HostModuleRef, index: u16);
    visit_fn!(global_set_host, module: HostModuleRef, index: u16);
    visit_fn!(global_get_extern, module: ModuleRef, index: u16);
    visit_fn!(global_set_extern, module: ModuleRef, index: u16);
}
