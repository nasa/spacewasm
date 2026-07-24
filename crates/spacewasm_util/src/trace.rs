use spacewasm::{
    BaseVisitor, Engine, HostModuleRef, IrVisitor, JumpTarget, LabelTarget, LocalVariable, MemArg,
    ModuleRef, TypeIdx, ValType,
};

/// A snapshot of interpreter state at a specific instruction
#[derive(Debug, Clone)]
pub struct StateSnapshot {
    pub pc: JumpTarget,
    pub sp: usize,
    pub fp: u32,
    pub instruction: &'static str,
    pub metadata: Option<(&'static str, usize)>,
}

/// Circular buffer for tracking recent state snapshots
pub struct StateHistory {
    snapshots: std::vec::Vec<StateSnapshot>,
    capacity: usize,
    index: usize,
    count: usize,
}

impl StateHistory {
    pub fn new(capacity: usize) -> Self {
        Self {
            snapshots: std::vec::Vec::with_capacity(capacity),
            capacity,
            index: 0,
            count: 0,
        }
    }

    pub fn record(&mut self, snapshot: StateSnapshot) {
        if self.snapshots.len() < self.capacity {
            self.snapshots.push(snapshot);
        } else {
            self.snapshots[self.index] = snapshot;
        }
        self.index = (self.index + 1) % self.capacity;
        self.count = self.count.saturating_add(1);
    }

    /// Get snapshots in chronological order (oldest to newest)
    pub fn iter(&self) -> std::boxed::Box<dyn Iterator<Item = &StateSnapshot> + '_> {
        if self.count < self.capacity {
            // Haven't wrapped yet, just iterate from start
            std::boxed::Box::new(self.snapshots.iter())
        } else {
            // Wrapped, start from current index (oldest)
            std::boxed::Box::new(
                self.snapshots[self.index..]
                    .iter()
                    .chain(self.snapshots[..self.index].iter()),
            )
        }
    }

    pub fn dump(&self) -> String {
        use core::fmt::Write;
        let mut s = String::new();
        let _ = writeln!(
            &mut s,
            "\n=== Execution Trace (last {} instructions) ===",
            self.snapshots.len()
        );
        for (i, snap) in self.iter().enumerate() {
            if let Some(md) = snap.metadata {
                let _ = writeln!(
                    &mut s,
                    "#{:4} pc={:5} sp={:4} fp={:4} | {} {}={}",
                    i, snap.pc.0, snap.sp, snap.fp, snap.instruction, md.0, md.1
                );
            } else {
                let _ = writeln!(
                    &mut s,
                    "#{:4} pc={:5} sp={:4} fp={:4} | {}",
                    i, snap.pc.0, snap.sp, snap.fp, snap.instruction
                );
            }
        }
        let _ = writeln!(&mut s, "===============================================");
        s
    }
}

macro_rules! trace_visit_fn {
    // No additional parameters
    ($name:ident) => {
        fn $name(&self, state: &mut Self::State) -> Result<(), Self::Error> {
            self.record_state(state, stringify!($name));
            self.v.$name(state)
        }
    };

    // With additional parameters
    ($name:ident, $($param:ident : $ty:ty),+) => {
        fn $name(&self, $($param: $ty),+, state: &mut Self::State) -> Result<(), Self::Error> {
            self.record_state(state, stringify!($name));
            self.v.$name($($param,)+ state)
        }
    };
}

/// State tracer that wraps an IrVisitor and records pc/sp/fp history
pub struct StateTracer<'a, T: BaseVisitor<State = Engine, Error = E>, E> {
    pub v: &'a T,
    pub history: core::cell::RefCell<StateHistory>,
}

impl<'a, T: BaseVisitor<State = Engine, Error = E>, E> StateTracer<'a, T, E> {
    pub fn new(v: &'a T, capacity: usize) -> Self {
        Self {
            v,
            history: core::cell::RefCell::new(StateHistory::new(capacity)),
        }
    }

