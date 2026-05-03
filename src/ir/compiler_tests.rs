#[cfg(test)]
mod tests {
    use crate::{
        BaseVisitor, Code, CodeBuilder, Compiler, GlobalVariable, IrVisitor, JumpTarget,
        LocalVariable, MemArg, Module, ModuleImports, TextBuilder, TypeIdx,
    };
    use core::cell::Cell;

    extern crate std;

    /// Helper to create a minimal module for testing
    fn create_test_module() -> Module<'static> {
        Module {
            types: crate::Vec::zero(),
            functions: crate::Vec::zero(),
            tables: crate::Vec::zero(),
            memories: crate::Vec::zero(),
            globals: crate::Vec::zero(),
            elements: crate::Vec::zero(),
            data: crate::Vec::zero(),
            start: None,
            imports: crate::Vec::zero(),
            exports: crate::Vec::zero(),
            text: crate::Vec::zero(),
            wasm_size: 0,
            final_page_offset: 0,
            memory_usage: Default::default(),
            module_imports: ModuleImports {
                functions: &[],
                memories: &[],
                globals: &[],
            },
        }
    }

    /// Helper to create a minimal function for testing
    fn create_test_func() -> crate::Func {
        crate::Func {
            ty: TypeIdx(0),
            stack_usage: 0,
            local_size: 0,
            parameter_size: 0,
            return_size: 0,
            locals: crate::Vec::zero(),
            expr: crate::Expr(JumpTarget(0)),
        }
    }

    /// Macro to generate TestHandler methods with panic defaults
    macro_rules! handler_methods {
        // Methods with no parameters
        (no_param: $($name:ident),* $(,)?) => {
            $(
                fn $name(&self, _: &mut Self::State) -> Result<(), ()> {
                    panic!(concat!("Unexpected: ", stringify!($name)))
                }
            )*
        };

        // Methods with one typed parameter
        (one_param: $(($name:ident, $ty:ty)),* $(,)?) => {
            $(
                fn $name(&self, _: $ty, _: &mut Self::State) -> Result<(), ()> {
                    panic!(concat!("Unexpected: ", stringify!($name)))
                }
            )*
        };

        // Special case for br_table with closure
        (br_table) => {
            fn handle_br_table(
                &self,
                _: impl FnOnce(u16) -> Result<JumpTarget, ()>,
                _: &mut Self::State,
            ) -> Result<(), ()> {
                panic!("Unexpected: br_table")
            }
        };
    }

    /// Macro to generate delegation methods in TestVisitor
    macro_rules! delegate_methods {
        // Methods with no parameters
        (no_param: $($visitor_name:ident => $handler_name:ident),* $(,)?) => {
            $(
                fn $visitor_name(&self, state: &mut Self::State) -> Result<(), Self::Error> {
                    self.0.$handler_name(state)
                }
            )*
        };

        // Methods with one typed parameter
        (one_param: $(($visitor_name:ident => $handler_name:ident, $param_name:ident: $ty:ty)),* $(,)?) => {
            $(
                fn $visitor_name(&self, $param_name: $ty, state: &mut Self::State) -> Result<(), Self::Error> {
                    self.0.$handler_name($param_name, state)
                }
            )*
        };

        // Special case for br_table
        (br_table) => {
            fn br_table(
                &self,
                f: impl FnOnce(u16) -> Result<JumpTarget, ()>,
                state: &mut Self::State,
            ) -> Result<(), Self::Error> {
                self.0.handle_br_table(f, state)
            }
        };
    }

    /// Handler trait with panic defaults for all BaseVisitor methods
    /// Tests override only the methods they need to test
    trait TestHandler {
        type State;

        // Control flow and parametric (no params)
        handler_methods!(no_param:
            handle_finish, handle_unreachable, handle_nop, handle_drop, handle_select,
        );

        // Memory loads (MemArg param)
        handler_methods!(one_param:
            (handle_i32_load, MemArg), (handle_i64_load, MemArg), (handle_f32_load, MemArg), (handle_f64_load, MemArg),
            (handle_i32_load8_s, MemArg), (handle_i32_load8_u, MemArg), (handle_i32_load16_s, MemArg), (handle_i32_load16_u, MemArg),
            (handle_i64_load8_s, MemArg), (handle_i64_load8_u, MemArg), (handle_i64_load16_s, MemArg), (handle_i64_load16_u, MemArg),
            (handle_i64_load32_s, MemArg), (handle_i64_load32_u, MemArg),
        );

        // Memory stores (MemArg param)
        handler_methods!(one_param:
            (handle_i32_store, MemArg), (handle_i64_store, MemArg), (handle_f32_store, MemArg), (handle_f64_store, MemArg),
            (handle_i32_store8, MemArg), (handle_i32_store16, MemArg),
            (handle_i64_store8, MemArg), (handle_i64_store16, MemArg), (handle_i64_store32, MemArg),
        );

        // Memory size/grow (no params)
        handler_methods!(no_param: handle_memory_size, handle_memory_grow);

        // Constants (typed params)
        handler_methods!(one_param:
            (handle_i32_const, i32), (handle_i64_const, i64), (handle_f32_const, f32), (handle_f64_const, f64),
        );

        // i32 comparison/test (no params)
        handler_methods!(no_param:
            handle_i32_eqz, handle_i32_eq, handle_i32_ne, handle_i32_lt_s, handle_i32_lt_u,
            handle_i32_gt_s, handle_i32_gt_u, handle_i32_le_s, handle_i32_le_u, handle_i32_ge_s, handle_i32_ge_u,
        );

        // i64 comparison/test (no params)
        handler_methods!(no_param:
            handle_i64_eqz, handle_i64_eq, handle_i64_ne, handle_i64_lt_s, handle_i64_lt_u,
            handle_i64_gt_s, handle_i64_gt_u, handle_i64_le_s, handle_i64_le_u, handle_i64_ge_s, handle_i64_ge_u,
        );

        // f32 comparison (no params)
        handler_methods!(no_param:
            handle_f32_eq, handle_f32_ne, handle_f32_lt, handle_f32_gt, handle_f32_le, handle_f32_ge,
        );

        // f64 comparison (no params)
        handler_methods!(no_param:
            handle_f64_eq, handle_f64_ne, handle_f64_lt, handle_f64_gt, handle_f64_le, handle_f64_ge,
        );

        // i32 arithmetic (no params)
        handler_methods!(no_param:
            handle_i32_clz, handle_i32_ctz, handle_i32_popcnt, handle_i32_add, handle_i32_sub, handle_i32_mul,
            handle_i32_div_s, handle_i32_div_u, handle_i32_rem_s, handle_i32_rem_u,
            handle_i32_and, handle_i32_or, handle_i32_xor, handle_i32_shl, handle_i32_shr_s, handle_i32_shr_u,
            handle_i32_rotl, handle_i32_rotr,
        );

        // i64 arithmetic (no params)
        handler_methods!(no_param:
            handle_i64_clz, handle_i64_ctz, handle_i64_popcnt, handle_i64_add, handle_i64_sub, handle_i64_mul,
            handle_i64_div_s, handle_i64_div_u, handle_i64_rem_s, handle_i64_rem_u,
            handle_i64_and, handle_i64_or, handle_i64_xor, handle_i64_shl, handle_i64_shr_s, handle_i64_shr_u,
            handle_i64_rotl, handle_i64_rotr,
        );

        // f32 arithmetic (no params)
        handler_methods!(no_param:
            handle_f32_abs, handle_f32_neg, handle_f32_ceil, handle_f32_floor, handle_f32_trunc, handle_f32_nearest, handle_f32_sqrt,
            handle_f32_add, handle_f32_sub, handle_f32_mul, handle_f32_div, handle_f32_min, handle_f32_max, handle_f32_copysign,
        );

        // f64 arithmetic (no params)
        handler_methods!(no_param:
            handle_f64_abs, handle_f64_neg, handle_f64_ceil, handle_f64_floor, handle_f64_trunc, handle_f64_nearest, handle_f64_sqrt,
            handle_f64_add, handle_f64_sub, handle_f64_mul, handle_f64_div, handle_f64_min, handle_f64_max, handle_f64_copysign,
        );

        // Type conversions (no params)
        handler_methods!(no_param:
            handle_i32_wrap_i64, handle_i32_trunc_f32_s, handle_i32_trunc_f32_u, handle_i32_trunc_f64_s, handle_i32_trunc_f64_u,
            handle_i64_extend_i32_s, handle_i64_extend_i32_u, handle_i64_trunc_f32_s, handle_i64_trunc_f32_u,
            handle_i64_trunc_f64_s, handle_i64_trunc_f64_u,
            handle_f32_convert_i32_s, handle_f32_convert_i32_u, handle_f32_convert_i64_s, handle_f32_convert_i64_u, handle_f32_demote_f64,
            handle_f64_convert_i32_s, handle_f64_convert_i32_u, handle_f64_convert_i64_s, handle_f64_convert_i64_u, handle_f64_promote_f32,
            handle_i32_reinterpret_f32, handle_i64_reinterpret_f64, handle_f32_reinterpret_i32, handle_f64_reinterpret_i64,
        );

        // IrVisitor methods - control flow (typed params)
        handler_methods!(one_param:
            (handle_if, JumpTarget), (handle_br, JumpTarget), (handle_br_if, JumpTarget),
        );

        // IrVisitor - br_table (special case with closure)
        handler_methods!(br_table);

        // IrVisitor - other control (typed params)
        handler_methods!(one_param:
            (handle_return, u8), (handle_call, u16), (handle_call_host, u16), (handle_call_indirect, TypeIdx),
        );

        // IrVisitor - variables (typed params)
        handler_methods!(one_param:
            (handle_local_get, LocalVariable), (handle_local_set, LocalVariable), (handle_local_tee, LocalVariable),
            (handle_global_get, GlobalVariable), (handle_global_set, GlobalVariable),
        );
    }

    /// Wrapper that delegates BaseVisitor and IrVisitor to TestHandler
    /// This is written ONCE and reused for all tests
    #[derive(Clone, Copy)]
    struct TestVisitor<H>(H);

    impl<H: TestHandler> BaseVisitor for TestVisitor<H> {
        type Error = ();
        type State = H::State;

        // Control flow and parametric
        delegate_methods!(no_param:
            finish => handle_finish, unreachable => handle_unreachable, nop => handle_nop,
            drop => handle_drop, select => handle_select,
        );

        // Memory loads
        delegate_methods!(one_param:
            (i32_load => handle_i32_load, arg: MemArg), (i64_load => handle_i64_load, arg: MemArg),
            (f32_load => handle_f32_load, arg: MemArg), (f64_load => handle_f64_load, arg: MemArg),
            (i32_load8_s => handle_i32_load8_s, arg: MemArg), (i32_load8_u => handle_i32_load8_u, arg: MemArg),
            (i32_load16_s => handle_i32_load16_s, arg: MemArg), (i32_load16_u => handle_i32_load16_u, arg: MemArg),
            (i64_load8_s => handle_i64_load8_s, arg: MemArg), (i64_load8_u => handle_i64_load8_u, arg: MemArg),
            (i64_load16_s => handle_i64_load16_s, arg: MemArg), (i64_load16_u => handle_i64_load16_u, arg: MemArg),
            (i64_load32_s => handle_i64_load32_s, arg: MemArg), (i64_load32_u => handle_i64_load32_u, arg: MemArg),
        );

        // Memory stores
        delegate_methods!(one_param:
            (i32_store => handle_i32_store, arg: MemArg), (i64_store => handle_i64_store, arg: MemArg),
            (f32_store => handle_f32_store, arg: MemArg), (f64_store => handle_f64_store, arg: MemArg),
            (i32_store8 => handle_i32_store8, arg: MemArg), (i32_store16 => handle_i32_store16, arg: MemArg),
            (i64_store8 => handle_i64_store8, arg: MemArg), (i64_store16 => handle_i64_store16, arg: MemArg),
            (i64_store32 => handle_i64_store32, arg: MemArg),
        );

        // Memory size/grow
        delegate_methods!(no_param: memory_size => handle_memory_size, memory_grow => handle_memory_grow);

        // Constants
        delegate_methods!(one_param:
            (i32_const => handle_i32_const, n: i32), (i64_const => handle_i64_const, n: i64),
            (f32_const => handle_f32_const, n: f32), (f64_const => handle_f64_const, n: f64),
        );

        // i32 comparison/test
        delegate_methods!(no_param:
            i32_eqz => handle_i32_eqz, i32_eq => handle_i32_eq, i32_ne => handle_i32_ne,
            i32_lt_s => handle_i32_lt_s, i32_lt_u => handle_i32_lt_u, i32_gt_s => handle_i32_gt_s, i32_gt_u => handle_i32_gt_u,
            i32_le_s => handle_i32_le_s, i32_le_u => handle_i32_le_u, i32_ge_s => handle_i32_ge_s, i32_ge_u => handle_i32_ge_u,
        );

        // i64 comparison/test
        delegate_methods!(no_param:
            i64_eqz => handle_i64_eqz, i64_eq => handle_i64_eq, i64_ne => handle_i64_ne,
            i64_lt_s => handle_i64_lt_s, i64_lt_u => handle_i64_lt_u, i64_gt_s => handle_i64_gt_s, i64_gt_u => handle_i64_gt_u,
            i64_le_s => handle_i64_le_s, i64_le_u => handle_i64_le_u, i64_ge_s => handle_i64_ge_s, i64_ge_u => handle_i64_ge_u,
        );

        // f32 comparison
        delegate_methods!(no_param:
            f32_eq => handle_f32_eq, f32_ne => handle_f32_ne, f32_lt => handle_f32_lt,
            f32_gt => handle_f32_gt, f32_le => handle_f32_le, f32_ge => handle_f32_ge,
        );

        // f64 comparison
        delegate_methods!(no_param:
            f64_eq => handle_f64_eq, f64_ne => handle_f64_ne, f64_lt => handle_f64_lt,
            f64_gt => handle_f64_gt, f64_le => handle_f64_le, f64_ge => handle_f64_ge,
        );

        // i32 arithmetic
        delegate_methods!(no_param:
            i32_clz => handle_i32_clz, i32_ctz => handle_i32_ctz, i32_popcnt => handle_i32_popcnt,
            i32_add => handle_i32_add, i32_sub => handle_i32_sub, i32_mul => handle_i32_mul,
            i32_div_s => handle_i32_div_s, i32_div_u => handle_i32_div_u, i32_rem_s => handle_i32_rem_s, i32_rem_u => handle_i32_rem_u,
            i32_and => handle_i32_and, i32_or => handle_i32_or, i32_xor => handle_i32_xor,
            i32_shl => handle_i32_shl, i32_shr_s => handle_i32_shr_s, i32_shr_u => handle_i32_shr_u,
            i32_rotl => handle_i32_rotl, i32_rotr => handle_i32_rotr,
        );

        // i64 arithmetic
        delegate_methods!(no_param:
            i64_clz => handle_i64_clz, i64_ctz => handle_i64_ctz, i64_popcnt => handle_i64_popcnt,
            i64_add => handle_i64_add, i64_sub => handle_i64_sub, i64_mul => handle_i64_mul,
            i64_div_s => handle_i64_div_s, i64_div_u => handle_i64_div_u, i64_rem_s => handle_i64_rem_s, i64_rem_u => handle_i64_rem_u,
            i64_and => handle_i64_and, i64_or => handle_i64_or, i64_xor => handle_i64_xor,
            i64_shl => handle_i64_shl, i64_shr_s => handle_i64_shr_s, i64_shr_u => handle_i64_shr_u,
            i64_rotl => handle_i64_rotl, i64_rotr => handle_i64_rotr,
        );

        // f32 arithmetic
        delegate_methods!(no_param:
            f32_abs => handle_f32_abs, f32_neg => handle_f32_neg, f32_ceil => handle_f32_ceil, f32_floor => handle_f32_floor,
            f32_trunc => handle_f32_trunc, f32_nearest => handle_f32_nearest, f32_sqrt => handle_f32_sqrt,
            f32_add => handle_f32_add, f32_sub => handle_f32_sub, f32_mul => handle_f32_mul, f32_div => handle_f32_div,
            f32_min => handle_f32_min, f32_max => handle_f32_max, f32_copysign => handle_f32_copysign,
        );

        // f64 arithmetic
        delegate_methods!(no_param:
            f64_abs => handle_f64_abs, f64_neg => handle_f64_neg, f64_ceil => handle_f64_ceil, f64_floor => handle_f64_floor,
            f64_trunc => handle_f64_trunc, f64_nearest => handle_f64_nearest, f64_sqrt => handle_f64_sqrt,
            f64_add => handle_f64_add, f64_sub => handle_f64_sub, f64_mul => handle_f64_mul, f64_div => handle_f64_div,
            f64_min => handle_f64_min, f64_max => handle_f64_max, f64_copysign => handle_f64_copysign,
        );

        // Type conversions
        delegate_methods!(no_param:
            i32_wrap_i64 => handle_i32_wrap_i64, i32_trunc_f32_s => handle_i32_trunc_f32_s, i32_trunc_f32_u => handle_i32_trunc_f32_u,
            i32_trunc_f64_s => handle_i32_trunc_f64_s, i32_trunc_f64_u => handle_i32_trunc_f64_u,
            i64_extend_i32_s => handle_i64_extend_i32_s, i64_extend_i32_u => handle_i64_extend_i32_u,
            i64_trunc_f32_s => handle_i64_trunc_f32_s, i64_trunc_f32_u => handle_i64_trunc_f32_u,
            i64_trunc_f64_s => handle_i64_trunc_f64_s, i64_trunc_f64_u => handle_i64_trunc_f64_u,
            f32_convert_i32_s => handle_f32_convert_i32_s, f32_convert_i32_u => handle_f32_convert_i32_u,
            f32_convert_i64_s => handle_f32_convert_i64_s, f32_convert_i64_u => handle_f32_convert_i64_u,
            f32_demote_f64 => handle_f32_demote_f64,
            f64_convert_i32_s => handle_f64_convert_i32_s, f64_convert_i32_u => handle_f64_convert_i32_u,
            f64_convert_i64_s => handle_f64_convert_i64_s, f64_convert_i64_u => handle_f64_convert_i64_u,
            f64_promote_f32 => handle_f64_promote_f32,
            i32_reinterpret_f32 => handle_i32_reinterpret_f32, i64_reinterpret_f64 => handle_i64_reinterpret_f64,
            f32_reinterpret_i32 => handle_f32_reinterpret_i32, f64_reinterpret_i64 => handle_f64_reinterpret_i64,
        );
    }

    impl<H: TestHandler> IrVisitor for TestVisitor<H> {
        // Control flow
        delegate_methods!(one_param:
            (if_ => handle_if, target: JumpTarget), (br => handle_br, target: JumpTarget),
            (br_if => handle_br_if, target: JumpTarget),
        );

        // br_table special case
        delegate_methods!(br_table);

        // Other control
        delegate_methods!(one_param:
            (return_ => handle_return, n: u8), (call => handle_call, idx: u16),
            (call_host => handle_call_host, idx: u16), (call_indirect => handle_call_indirect, ty: TypeIdx),
        );

        // Variables
        delegate_methods!(one_param:
            (local_get => handle_local_get, var: LocalVariable), (local_set => handle_local_set, var: LocalVariable),
            (local_tee => handle_local_tee, var: LocalVariable),
            (global_get => handle_global_get, var: GlobalVariable), (global_set => handle_global_set, var: GlobalVariable),
        );
    }

    /// Helper function to test a single instruction
    /// Handles all the boilerplate: creating compiler, compiling instruction, reading it back
    fn test_instruction<H, S, F, A>(handler: H, initial_state: S, compile: F, assert_fn: A)
    where
        H: TestHandler<State = S>,
        for<'a, 'b> F: FnOnce(
            &'a Compiler<'a, 4>,
            &'b mut TextBuilder<'a, 'a, 4>,
        ) -> Result<(), crate::ValidationError>,
        A: FnOnce(S),
    {
        let mut code_builder = CodeBuilder::<4>::new();
        let module = create_test_module();
        let func = create_test_func();
        let compiler = Compiler::<4>::new();

        {
            let mut text_builder = TextBuilder::new(&mut code_builder, &module, &func);
            compile(&compiler, &mut text_builder).unwrap();
        }

        let (pages, _) = code_builder.finish().unwrap();
        let code = Code::new(pages);
        let visitor = TestVisitor(handler);
        let mut state = initial_state;

        code.visit_instruction(&mut state, JumpTarget(0), visitor)
            .unwrap();
        assert_fn(state);
    }

    /// Macro to generate instruction tests
    macro_rules! test_instr {
        // No params - just verify instruction was called
        (no_param: $test_name:ident, $handler_method:ident, $compiler_method:ident) => {
            #[test]
            fn $test_name() {
                struct Handler;
                impl TestHandler for Handler {
                    type State = Cell<bool>;
                    fn $handler_method(&self, state: &mut Self::State) -> Result<(), ()> {
                        state.set(true);
                        Ok(())
                    }
                }

                test_instruction(
                    Handler,
                    Cell::new(false),
                    |compiler, text| compiler.$compiler_method(text),
                    |state| assert!(state.get()),
                );
            }
        };

        // Const instruction - verify param value was passed
        (const: $test_name:ident, $handler_method:ident, $compiler_method:ident, $param_ty:ty, $test_value:expr) => {
            #[test]
            fn $test_name() {
                struct Handler;
                impl TestHandler for Handler {
                    type State = Cell<Option<$param_ty>>;
                    fn $handler_method(
                        &self,
                        n: $param_ty,
                        state: &mut Self::State,
                    ) -> Result<(), ()> {
                        state.set(Some(n));
                        Ok(())
                    }
                }

                test_instruction(
                    Handler,
                    Cell::new(None),
                    |compiler, text| compiler.$compiler_method($test_value, text),
                    |state| assert_eq!(state.get(), Some($test_value)),
                );
            }
        };

        // MemArg instruction - verify instruction was called with MemArg
        (memarg: $test_name:ident, $handler_method:ident, $compiler_method:ident, $align:expr, $offset:expr) => {
            #[test]
            fn $test_name() {
                struct Handler;
                impl TestHandler for Handler {
                    type State = Cell<(bool, u32, u32)>;
                    fn $handler_method(
                        &self,
                        arg: MemArg,
                        state: &mut Self::State,
                    ) -> Result<(), ()> {
                        state.set((true, arg.align, arg.offset));
                        Ok(())
                    }
                }

                let test_arg = MemArg {
                    align: $align,
                    offset: $offset,
                };
                test_instruction(
                    Handler,
                    Cell::new((false, 0, 0)),
                    |compiler, text| compiler.$compiler_method(test_arg, text),
                    |state| {
                        let (called, align, offset) = state.get();
                        assert!(called);
                        assert_eq!(align, $align);
                        assert_eq!(offset, $offset);
                    },
                );
            }
        };

        // LocalVariable instruction - verify variable access
        (local: $test_name:ident, $handler_method:ident, $compiler_method:ident, $frame_offset:expr, $ty:expr) => {
            #[test]
            fn $test_name() {
                struct Handler;
                impl TestHandler for Handler {
                    type State = Cell<(bool, i16, crate::ValType)>;
                    fn $handler_method(
                        &self,
                        var: LocalVariable,
                        state: &mut Self::State,
                    ) -> Result<(), ()> {
                        state.set((true, var.frame_offset, var.ty));
                        Ok(())
                    }
                }

                let test_var = LocalVariable {
                    frame_offset: $frame_offset,
                    ty: $ty,
                };
                test_instruction(
                    Handler,
                    Cell::new((false, 0, crate::ValType::I32)),
                    |compiler, text| compiler.$compiler_method(test_var, text),
                    |state| {
                        let (called, offset, ty) = state.get();
                        assert!(called);
                        assert_eq!(offset, $frame_offset);
                        assert_eq!(ty, $ty);
                    },
                );
            }
        };

        // GlobalVariable instruction - verify global access
        (global: $test_name:ident, $handler_method:ident, $compiler_method:ident, $index:expr, $ty:expr, $is_imported:expr) => {
            #[test]
            fn $test_name() {
                struct Handler;
                impl TestHandler for Handler {
                    type State = Cell<(bool, u32, crate::ValType, bool)>;
                    fn $handler_method(
                        &self,
                        var: GlobalVariable,
                        state: &mut Self::State,
                    ) -> Result<(), ()> {
                        let idx = match var.reference {
                            crate::GlobalVariableRef::Internal(i) => i,
                            crate::GlobalVariableRef::Imported(i) => i,
                        };
                        let is_imported =
                            matches!(var.reference, crate::GlobalVariableRef::Imported(_));
                        state.set((true, idx, var.ty, is_imported));
                        Ok(())
                    }
                }

                let test_var = GlobalVariable {
                    reference: if $is_imported {
                        crate::GlobalVariableRef::Imported($index)
                    } else {
                        crate::GlobalVariableRef::Internal($index)
                    },
                    ty: $ty,
                    mutable: true,
                };
                test_instruction(
                    Handler,
                    Cell::new((false, 0, crate::ValType::I32, false)),
                    |compiler, text| compiler.$compiler_method(test_var, text),
                    |state| {
                        let (called, idx, ty, is_imported) = state.get();
                        assert!(called);
                        assert_eq!(idx, $index);
                        assert_eq!(ty, $ty);
                        assert_eq!(is_imported, $is_imported);
                    },
                );
            }
        };

        // Call instruction - verify function index
        (call: $test_name:ident, $handler_method:ident, $compiler_method:ident, $idx:expr) => {
            #[test]
            fn $test_name() {
                struct Handler;
                impl TestHandler for Handler {
                    type State = Cell<Option<u16>>;
                    fn $handler_method(&self, idx: u16, state: &mut Self::State) -> Result<(), ()> {
                        state.set(Some(idx));
                        Ok(())
                    }
                }

                test_instruction(
                    Handler,
                    Cell::new(None),
                    |compiler, text| compiler.$compiler_method($idx, text),
                    |state| assert_eq!(state.get(), Some($idx)),
                );
            }
        };

        // Return instruction - verify return count
        (return: $test_name:ident, $handler_method:ident, $compiler_method:ident, $count:expr) => {
            #[test]
            fn $test_name() {
                struct Handler;
                impl TestHandler for Handler {
                    type State = Cell<Option<u8>>;
                    fn $handler_method(
                        &self,
                        count: u8,
                        state: &mut Self::State,
                    ) -> Result<(), ()> {
                        state.set(Some(count));
                        Ok(())
                    }
                }

                test_instruction(
                    Handler,
                    Cell::new(None),
                    |compiler, text| compiler.$compiler_method($count, text),
                    |state| assert_eq!(state.get(), Some($count)),
                );
            }
        };

        // JumpTarget instruction - verify control flow target
        (jump: $test_name:ident, $handler_method:ident, $compiler_method:ident, $target:expr) => {
            #[test]
            fn $test_name() {
                struct Handler;
                impl TestHandler for Handler {
                    type State = Cell<Option<u32>>;
                    fn $handler_method(
                        &self,
                        target: JumpTarget,
                        state: &mut Self::State,
                    ) -> Result<(), ()> {
                        state.set(Some(target.0));
                        Ok(())
                    }
                }

                test_instruction(
                    Handler,
                    Cell::new(None),
                    |compiler, text| compiler.$compiler_method(JumpTarget($target), text),
                    |state| assert_eq!(state.get(), Some($target)),
                );
            }
        };

        // TypeIdx instruction - verify type index
        (typeidx: $test_name:ident, $handler_method:ident, $compiler_method:ident, $idx:expr) => {
            #[test]
            fn $test_name() {
                struct Handler;
                impl TestHandler for Handler {
                    type State = Cell<Option<u32>>;
                    fn $handler_method(
                        &self,
                        ty: TypeIdx,
                        state: &mut Self::State,
                    ) -> Result<(), ()> {
                        state.set(Some(ty.0));
                        Ok(())
                    }
                }

                test_instruction(
                    Handler,
                    Cell::new(None),
                    |compiler, text| compiler.$compiler_method(TypeIdx($idx), text),
                    |state| assert_eq!(state.get(), Some($idx)),
                );
            }
        };
    }

    // Control flow and parametric (no params)
    test_instr!(no_param: test_unreachable, handle_unreachable, unreachable);
    test_instr!(no_param: test_nop, handle_nop, nop);
    test_instr!(no_param: test_drop, handle_drop, drop);
    test_instr!(no_param: test_select, handle_select, select);

    // Memory size/grow
    test_instr!(no_param: test_memory_size, handle_memory_size, memory_size);

    // Const instructions
    test_instr!(const: test_i32_const_small, handle_i32_const, i32_const, i32, 42);
    test_instr!(const: test_i32_const_large, handle_i32_const, i32_const, i32, 0x1234_5678);
    test_instr!(const: test_i64_const_small, handle_i64_const, i64_const, i64, 56);
    test_instr!(const: test_i64_const_large, handle_i64_const, i64_const, i64, 0x1234_5678_9ABC_DEF0);
    test_instr!(const: test_f32_const, handle_f32_const, f32_const, f32, core::f32::consts::PI);
    test_instr!(const: test_f64_const, handle_f64_const, f64_const, f64, core::f64::consts::E);

    // i32 comparison/test
    test_instr!(no_param: test_i32_eqz, handle_i32_eqz, i32_eqz);
    test_instr!(no_param: test_i32_eq, handle_i32_eq, i32_eq);
    test_instr!(no_param: test_i32_ne, handle_i32_ne, i32_ne);
    test_instr!(no_param: test_i32_lt_s, handle_i32_lt_s, i32_lt_s);
    test_instr!(no_param: test_i32_lt_u, handle_i32_lt_u, i32_lt_u);
    test_instr!(no_param: test_i32_gt_s, handle_i32_gt_s, i32_gt_s);
    test_instr!(no_param: test_i32_gt_u, handle_i32_gt_u, i32_gt_u);
    test_instr!(no_param: test_i32_le_s, handle_i32_le_s, i32_le_s);
    test_instr!(no_param: test_i32_le_u, handle_i32_le_u, i32_le_u);
    test_instr!(no_param: test_i32_ge_s, handle_i32_ge_s, i32_ge_s);
    test_instr!(no_param: test_i32_ge_u, handle_i32_ge_u, i32_ge_u);

    // i64 comparison/test
    test_instr!(no_param: test_i64_eqz, handle_i64_eqz, i64_eqz);
    test_instr!(no_param: test_i64_eq, handle_i64_eq, i64_eq);
    test_instr!(no_param: test_i64_ne, handle_i64_ne, i64_ne);
    test_instr!(no_param: test_i64_lt_s, handle_i64_lt_s, i64_lt_s);
    test_instr!(no_param: test_i64_lt_u, handle_i64_lt_u, i64_lt_u);
    test_instr!(no_param: test_i64_gt_s, handle_i64_gt_s, i64_gt_s);
    test_instr!(no_param: test_i64_gt_u, handle_i64_gt_u, i64_gt_u);
    test_instr!(no_param: test_i64_le_s, handle_i64_le_s, i64_le_s);
    test_instr!(no_param: test_i64_le_u, handle_i64_le_u, i64_le_u);
    test_instr!(no_param: test_i64_ge_s, handle_i64_ge_s, i64_ge_s);
    test_instr!(no_param: test_i64_ge_u, handle_i64_ge_u, i64_ge_u);

    // f32 comparison
    test_instr!(no_param: test_f32_eq, handle_f32_eq, f32_eq);
    test_instr!(no_param: test_f32_ne, handle_f32_ne, f32_ne);
    test_instr!(no_param: test_f32_lt, handle_f32_lt, f32_lt);
    test_instr!(no_param: test_f32_gt, handle_f32_gt, f32_gt);
    test_instr!(no_param: test_f32_le, handle_f32_le, f32_le);
    test_instr!(no_param: test_f32_ge, handle_f32_ge, f32_ge);

    // f64 comparison
    test_instr!(no_param: test_f64_eq, handle_f64_eq, f64_eq);
    test_instr!(no_param: test_f64_ne, handle_f64_ne, f64_ne);
    test_instr!(no_param: test_f64_lt, handle_f64_lt, f64_lt);
    test_instr!(no_param: test_f64_gt, handle_f64_gt, f64_gt);
    test_instr!(no_param: test_f64_le, handle_f64_le, f64_le);
    test_instr!(no_param: test_f64_ge, handle_f64_ge, f64_ge);

    // i32 arithmetic
    test_instr!(no_param: test_i32_clz, handle_i32_clz, i32_clz);
    test_instr!(no_param: test_i32_ctz, handle_i32_ctz, i32_ctz);
    test_instr!(no_param: test_i32_popcnt, handle_i32_popcnt, i32_popcnt);
    test_instr!(no_param: test_i32_add, handle_i32_add, i32_add);
    test_instr!(no_param: test_i32_sub, handle_i32_sub, i32_sub);
    test_instr!(no_param: test_i32_mul, handle_i32_mul, i32_mul);
    test_instr!(no_param: test_i32_div_s, handle_i32_div_s, i32_div_s);
    test_instr!(no_param: test_i32_div_u, handle_i32_div_u, i32_div_u);
    test_instr!(no_param: test_i32_rem_s, handle_i32_rem_s, i32_rem_s);
    test_instr!(no_param: test_i32_rem_u, handle_i32_rem_u, i32_rem_u);
    test_instr!(no_param: test_i32_and, handle_i32_and, i32_and);
    test_instr!(no_param: test_i32_or, handle_i32_or, i32_or);
    test_instr!(no_param: test_i32_xor, handle_i32_xor, i32_xor);
    test_instr!(no_param: test_i32_shl, handle_i32_shl, i32_shl);
    test_instr!(no_param: test_i32_shr_s, handle_i32_shr_s, i32_shr_s);
    test_instr!(no_param: test_i32_shr_u, handle_i32_shr_u, i32_shr_u);
    test_instr!(no_param: test_i32_rotl, handle_i32_rotl, i32_rotl);
    test_instr!(no_param: test_i32_rotr, handle_i32_rotr, i32_rotr);

    // i64 arithmetic
    test_instr!(no_param: test_i64_clz, handle_i64_clz, i64_clz);
    test_instr!(no_param: test_i64_ctz, handle_i64_ctz, i64_ctz);
    test_instr!(no_param: test_i64_popcnt, handle_i64_popcnt, i64_popcnt);
    test_instr!(no_param: test_i64_add, handle_i64_add, i64_add);
    test_instr!(no_param: test_i64_sub, handle_i64_sub, i64_sub);
    test_instr!(no_param: test_i64_mul, handle_i64_mul, i64_mul);
    test_instr!(no_param: test_i64_div_s, handle_i64_div_s, i64_div_s);
    test_instr!(no_param: test_i64_div_u, handle_i64_div_u, i64_div_u);
    test_instr!(no_param: test_i64_rem_s, handle_i64_rem_s, i64_rem_s);
    test_instr!(no_param: test_i64_rem_u, handle_i64_rem_u, i64_rem_u);
    test_instr!(no_param: test_i64_and, handle_i64_and, i64_and);
    test_instr!(no_param: test_i64_or, handle_i64_or, i64_or);
    test_instr!(no_param: test_i64_xor, handle_i64_xor, i64_xor);
    test_instr!(no_param: test_i64_shl, handle_i64_shl, i64_shl);
    test_instr!(no_param: test_i64_shr_s, handle_i64_shr_s, i64_shr_s);
    test_instr!(no_param: test_i64_shr_u, handle_i64_shr_u, i64_shr_u);
    test_instr!(no_param: test_i64_rotl, handle_i64_rotl, i64_rotl);
    test_instr!(no_param: test_i64_rotr, handle_i64_rotr, i64_rotr);

    // f32 arithmetic
    test_instr!(no_param: test_f32_abs, handle_f32_abs, f32_abs);
    test_instr!(no_param: test_f32_neg, handle_f32_neg, f32_neg);
    test_instr!(no_param: test_f32_ceil, handle_f32_ceil, f32_ceil);
    test_instr!(no_param: test_f32_floor, handle_f32_floor, f32_floor);
    test_instr!(no_param: test_f32_trunc, handle_f32_trunc, f32_trunc);
    test_instr!(no_param: test_f32_nearest, handle_f32_nearest, f32_nearest);
    test_instr!(no_param: test_f32_sqrt, handle_f32_sqrt, f32_sqrt);
    test_instr!(no_param: test_f32_add, handle_f32_add, f32_add);
    test_instr!(no_param: test_f32_sub, handle_f32_sub, f32_sub);
    test_instr!(no_param: test_f32_mul, handle_f32_mul, f32_mul);
    test_instr!(no_param: test_f32_div, handle_f32_div, f32_div);
    test_instr!(no_param: test_f32_min, handle_f32_min, f32_min);
    test_instr!(no_param: test_f32_max, handle_f32_max, f32_max);
    test_instr!(no_param: test_f32_copysign, handle_f32_copysign, f32_copysign);

    // f64 arithmetic
    test_instr!(no_param: test_f64_abs, handle_f64_abs, f64_abs);
    test_instr!(no_param: test_f64_neg, handle_f64_neg, f64_neg);
    test_instr!(no_param: test_f64_ceil, handle_f64_ceil, f64_ceil);
    test_instr!(no_param: test_f64_floor, handle_f64_floor, f64_floor);
    test_instr!(no_param: test_f64_trunc, handle_f64_trunc, f64_trunc);
    test_instr!(no_param: test_f64_nearest, handle_f64_nearest, f64_nearest);
    test_instr!(no_param: test_f64_sqrt, handle_f64_sqrt, f64_sqrt);
    test_instr!(no_param: test_f64_add, handle_f64_add, f64_add);
    test_instr!(no_param: test_f64_sub, handle_f64_sub, f64_sub);
    test_instr!(no_param: test_f64_mul, handle_f64_mul, f64_mul);
    test_instr!(no_param: test_f64_div, handle_f64_div, f64_div);
    test_instr!(no_param: test_f64_min, handle_f64_min, f64_min);
    test_instr!(no_param: test_f64_max, handle_f64_max, f64_max);
    test_instr!(no_param: test_f64_copysign, handle_f64_copysign, f64_copysign);

    // Type conversions
    test_instr!(no_param: test_i32_wrap_i64, handle_i32_wrap_i64, i32_wrap_i64);
    test_instr!(no_param: test_i32_trunc_f32_s, handle_i32_trunc_f32_s, i32_trunc_f32_s);
    test_instr!(no_param: test_i32_trunc_f32_u, handle_i32_trunc_f32_u, i32_trunc_f32_u);
    test_instr!(no_param: test_i32_trunc_f64_s, handle_i32_trunc_f64_s, i32_trunc_f64_s);
    test_instr!(no_param: test_i32_trunc_f64_u, handle_i32_trunc_f64_u, i32_trunc_f64_u);
    test_instr!(no_param: test_i64_extend_i32_s, handle_i64_extend_i32_s, i64_extend_i32_s);
    test_instr!(no_param: test_i64_extend_i32_u, handle_i64_extend_i32_u, i64_extend_i32_u);
    test_instr!(no_param: test_i64_trunc_f32_s, handle_i64_trunc_f32_s, i64_trunc_f32_s);
    test_instr!(no_param: test_i64_trunc_f32_u, handle_i64_trunc_f32_u, i64_trunc_f32_u);
    test_instr!(no_param: test_i64_trunc_f64_s, handle_i64_trunc_f64_s, i64_trunc_f64_s);
    test_instr!(no_param: test_i64_trunc_f64_u, handle_i64_trunc_f64_u, i64_trunc_f64_u);
    test_instr!(no_param: test_f32_convert_i32_s, handle_f32_convert_i32_s, f32_convert_i32_s);
    test_instr!(no_param: test_f32_convert_i32_u, handle_f32_convert_i32_u, f32_convert_i32_u);
    test_instr!(no_param: test_f32_convert_i64_s, handle_f32_convert_i64_s, f32_convert_i64_s);
    test_instr!(no_param: test_f32_convert_i64_u, handle_f32_convert_i64_u, f32_convert_i64_u);
    test_instr!(no_param: test_f32_demote_f64, handle_f32_demote_f64, f32_demote_f64);
    test_instr!(no_param: test_f64_convert_i32_s, handle_f64_convert_i32_s, f64_convert_i32_s);
    test_instr!(no_param: test_f64_convert_i32_u, handle_f64_convert_i32_u, f64_convert_i32_u);
    test_instr!(no_param: test_f64_convert_i64_s, handle_f64_convert_i64_s, f64_convert_i64_s);
    test_instr!(no_param: test_f64_convert_i64_u, handle_f64_convert_i64_u, f64_convert_i64_u);
    test_instr!(no_param: test_f64_promote_f32, handle_f64_promote_f32, f64_promote_f32);
    // Note: reinterpret instructions are no-ops in the IR (bitwise transmutes)
    // so they don't generate any code to test

    // Memory loads
    test_instr!(memarg: test_i32_load, handle_i32_load, i32_load, 2, 0);
    test_instr!(memarg: test_i64_load, handle_i64_load, i64_load, 3, 8);
    test_instr!(memarg: test_f32_load, handle_f32_load, f32_load, 2, 16);
    test_instr!(memarg: test_f64_load, handle_f64_load, f64_load, 3, 24);
    test_instr!(memarg: test_i32_load8_s, handle_i32_load8_s, i32_load8_s, 0, 0);
    test_instr!(memarg: test_i32_load8_u, handle_i32_load8_u, i32_load8_u, 0, 1);
    test_instr!(memarg: test_i32_load16_s, handle_i32_load16_s, i32_load16_s, 1, 2);
    test_instr!(memarg: test_i32_load16_u, handle_i32_load16_u, i32_load16_u, 1, 4);
    test_instr!(memarg: test_i64_load8_s, handle_i64_load8_s, i64_load8_s, 0, 0);
    test_instr!(memarg: test_i64_load8_u, handle_i64_load8_u, i64_load8_u, 0, 1);
    test_instr!(memarg: test_i64_load16_s, handle_i64_load16_s, i64_load16_s, 1, 2);
    test_instr!(memarg: test_i64_load16_u, handle_i64_load16_u, i64_load16_u, 1, 4);
    test_instr!(memarg: test_i64_load32_s, handle_i64_load32_s, i64_load32_s, 2, 8);
    test_instr!(memarg: test_i64_load32_u, handle_i64_load32_u, i64_load32_u, 2, 12);

    // Memory stores
    test_instr!(memarg: test_i32_store, handle_i32_store, i32_store, 2, 0);
    test_instr!(memarg: test_i64_store, handle_i64_store, i64_store, 3, 8);
    test_instr!(memarg: test_f32_store, handle_f32_store, f32_store, 2, 16);
    test_instr!(memarg: test_f64_store, handle_f64_store, f64_store, 3, 24);
    test_instr!(memarg: test_i32_store8, handle_i32_store8, i32_store8, 0, 0);
    test_instr!(memarg: test_i32_store16, handle_i32_store16, i32_store16, 1, 2);
    test_instr!(memarg: test_i64_store8, handle_i64_store8, i64_store8, 0, 0);
    test_instr!(memarg: test_i64_store16, handle_i64_store16, i64_store16, 1, 2);
    test_instr!(memarg: test_i64_store32, handle_i64_store32, i64_store32, 2, 4);

    /// Test if/else control flow specifically
    #[test]
    fn test_if_else() {
        use crate::{ResultType, WasmVisitor};

        let mut code_builder = CodeBuilder::<4>::new();
        let mut module = create_test_module();

        let mut types = crate::Vec::new(1).unwrap();
        types.push(crate::FuncType {
            params: crate::Vec::zero(),
            returns: crate::Vec::zero(),
        });
        module.types = types;

        let func = create_test_func();
        let compiler = Compiler::<4>::new();

        {
            let mut text_builder = TextBuilder::new(&mut code_builder, &module, &func);

            // Test if/else structure with code in both branches
            compiler.i32_const(1, &mut text_builder).unwrap(); // Condition
            compiler.if_(ResultType(None), &mut text_builder).unwrap();
            compiler.i32_const(2, &mut text_builder).unwrap(); // Then branch
            compiler.else_(&mut text_builder).unwrap();
            compiler.i32_const(3, &mut text_builder).unwrap(); // Else branch
            compiler.exit_block(&mut text_builder).unwrap();
            compiler.return_(&mut text_builder).unwrap();
        }

        let result = code_builder.finish();
        assert!(result.is_ok(), "if/else should compile: {:?}", result.err());

        // Verify the code was generated
        let (pages, _) = result.unwrap();
        assert!(pages.len() > 0, "Should generate code");

        // Now decode and verify we see the BR instruction from else
        use core::cell::Cell;
        use std::rc::Rc;

        #[derive(Clone)]
        struct ElseTestHandler {
            if_count: Rc<Cell<u32>>,
            br_count: Rc<Cell<u32>>,
            i32_const_count: Rc<Cell<u32>>,
        }

        impl TestHandler for ElseTestHandler {
            type State = ();

            fn handle_if(&self, _: JumpTarget, _: &mut Self::State) -> Result<(), ()> {
                self.if_count.set(self.if_count.get() + 1);
                Ok(())
            }

            fn handle_br(&self, _: JumpTarget, _: &mut Self::State) -> Result<(), ()> {
                self.br_count.set(self.br_count.get() + 1);
                Ok(())
            }

            fn handle_i32_const(&self, _: i32, _: &mut Self::State) -> Result<(), ()> {
                self.i32_const_count.set(self.i32_const_count.get() + 1);
                Ok(())
            }

            fn handle_return(&self, _: u8, _: &mut Self::State) -> Result<(), ()> {
                Ok(())
            }

            fn handle_unreachable(&self, _: &mut Self::State) -> Result<(), ()> {
                Ok(())
            }

            fn handle_finish(&self, _: &mut Self::State) -> Result<(), ()> {
                Ok(())
            }
        }

        let code = Code::new(pages);
        let handler = ElseTestHandler {
            if_count: Rc::new(Cell::new(0)),
            br_count: Rc::new(Cell::new(0)),
            i32_const_count: Rc::new(Cell::new(0)),
        };

        let mut state = ();
        let mut pc = JumpTarget(0);
        let mut instruction_count = 0;

        loop {
            let visitor = TestVisitor(handler.clone());
            match code.visit_instruction(&mut state, pc, visitor) {
                Ok((words_consumed, _)) => {
                    pc = pc + words_consumed;
                    instruction_count += 1;
                    if instruction_count > 100 {
                        break;
                    }
                }
                Err(_) => break,
            }
        }

        // Verify if/else structure:
        // - 1 if instruction
        // - 1 br instruction (generated by else to jump to end)
        // - 2 i32.const instructions (one in then branch, one in else branch)
        assert_eq!(handler.if_count.get(), 1, "Should have 1 if instruction");
        assert_eq!(
            handler.br_count.get(),
            1,
            "Should have 1 br instruction (from else clause)"
        );
        assert_eq!(
            handler.i32_const_count.get(),
            3,
            "Should have 3 i32.const (condition + then + else)"
        );
    }

    /// Comprehensive test for control flow and variable instructions
    ///
    /// Compiles a sequence of instructions covering all major categories:
    /// - Local variables: local.get, local.set, local.tee
    /// - Global variables: global.get, global.set
    /// - Control flow: block, loop, if, br, br_if, br_table, return, call (internal functions)
    /// - Constants: i32.const
    ///
    /// Then reads back the compiled IR code and verifies each instruction type
    /// was correctly encoded and decoded. This validates the full compile→encode→decode
    /// pipeline for control flow and variable operations.
    ///
    /// Note: The 'else' instruction is tested separately in test_if_else.
    #[test]
    fn test_control_flow_and_variables() {
        use crate::{FuncIdx, GlobalIdx, LabelIdx, LocalIdx, ResultType, WasmVisitor};
        use core::ops::ControlFlow;

        let mut code_builder = CodeBuilder::<4>::new();

        // Create module with globals and imports
        let mut module = create_test_module();

        // Add function types for the test function and callees
        let mut types = crate::Vec::new(2).unwrap();
        // Type 0: () -> () for our test function and callees
        types.push(crate::FuncType {
            params: crate::Vec::zero(),
            returns: crate::Vec::zero(),
        });
        module.types = types;

        // Add some internal functions to call
        let mut functions = crate::Vec::new(2).unwrap();
        functions.push(create_test_func()); // Function 0 (internal)
        functions.push(create_test_func()); // Function 1 (internal)
        module.functions = functions;

        // Add global variables
        let mut globals = crate::Vec::new(2).unwrap();
        globals.push(crate::Global {
            type_: crate::GlobalType {
                ty: crate::ValType::I32,
                mutable: true,
            },
            init: crate::Value::I32(0),
        });
        globals.push(crate::Global {
            type_: crate::GlobalType {
                ty: crate::ValType::I64,
                mutable: true,
            },
            init: crate::Value::I64(0),
        });
        module.globals = globals;

        // Setup module imports - add an imported function
        static EMPTY_PARAMS: &[crate::ValType] = &[];
        static EMPTY_RETURNS: &[crate::ValType] = &[];

        fn dummy_host_fn(_args: &[crate::Value]) -> crate::HostFunctionResult {
            ControlFlow::Continue(None)
        }

        let host_func = crate::HostFunction::new(
            "env",
            "imported_fn",
            EMPTY_PARAMS,
            EMPTY_RETURNS,
            dummy_host_fn,
        );

        // Create a long-lived slice containing the host function
        let host_funcs = [host_func];

        module.module_imports = ModuleImports {
            functions: &host_funcs,
            memories: &[],
            globals: &[],
        };

        // Add Import entries to indicate which function indices are imported
        let mut imports = crate::Vec::new(1).unwrap();
        imports.push(crate::Import::Func(0)); // Function index 0 is imported (host_funcs[0])
        module.imports = imports;

        // Create function with locals
        let mut func = create_test_func();
        func.locals = {
            let mut locals = crate::Vec::new(3).unwrap();
            locals.push((1, crate::ValType::I32));
            locals.push((1, crate::ValType::I64));
            locals.push((1, crate::ValType::F32));
            locals
        };
        func.local_size = 16;
        func.parameter_size = 0;

        let compiler = Compiler::<4>::new();

        // Compile a sequence of instructions testing all control flow and variable operations
        {
            let mut text_builder = TextBuilder::new(&mut code_builder, &module, &func);

            // Test local variable operations
            compiler.local_get(LocalIdx(0), &mut text_builder).unwrap();
            compiler.local_set(LocalIdx(1), &mut text_builder).unwrap();
            compiler.local_tee(LocalIdx(2), &mut text_builder).unwrap();

            // Test global variable operations
            compiler
                .global_get(GlobalIdx(0), &mut text_builder)
                .unwrap();
            compiler
                .global_set(GlobalIdx(1), &mut text_builder)
                .unwrap();

            // Test control flow: block
            compiler
                .enter_block(ResultType(None), &mut text_builder)
                .unwrap();
            compiler.i32_const(42, &mut text_builder).unwrap();
            compiler.exit_block(&mut text_builder).unwrap();

            // Test control flow: loop
            compiler.loop_(ResultType(None), &mut text_builder).unwrap();
            compiler.i32_const(43, &mut text_builder).unwrap();
            compiler.exit_block(&mut text_builder).unwrap();

            // Test control flow: if (without else - else is tested separately)
            compiler.i32_const(1, &mut text_builder).unwrap();
            compiler.if_(ResultType(None), &mut text_builder).unwrap();
            compiler.i32_const(2, &mut text_builder).unwrap();
            compiler.exit_block(&mut text_builder).unwrap();

            // Test control flow: br, br_if
            compiler
                .enter_block(ResultType(None), &mut text_builder)
                .unwrap();
            compiler.i32_const(1, &mut text_builder).unwrap();
            compiler.br_if(LabelIdx(0), &mut text_builder).unwrap();
            compiler.br(LabelIdx(0), &mut text_builder).unwrap();
            compiler.exit_block(&mut text_builder).unwrap();

            // Call internal
            compiler.call(FuncIdx(1), &mut text_builder).unwrap();

            // Call host
            compiler.call(FuncIdx(0), &mut text_builder).unwrap();

            // Test control flow: br_table
            compiler
                .enter_block(ResultType(None), &mut text_builder)
                .unwrap();
            compiler.i32_const(0, &mut text_builder).unwrap();
            compiler
                .br_table(&[LabelIdx(0), LabelIdx(0)], LabelIdx(0), &mut text_builder)
                .unwrap();
            compiler.exit_block(&mut text_builder).unwrap();

            // Test control flow: return
            compiler.return_(&mut text_builder).unwrap();
        }

        // Verify compilation succeeded
        let result = code_builder.finish();
        assert!(result.is_ok(), "Compilation should succeed");

        // Verify we generated some code
        let (pages, _) = result.unwrap();
        assert!(pages.len() > 0, "Should generate at least one page of code");

        // Now read back the compiled code and verify correct instructions are decoded
        use core::cell::Cell;
        extern crate std;
        use std::rc::Rc;

        #[derive(Clone)]
        struct InstructionCounts {
            local_get: Rc<Cell<u32>>,
            local_set: Rc<Cell<u32>>,
            local_tee: Rc<Cell<u32>>,
            global_get: Rc<Cell<u32>>,
            global_set: Rc<Cell<u32>>,
            i32_const: Rc<Cell<u32>>,
            if_: Rc<Cell<u32>>,
            br: Rc<Cell<u32>>,
            br_if: Rc<Cell<u32>>,
            br_table: Rc<Cell<u32>>,
            call: Rc<Cell<u32>>,
            call_host: Rc<Cell<u32>>,
            return_: Rc<Cell<u32>>,
        }

        #[derive(Clone)]
        struct ReadHandler {
            counts: InstructionCounts,
            first_local_get: Rc<Cell<Option<(i16, crate::ValType)>>>,
        }

        impl TestHandler for ReadHandler {
            type State = ();

            fn handle_local_get(&self, var: LocalVariable, _: &mut Self::State) -> Result<(), ()> {
                self.counts.local_get.set(self.counts.local_get.get() + 1);
                // Capture the first local_get for verification
                if self.first_local_get.get().is_none() {
                    self.first_local_get.set(Some((var.frame_offset, var.ty)));
                }
                Ok(())
            }

            fn handle_local_set(&self, _: LocalVariable, _: &mut Self::State) -> Result<(), ()> {
                self.counts.local_set.set(self.counts.local_set.get() + 1);
                Ok(())
            }

            fn handle_local_tee(&self, _: LocalVariable, _: &mut Self::State) -> Result<(), ()> {
                self.counts.local_tee.set(self.counts.local_tee.get() + 1);
                Ok(())
            }

            fn handle_global_get(&self, _: GlobalVariable, _: &mut Self::State) -> Result<(), ()> {
                self.counts.global_get.set(self.counts.global_get.get() + 1);
                Ok(())
            }

            fn handle_global_set(&self, _: GlobalVariable, _: &mut Self::State) -> Result<(), ()> {
                self.counts.global_set.set(self.counts.global_set.get() + 1);
                Ok(())
            }

            fn handle_i32_const(&self, _: i32, _: &mut Self::State) -> Result<(), ()> {
                self.counts.i32_const.set(self.counts.i32_const.get() + 1);
                Ok(())
            }

            fn handle_if(&self, _: JumpTarget, _: &mut Self::State) -> Result<(), ()> {
                self.counts.if_.set(self.counts.if_.get() + 1);
                Ok(())
            }

            fn handle_br(&self, _: JumpTarget, _: &mut Self::State) -> Result<(), ()> {
                self.counts.br.set(self.counts.br.get() + 1);
                Ok(())
            }

            fn handle_br_if(&self, _: JumpTarget, _: &mut Self::State) -> Result<(), ()> {
                self.counts.br_if.set(self.counts.br_if.get() + 1);
                Ok(())
            }

            fn handle_br_table(
                &self,
                _f: impl FnOnce(u16) -> Result<JumpTarget, ()>,
                _: &mut Self::State,
            ) -> Result<(), ()> {
                self.counts.br_table.set(self.counts.br_table.get() + 1);
                Ok(())
            }

            fn handle_return(&self, _: u8, _: &mut Self::State) -> Result<(), ()> {
                self.counts.return_.set(self.counts.return_.get() + 1);
                Ok(())
            }

            fn handle_call(&self, idx: u16, _: &mut Self::State) -> Result<(), ()> {
                assert_eq!(idx, 0);
                self.counts.call.set(self.counts.call.get() + 1);
                Ok(())
            }

            fn handle_call_host(&self, idx: u16, _: &mut Self::State) -> Result<(), ()> {
                assert_eq!(idx, 0);
                self.counts.call_host.set(self.counts.call_host.get() + 1);
                Ok(())
            }

            // Allow unreachable instructions (might be generated by compiler for certain control flow)
            fn handle_unreachable(&self, _: &mut Self::State) -> Result<(), ()> {
                Ok(())
            }

            // Allow finish instructions
            fn handle_finish(&self, _: &mut Self::State) -> Result<(), ()> {
                Ok(())
            }
        }

        let code = Code::new(pages);
        let handler = ReadHandler {
            counts: InstructionCounts {
                local_get: Rc::new(Cell::new(0)),
                local_set: Rc::new(Cell::new(0)),
                local_tee: Rc::new(Cell::new(0)),
                global_get: Rc::new(Cell::new(0)),
                global_set: Rc::new(Cell::new(0)),
                i32_const: Rc::new(Cell::new(0)),
                if_: Rc::new(Cell::new(0)),
                br: Rc::new(Cell::new(0)),
                br_if: Rc::new(Cell::new(0)),
                br_table: Rc::new(Cell::new(0)),
                call: Rc::new(Cell::new(0)),
                call_host: Rc::new(Cell::new(0)),
                return_: Rc::new(Cell::new(0)),
            },
            first_local_get: Rc::new(Cell::new(None)),
        };
        let mut state = ();

        let mut pc = JumpTarget(0);
        let mut instruction_count = 0;
        loop {
            let visitor = TestVisitor(handler.clone());
            match code.visit_instruction(&mut state, pc, visitor) {
                Ok((words_consumed, _)) => {
                    pc = pc + words_consumed;
                    instruction_count += 1;
                    // Safety limit to prevent infinite loops
                    if instruction_count > 1000 || handler.counts.return_.get() > 0 {
                        break;
                    }
                }
                Err(_) => break,
            }
        }

        assert!(
            instruction_count > 0,
            "Should have decoded some instructions"
        );

        assert_eq!(handler.counts.local_get.get(), 1);
        assert_eq!(handler.counts.local_set.get(), 1);
        assert_eq!(handler.counts.local_tee.get(), 1);

        if let Some((_offset, ty)) = handler.first_local_get.get() {
            assert_eq!(ty, crate::ValType::I32, "First local.get should be I32");
        } else {
            panic!("Should have captured first local.get");
        }

        assert_eq!(handler.counts.global_get.get(), 1);
        assert_eq!(handler.counts.global_set.get(), 1);

        assert_eq!(handler.counts.if_.get(), 1);
        assert!(
            handler.counts.br.get() >= 1,
            "Should have at least 1 br, found {}",
            handler.counts.br.get()
        );
        assert_eq!(handler.counts.br_if.get(), 1);
        assert_eq!(handler.counts.br_table.get(), 1);
        assert_eq!(handler.counts.return_.get(), 1);
        assert_eq!(handler.counts.call.get(), 1);
        assert_eq!(handler.counts.call_host.get(), 1);
        assert!(
            handler.counts.i32_const.get() >= 6,
            "Should have at least 6 i32.const instructions, found {}",
            handler.counts.i32_const.get()
        );
    }
}