    fn record_state(&self, state: &Engine, instruction: &'static str) {
        self.history.borrow_mut().record(StateSnapshot {
            pc: state.pc,
            sp: state.sp,
            fp: state.fp,
            instruction,
            metadata: None,
        });
    }

    fn record_state_with_metadata(
        &self,
        state: &Engine,
        instruction: &'static str,
        meta_name: &'static str,
        meta_value: usize,
    ) {
        self.history.borrow_mut().record(StateSnapshot {
            pc: state.pc,
            sp: state.sp,
            fp: state.fp,
            instruction,
            metadata: Some((meta_name, meta_value)),
        });
    }

    pub fn dump_history(&self) -> String {
        self.history.borrow().dump()
    }

    pub fn clear(&self) {
        self.history.borrow_mut().count = 0;
        self.history.borrow_mut().index = 0;
        self.history.borrow_mut().snapshots.clear();
    }
}

impl<'a, T: BaseVisitor<State = Engine, Error = E>, E> BaseVisitor for StateTracer<'a, T, E> {
    type Error = E;
    type State = Engine;

    // Control instructions
    trace_visit_fn!(unreachable);
    trace_visit_fn!(nop);

    // Memory instructions - loads
    trace_visit_fn!(i32_load, m: MemArg);
    trace_visit_fn!(i64_load, m: MemArg);
    trace_visit_fn!(f32_load, m: MemArg);
    trace_visit_fn!(f64_load, m: MemArg);
    trace_visit_fn!(i32_load8_s, m: MemArg);
    trace_visit_fn!(i32_load8_u, m: MemArg);
    trace_visit_fn!(i32_load16_s, m: MemArg);
    trace_visit_fn!(i32_load16_u, m: MemArg);
    trace_visit_fn!(i64_load8_s, m: MemArg);
    trace_visit_fn!(i64_load8_u, m: MemArg);
    trace_visit_fn!(i64_load16_s, m: MemArg);
    trace_visit_fn!(i64_load16_u, m: MemArg);
    trace_visit_fn!(i64_load32_s, m: MemArg);
    trace_visit_fn!(i64_load32_u, m: MemArg);

    // Memory instructions - stores
    trace_visit_fn!(i32_store, m: MemArg);
    trace_visit_fn!(i64_store, m: MemArg);
    trace_visit_fn!(f32_store, m: MemArg);
    trace_visit_fn!(f64_store, m: MemArg);
    trace_visit_fn!(i32_store8, m: MemArg);
    trace_visit_fn!(i32_store16, m: MemArg);
    trace_visit_fn!(i64_store8, m: MemArg);
    trace_visit_fn!(i64_store16, m: MemArg);
    trace_visit_fn!(i64_store32, m: MemArg);

    // Memory instructions - size/grow
    trace_visit_fn!(memory_size);
    trace_visit_fn!(memory_grow);

    // Numeric instructions - const
    trace_visit_fn!(i32_const, n: i32);
    trace_visit_fn!(i64_const, n: i64);
    trace_visit_fn!(f32_const, z: f32);
    trace_visit_fn!(f64_const, z: f64);

    // Numeric instructions - i32 test/rel
    trace_visit_fn!(i32_eqz);
    trace_visit_fn!(i32_eq);
    trace_visit_fn!(i32_ne);
    trace_visit_fn!(i32_lt_s);
    trace_visit_fn!(i32_lt_u);
    trace_visit_fn!(i32_gt_s);
    trace_visit_fn!(i32_gt_u);
    trace_visit_fn!(i32_le_s);
    trace_visit_fn!(i32_le_u);
    trace_visit_fn!(i32_ge_s);
    trace_visit_fn!(i32_ge_u);

    // Numeric instructions - i64 test/rel
    trace_visit_fn!(i64_eqz);
    trace_visit_fn!(i64_eq);
    trace_visit_fn!(i64_ne);
    trace_visit_fn!(i64_lt_s);
    trace_visit_fn!(i64_lt_u);
    trace_visit_fn!(i64_gt_s);
    trace_visit_fn!(i64_gt_u);
    trace_visit_fn!(i64_le_s);
    trace_visit_fn!(i64_le_u);
    trace_visit_fn!(i64_ge_s);
    trace_visit_fn!(i64_ge_u);

    // Numeric instructions - f32 rel
    trace_visit_fn!(f32_eq);
    trace_visit_fn!(f32_ne);
    trace_visit_fn!(f32_lt);
    trace_visit_fn!(f32_gt);
    trace_visit_fn!(f32_le);
    trace_visit_fn!(f32_ge);

    // Numeric instructions - f64 rel
    trace_visit_fn!(f64_eq);
    trace_visit_fn!(f64_ne);
    trace_visit_fn!(f64_lt);
    trace_visit_fn!(f64_gt);
    trace_visit_fn!(f64_le);
    trace_visit_fn!(f64_ge);

    // Numeric instructions - i32 unary/binary
    trace_visit_fn!(i32_clz);
    trace_visit_fn!(i32_ctz);
    trace_visit_fn!(i32_popcnt);
    trace_visit_fn!(i32_add);
    trace_visit_fn!(i32_sub);
    trace_visit_fn!(i32_mul);
    trace_visit_fn!(i32_div_s);
    trace_visit_fn!(i32_div_u);
    trace_visit_fn!(i32_rem_s);
    trace_visit_fn!(i32_rem_u);
    trace_visit_fn!(i32_and);
    trace_visit_fn!(i32_or);
    trace_visit_fn!(i32_xor);
    trace_visit_fn!(i32_shl);
    trace_visit_fn!(i32_shr_s);
    trace_visit_fn!(i32_shr_u);
    trace_visit_fn!(i32_rotl);
    trace_visit_fn!(i32_rotr);

    // Numeric instructions - i64 unary/binary
    trace_visit_fn!(i64_clz);
    trace_visit_fn!(i64_ctz);
    trace_visit_fn!(i64_popcnt);
    trace_visit_fn!(i64_add);
    trace_visit_fn!(i64_sub);
    trace_visit_fn!(i64_mul);
    trace_visit_fn!(i64_div_s);
    trace_visit_fn!(i64_div_u);
    trace_visit_fn!(i64_rem_s);
    trace_visit_fn!(i64_rem_u);
    trace_visit_fn!(i64_and);
    trace_visit_fn!(i64_or);
    trace_visit_fn!(i64_xor);
    trace_visit_fn!(i64_shl);
    trace_visit_fn!(i64_shr_s);
    trace_visit_fn!(i64_shr_u);
    trace_visit_fn!(i64_rotl);
    trace_visit_fn!(i64_rotr);

    // Numeric instructions - f32 unary/binary
    trace_visit_fn!(f32_abs);
    trace_visit_fn!(f32_neg);
    trace_visit_fn!(f32_ceil);
    trace_visit_fn!(f32_floor);
    trace_visit_fn!(f32_trunc);
    trace_visit_fn!(f32_nearest);
    trace_visit_fn!(f32_sqrt);
    trace_visit_fn!(f32_add);
    trace_visit_fn!(f32_sub);
    trace_visit_fn!(f32_mul);
    trace_visit_fn!(f32_div);
    trace_visit_fn!(f32_min);
    trace_visit_fn!(f32_max);
    trace_visit_fn!(f32_copysign);

    // Numeric instructions - f64 unary/binary
    trace_visit_fn!(f64_abs);
    trace_visit_fn!(f64_neg);
    trace_visit_fn!(f64_ceil);
    trace_visit_fn!(f64_floor);
    trace_visit_fn!(f64_trunc);
    trace_visit_fn!(f64_nearest);
    trace_visit_fn!(f64_sqrt);
    trace_visit_fn!(f64_add);
    trace_visit_fn!(f64_sub);
    trace_visit_fn!(f64_mul);
    trace_visit_fn!(f64_div);
    trace_visit_fn!(f64_min);
    trace_visit_fn!(f64_max);
    trace_visit_fn!(f64_copysign);

    // Numeric instructions - conversions
    trace_visit_fn!(i32_wrap_i64);
    trace_visit_fn!(i32_extend8_s);
    trace_visit_fn!(i32_extend16_s);
    trace_visit_fn!(i64_extend8_s);
    trace_visit_fn!(i64_extend16_s);
    trace_visit_fn!(i64_extend32_s);
    trace_visit_fn!(i32_trunc_f32_s);
    trace_visit_fn!(i32_trunc_f32_u);
    trace_visit_fn!(i32_trunc_f64_s);
    trace_visit_fn!(i32_trunc_f64_u);
    trace_visit_fn!(i64_extend_i32_s);
    trace_visit_fn!(i64_extend_i32_u);
    trace_visit_fn!(i64_trunc_f32_s);
    trace_visit_fn!(i64_trunc_f32_u);
    trace_visit_fn!(i64_trunc_f64_s);
    trace_visit_fn!(i64_trunc_f64_u);
    trace_visit_fn!(f32_convert_i32_s);
    trace_visit_fn!(f32_convert_i32_u);
    trace_visit_fn!(f32_convert_i64_s);
    trace_visit_fn!(f32_convert_i64_u);
    trace_visit_fn!(f32_demote_f64);
    trace_visit_fn!(f64_convert_i32_s);
    trace_visit_fn!(f64_convert_i32_u);
    trace_visit_fn!(f64_convert_i64_s);
    trace_visit_fn!(f64_convert_i64_u);
    trace_visit_fn!(f64_promote_f32);
}

impl<'a, T, E> IrVisitor for StateTracer<'a, T, E>
where
    T: BaseVisitor<State = Engine, Error = E> + IrVisitor,
{
    // Parametric instructions
    trace_visit_fn!(drop, ty: ValType);
    trace_visit_fn!(select, ty: ValType);

    trace_visit_fn!(if_, false_address: LabelTarget);
    trace_visit_fn!(br, addr: LabelTarget);
    trace_visit_fn!(br_if, true_address: LabelTarget);

    fn br_table(
        &self,
        n: u32,
        cases: impl FnOnce(u32) -> LabelTarget,
        state: &mut Self::State,
    ) -> Result<(), Self::Error> {
        self.record_state(state, "br_table");
        self.v.br_table(n, cases, state)
    }

    trace_visit_fn!(return_, return_size: u8);
    fn call(&self, x: u16, state: &mut Self::State) -> Result<(), Self::Error> {
        let m = &state.store.modules()[state.module.0 as usize];
        let f = &m.functions[x as usize];

        self.record_state_with_metadata(
            state,
            stringify!(call),
            "stack_usage",
            f.stack_usage as usize,
        );
        self.v.call(x, state)
    }
    trace_visit_fn!(call_host, module: HostModuleRef, x: u16);
    trace_visit_fn!(call_extern, module: ModuleRef, x: u16);
    trace_visit_fn!(call_indirect, x: TypeIdx);

    // Variable instructions
    trace_visit_fn!(local_get, l: LocalVariable);
    trace_visit_fn!(local_set, l: LocalVariable);
    trace_visit_fn!(local_tee, l: LocalVariable);
    trace_visit_fn!(global_get, idx: u16);
    trace_visit_fn!(global_set, idx: u16);
    trace_visit_fn!(global_get_host, module: HostModuleRef, index: u16);
    trace_visit_fn!(global_set_host, module: HostModuleRef, index: u16);
    trace_visit_fn!(global_get_extern, module: ModuleRef, index: u16);
    trace_visit_fn!(global_set_extern, module: ModuleRef, index: u16);
}
