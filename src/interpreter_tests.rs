#[cfg(test)]
mod tests {
    use crate::{
        AllocError, BaseVisitor, Interpreter, InterpreterState, IrVisitor, MemArg, MemoryKind,
        Module, Store, ValType,
    };

    extern crate std;

    struct TestAllocator;
    impl crate::memory::WasmMemoryAllocator for TestAllocator {
        fn allocate(
            &self,
            layout: std::alloc::Layout,
        ) -> Result<std::ptr::NonNull<u8>, AllocError> {
            unsafe {
                let ptr = std::alloc::alloc(layout);
                std::ptr::NonNull::new(ptr).ok_or(AllocError::AllocationFailed)
            }
        }

        fn reallocate(
            &self,
            ptr: std::ptr::NonNull<u8>,
            old_layout: std::alloc::Layout,
            layout: std::alloc::Layout,
        ) -> Result<std::ptr::NonNull<u8>, AllocError> {
            unsafe {
                let new_ptr = std::alloc::realloc(ptr.as_ptr(), old_layout, layout.size());
                std::ptr::NonNull::new(new_ptr).ok_or(AllocError::AllocationFailed)
            }
        }

        fn deallocate(&self, ptr: std::ptr::NonNull<u8>, layout: std::alloc::Layout) {
            unsafe {
                std::alloc::dealloc(ptr.as_ptr(), layout);
            }
        }
    }

    fn create_test_context() -> (Interpreter, InterpreterState) {
        let mut store = Store::new(1, []).unwrap();

        // Create a minimal valid module
        let module = Module {
            name: "test".try_into().unwrap(),
            types: crate::Vec::zero(),
            functions: crate::Vec::zero(),
            table: crate::Vec::zero(),
            memory: Some(MemoryKind::Allocate {
                ty: crate::MemType { min: 1, max: 1 },
                index: 0,
            }),
            globals: crate::Vec::zero(),
            data: crate::Vec::zero(),
            start: None,
            imports: crate::Vec::zero(),
            exports: crate::Vec::zero(),
            table_defined: false,
        };

        store.modules.push(crate::Box::new(module).unwrap());
        store.finish(&TestAllocator).unwrap();

        let state = InterpreterState::new(&mut store, 0, 1024);
        let interpreter = Interpreter::new(store);

        (interpreter, state)
    }

    // Helper macro for testing operations
    macro_rules! test_op {
        // i32 unary operation
        ($test_name:ident, $op:ident, i32: $input:expr => $expected:expr) => {
            #[test]
            fn $test_name() {
                let (interpreter, mut state) = create_test_context();

                state.stack.write_u32(0, $input);
                state.sp = 1;

                (&interpreter).$op(&mut state).unwrap();

                assert_eq!(state.sp, 1);
                assert_eq!(state.stack.read_u32(0), $expected);
            }
        };

        // i32 binary operation
        ($test_name:ident, $op:ident, i32, i32: $a:expr, $b:expr => $expected:expr) => {
            #[test]
            fn $test_name() {
                let (interpreter, mut state) = create_test_context();

                state.stack.write_u32(0, $a);
                state.stack.write_u32(1, $b);
                state.sp = 2;

                (&interpreter).$op(&mut state).unwrap();

                assert_eq!(state.sp, 1);
                assert_eq!(state.stack.read_u32(0), $expected);
            }
        };

        // i64 unary operation (2 words)
        ($test_name:ident, $op:ident, i64: $input:expr => $expected:expr) => {
            #[test]
            fn $test_name() {
                let (interpreter, mut state) = create_test_context();

                let input_val = $input as u64;
                state.stack.write_u64(0, input_val);
                state.sp = 2;

                (&interpreter).$op(&mut state).unwrap();

                assert_eq!(state.sp, 2);
                let result = state.stack.read_u64(0);
                assert_eq!(result, $expected as u64);
            }
        };

        // i64 unary bool operation (2 words)
        ($test_name:ident, $op:ident, i64 bool: $input:expr => $expected:expr) => {
            #[test]
            fn $test_name() {
                let (interpreter, mut state) = create_test_context();

                let input_val = $input as u64;
                state.stack.write_u64(0, input_val);
                state.sp = 2;

                (&interpreter).$op(&mut state).unwrap();

                assert_eq!(state.sp, 1);
                let result = state.stack.read_u32(0);
                assert_eq!(result, $expected as u32);
            }
        };

        // i64 binary operation (4 words -> 2 words)
        ($test_name:ident, $op:ident, i64, i64: $a:expr, $b:expr => $expected:expr) => {
            #[test]
            fn $test_name() {
                let (interpreter, mut state) = create_test_context();

                let a_val = $a as u64;
                let b_val = $b as u64;
                state.stack.write_u64(0, a_val);
                state.stack.write_u64(2, b_val);
                state.sp = 4;

                (&interpreter).$op(&mut state).unwrap();

                assert_eq!(state.sp, 2);
                let result = state.stack.read_u64(0);
                assert_eq!(result, $expected as u64);
            }
        };

        // f32 unary operation
        ($test_name:ident, $op:ident, f32: $input:expr => $expected:expr) => {
            #[test]
            fn $test_name() {
                let (interpreter, mut state) = create_test_context();

                state.stack.write_f32(0, $input);
                state.sp = 1;

                (&interpreter).$op(&mut state).unwrap();

                assert_eq!(state.sp, 1);
                let result = state.stack.read_f32(0);
                assert!((result - $expected).abs() < 0.0001);
            }
        };

        // f32 binary operation
        ($test_name:ident, $op:ident, f32, f32: $a:expr, $b:expr => $expected:expr) => {
            #[test]
            fn $test_name() {
                let (interpreter, mut state) = create_test_context();

                state.stack.write_f32(0, $a);
                state.stack.write_f32(1, $b);
                state.sp = 2;

                (&interpreter).$op(&mut state).unwrap();

                assert_eq!(state.sp, 1);
                let result = state.stack.read_f32(0);
                assert!((result - $expected).abs() < 0.0001);
            }
        };

        // f64 unary operation
        ($test_name:ident, $op:ident, f64: $input:expr => $expected:expr) => {
            #[test]
            fn $test_name() {
                let (interpreter, mut state) = create_test_context();

                state.stack.write_f64(0, $input);
                state.sp = 2;

                (&interpreter).$op(&mut state).unwrap();

                assert_eq!(state.sp, 2);
                let result = state.stack.read_f64(0);
                assert!((result - $expected).abs() < 0.0001);
            }
        };

        // f64 binary operation
        ($test_name:ident, $op:ident, f64, f64: $a:expr, $b:expr => $expected:expr) => {
            #[test]
            fn $test_name() {
                let (interpreter, mut state) = create_test_context();

                state.stack.write_f64(0, $a);
                state.stack.write_f64(2, $b);
                state.sp = 4;

                (&interpreter).$op(&mut state).unwrap();

                assert_eq!(state.sp, 2);
                let result = state.stack.read_f64(0);
                assert!((result - $expected).abs() < 0.0001);
            }
        };
    }

    // ===== Const Operations =====
    #[test]
    fn test_i32_const() {
        let (interpreter, mut state) = create_test_context();

        (&interpreter).i32_const(42, &mut state).unwrap();
        assert_eq!(state.sp, 1);
        assert_eq!(state.stack.read_u32(0), 42);

        (&interpreter).i32_const(-1, &mut state).unwrap();
        assert_eq!(state.sp, 2);
        assert_eq!(state.stack.read_u32(1) as i32, -1);
    }

    #[test]
    fn test_i64_const() {
        let (interpreter, mut state) = create_test_context();

        (&interpreter)
            .i64_const(0x123456789ABCDEF0i64, &mut state)
            .unwrap();
        assert_eq!(state.sp, 2);
        assert_eq!(state.stack.read_u64(0), 0x123456789ABCDEF0u64);

        (&interpreter).i64_const(-1, &mut state).unwrap();
        assert_eq!(state.sp, 4);
        assert_eq!(state.stack.read_u64(2) as i64, -1);
    }

    #[test]
    fn test_f32_const() {
        let (interpreter, mut state) = create_test_context();

        (&interpreter).f32_const(3.14f32, &mut state).unwrap();
        assert_eq!(state.sp, 1);
        assert!((state.stack.read_f32(0) - 3.14f32).abs() < 0.0001);
    }

    #[test]
    fn test_f64_const() {
        let (interpreter, mut state) = create_test_context();

        (&interpreter)
            .f64_const(3.14159265358979, &mut state)
            .unwrap();
        assert_eq!(state.sp, 2);
        assert!((state.stack.read_f64(0) - 3.14159265358979).abs() < 0.0001);
    }

    // ===== Memory Load Operations =====
    #[test]
    fn test_i32_load() {
        let (interpreter, mut state) = create_test_context();

        // Store a value in memory
        state.memory.store_u32(100, 0x12345678).unwrap();

        // Push address onto stack
        state.stack.write_u32(0, 100);
        state.sp = 1;

        (&interpreter)
            .i32_load(
                MemArg {
                    align: 2,
                    offset: 0,
                },
                &mut state,
            )
            .unwrap();

        assert_eq!(state.sp, 1);
        assert_eq!(state.stack.read_u32(0), 0x12345678);
    }

    #[test]
    fn test_i32_load_with_offset() {
        let (interpreter, mut state) = create_test_context();

        state.memory.store_u32(108, 0xDEADBEEF).unwrap();

        state.stack.write_u32(0, 100);
        state.sp = 1;

        (&interpreter)
            .i32_load(
                MemArg {
                    align: 2,
                    offset: 8,
                },
                &mut state,
            )
            .unwrap();

        assert_eq!(state.stack.read_u32(0), 0xDEADBEEF);
    }

    #[test]
    fn test_i64_load() {
        let (interpreter, mut state) = create_test_context();

        state.memory.store_u64(100, 0x123456789ABCDEF0).unwrap();

        state.stack.write_u32(0, 100);
        state.sp = 1;

        (&interpreter)
            .i64_load(
                MemArg {
                    align: 3,
                    offset: 0,
                },
                &mut state,
            )
            .unwrap();

        assert_eq!(state.sp, 2);
        assert_eq!(state.stack.read_u64(0), 0x123456789ABCDEF0);
    }

    #[test]
    fn test_i32_load8_s() {
        let (interpreter, mut state) = create_test_context();

        state.memory.store_u8(100, 0xFF).unwrap(); // -1 in i8

        state.stack.write_u32(0, 100);
        state.sp = 1;

        (&interpreter)
            .i32_load8_s(
                MemArg {
                    align: 0,
                    offset: 0,
                },
                &mut state,
            )
            .unwrap();

        assert_eq!(state.stack.read_u32(0) as i32, -1);
    }

    #[test]
    fn test_i32_load8_u() {
        let (interpreter, mut state) = create_test_context();

        state.memory.store_u8(100, 0xFF).unwrap();

        state.stack.write_u32(0, 100);
        state.sp = 1;

        (&interpreter)
            .i32_load8_u(
                MemArg {
                    align: 0,
                    offset: 0,
                },
                &mut state,
            )
            .unwrap();

        assert_eq!(state.stack.read_u32(0), 0xFF);
    }

    #[test]
    fn test_i32_load16_s() {
        let (interpreter, mut state) = create_test_context();

        state.memory.store_u16(100, 0xFFFF).unwrap(); // -1 in i16

        state.stack.write_u32(0, 100);
        state.sp = 1;

        (&interpreter)
            .i32_load16_s(
                MemArg {
                    align: 1,
                    offset: 0,
                },
                &mut state,
            )
            .unwrap();

        assert_eq!(state.stack.read_u32(0) as i32, -1);
    }

    #[test]
    fn test_i32_load16_u() {
        let (interpreter, mut state) = create_test_context();

        state.memory.store_u16(100, 0xFFFF).unwrap();

        state.stack.write_u32(0, 100);
        state.sp = 1;

        (&interpreter)
            .i32_load16_u(
                MemArg {
                    align: 1,
                    offset: 0,
                },
                &mut state,
            )
            .unwrap();

        assert_eq!(state.stack.read_u32(0), 0xFFFF);
    }

    #[test]
    fn test_i64_load8_s() {
        let (interpreter, mut state) = create_test_context();

        state.memory.store_u8(100, 0xFF).unwrap();

        state.stack.write_u32(0, 100);
        state.sp = 1;

        (&interpreter)
            .i64_load8_s(
                MemArg {
                    align: 0,
                    offset: 0,
                },
                &mut state,
            )
            .unwrap();

        assert_eq!(state.sp, 2);
        assert_eq!(state.stack.read_u64(0) as i64, -1);
    }

    #[test]
    fn test_i64_load32_u() {
        let (interpreter, mut state) = create_test_context();

        state.memory.store_u32(100, 0xDEADBEEF).unwrap();

        state.stack.write_u32(0, 100);
        state.sp = 1;

        (&interpreter)
            .i64_load32_u(
                MemArg {
                    align: 2,
                    offset: 0,
                },
                &mut state,
            )
            .unwrap();

        assert_eq!(state.sp, 2);
        assert_eq!(state.stack.read_u64(0), 0xDEADBEEF);
    }

    // ===== Memory Store Operations =====
    #[test]
    fn test_i32_store() {
        let (interpreter, mut state) = create_test_context();

        state.stack.write_u32(0, 100); // address
        state.stack.write_u32(1, 0x12345678); // value
        state.sp = 2;

        (&interpreter)
            .i32_store(
                MemArg {
                    align: 2,
                    offset: 0,
                },
                &mut state,
            )
            .unwrap();

        assert_eq!(state.sp, 0);
        assert_eq!(state.memory.load_u32(100).unwrap(), 0x12345678);
    }

    #[test]
    fn test_i32_store_with_offset() {
        let (interpreter, mut state) = create_test_context();

        state.stack.write_u32(0, 100);
        state.stack.write_u32(1, 0xDEADBEEF);
        state.sp = 2;

        (&interpreter)
            .i32_store(
                MemArg {
                    align: 2,
                    offset: 8,
                },
                &mut state,
            )
            .unwrap();

        assert_eq!(state.memory.load_u32(108).unwrap(), 0xDEADBEEF);
    }

    #[test]
    fn test_i64_store() {
        let (interpreter, mut state) = create_test_context();

        state.stack.write_u32(0, 100); // address
        state.stack.write_u64(1, 0x123456789ABCDEF0); // value
        state.sp = 3;

        (&interpreter)
            .i64_store(
                MemArg {
                    align: 3,
                    offset: 0,
                },
                &mut state,
            )
            .unwrap();

        assert_eq!(state.sp, 0);
        assert_eq!(state.memory.load_u64(100).unwrap(), 0x123456789ABCDEF0);
    }

    #[test]
    fn test_i32_store8() {
        let (interpreter, mut state) = create_test_context();

        state.stack.write_u32(0, 100);
        state.stack.write_u32(1, 0x123456FF);
        state.sp = 2;

        (&interpreter)
            .i32_store8(
                MemArg {
                    align: 0,
                    offset: 0,
                },
                &mut state,
            )
            .unwrap();

        assert_eq!(state.sp, 0);
        assert_eq!(state.memory.load_u8(100).unwrap(), 0xFF);
    }

    #[test]
    fn test_i32_store16() {
        let (interpreter, mut state) = create_test_context();

        state.stack.write_u32(0, 100);
        state.stack.write_u32(1, 0x1234FFFF);
        state.sp = 2;

        (&interpreter)
            .i32_store16(
                MemArg {
                    align: 1,
                    offset: 0,
                },
                &mut state,
            )
            .unwrap();

        assert_eq!(state.sp, 0);
        assert_eq!(state.memory.load_u16(100).unwrap(), 0xFFFF);
    }

    #[test]
    fn test_i64_store8() {
        let (interpreter, mut state) = create_test_context();

        state.stack.write_u32(0, 100);
        state.stack.write_u64(1, 0x123456789ABCDEFF);
        state.sp = 3;

        (&interpreter)
            .i64_store8(
                MemArg {
                    align: 0,
                    offset: 0,
                },
                &mut state,
            )
            .unwrap();

        assert_eq!(state.sp, 0);
        assert_eq!(state.memory.load_u8(100).unwrap(), 0xFF);
    }

    #[test]
    fn test_i64_store32() {
        let (interpreter, mut state) = create_test_context();

        state.stack.write_u32(0, 100);
        state.stack.write_u64(1, 0x12345678DEADBEEF);
        state.sp = 3;

        (&interpreter)
            .i64_store32(
                MemArg {
                    align: 2,
                    offset: 0,
                },
                &mut state,
            )
            .unwrap();

        assert_eq!(state.sp, 0);
        assert_eq!(state.memory.load_u32(100).unwrap(), 0xDEADBEEF);
    }

    // ===== i32 Test/Relational Operations =====
    test_op!(test_i32_eqz_zero, i32_eqz, i32: 0 => 1);
    test_op!(test_i32_eqz_nonzero, i32_eqz, i32: 42 => 0);
    test_op!(test_i32_eq_true, i32_eq, i32, i32: 42, 42 => 1);
    test_op!(test_i32_eq_false, i32_eq, i32, i32: 42, 43 => 0);
    test_op!(test_i32_ne_true, i32_ne, i32, i32: 42, 43 => 1);
    test_op!(test_i32_ne_false, i32_ne, i32, i32: 42, 42 => 0);
    test_op!(test_i32_lt_s_true, i32_lt_s, i32, i32: (-5i32) as u32, 5 => 1);
    test_op!(test_i32_lt_s_false, i32_lt_s, i32, i32: 5, 5 => 0);
    test_op!(test_i32_lt_u_true, i32_lt_u, i32, i32: 5, 10 => 1);
    test_op!(test_i32_lt_u_false, i32_lt_u, i32, i32: 10, 5 => 0);
    test_op!(test_i32_gt_s_true, i32_gt_s, i32, i32: 5, (-5i32) as u32 => 1);
    test_op!(test_i32_le_s_true, i32_le_s, i32, i32: 5, 5 => 1);
    test_op!(test_i32_ge_s_true, i32_ge_s, i32, i32: 5, 5 => 1);

    // ===== i32 Arithmetic Operations =====
    test_op!(test_i32_clz, i32_clz, i32: 0x00F00000 => 8);
    test_op!(test_i32_ctz, i32_ctz, i32: 0x00000F00 => 8);
    test_op!(test_i32_popcnt, i32_popcnt, i32: 0x0F0F0F0F => 16);
    test_op!(test_i32_add, i32_add, i32, i32: 5, 10 => 15);
    test_op!(test_i32_add_overflow, i32_add, i32, i32: 0xFFFFFFFF, 1 => 0);
    test_op!(test_i32_sub, i32_sub, i32, i32: 10, 5 => 5);
    test_op!(test_i32_mul, i32_mul, i32, i32: 5, 10 => 50);
    test_op!(test_i32_div_s, i32_div_s, i32, i32: 20, 4 => 5);
    test_op!(test_i32_div_s_negative, i32_div_s, i32, i32: (-20i32) as u32, 4 => (-5i32) as u32);
    test_op!(test_i32_div_u, i32_div_u, i32, i32: 20, 4 => 5);
    test_op!(test_i32_rem_s, i32_rem_s, i32, i32: 17, 5 => 2);
    test_op!(test_i32_rem_u, i32_rem_u, i32, i32: 17, 5 => 2);
    test_op!(test_i32_and, i32_and, i32, i32: 0xFF00, 0x0FF0 => 0x0F00);
    test_op!(test_i32_or, i32_or, i32, i32: 0xFF00, 0x00FF => 0xFFFF);
    test_op!(test_i32_xor, i32_xor, i32, i32: 0xFFFF, 0x0FF0 => 0xF00F);
    test_op!(test_i32_shl, i32_shl, i32, i32: 1, 8 => 256);
    test_op!(test_i32_shr_s, i32_shr_s, i32, i32: (-256i32) as u32, 8 => (-1i32) as u32);
    test_op!(test_i32_shr_u, i32_shr_u, i32, i32: 256, 8 => 1);
    test_op!(test_i32_rotl, i32_rotl, i32, i32: 0x80000001, 1 => 3);
    test_op!(test_i32_rotr, i32_rotr, i32, i32: 0x80000001, 1 => 0xC0000000);

    // ===== i64 Test/Relational Operations =====
    test_op!(test_i64_eqz_zero, i64_eqz, i64 bool: 0i64 => 1i32);
    test_op!(test_i64_eqz_nonzero, i64_eqz, i64 bool: 42i64 => 0i32);
    test_op!(test_i64_add, i64_add, i64, i64: 5i64, 10i64 => 15i64);
    test_op!(test_i64_sub, i64_sub, i64, i64: 10i64, 5i64 => 5i64);
    test_op!(test_i64_mul, i64_mul, i64, i64: 5i64, 10i64 => 50i64);
    test_op!(test_i64_div_s, i64_div_s, i64, i64: 20i64, 4i64 => 5i64);
    test_op!(test_i64_rem_s, i64_rem_s, i64, i64: 17i64, 5i64 => 2i64);
    test_op!(test_i64_and, i64_and, i64, i64: 0xFF00i64, 0x0FF0i64 => 0x0F00i64);
    test_op!(test_i64_or, i64_or, i64, i64: 0xFF00i64, 0x00FFi64 => 0xFFFFi64);
    test_op!(test_i64_xor, i64_xor, i64, i64: 0xFFFFi64, 0x0FF0i64 => 0xF00Fi64);
    test_op!(test_i64_shl, i64_shl, i64, i64: 1i64, 8i64 => 256i64);
    test_op!(test_i64_clz, i64_clz, i64: 0x00F0000000000000i64 => 8i64);
    test_op!(test_i64_ctz, i64_ctz, i64: 0x0000000000000F00i64 => 8i64);
    test_op!(test_i64_popcnt, i64_popcnt, i64: 0x0F0F0F0F0F0F0F0Fi64 => 32i64);

    // ===== f32 Operations =====
    test_op!(test_f32_abs_positive, f32_abs, f32: 3.14f32 => 3.14f32);
    test_op!(test_f32_abs_negative, f32_abs, f32: -3.14f32 => 3.14f32);
    test_op!(test_f32_neg, f32_neg, f32: 3.14f32 => -3.14f32);
    test_op!(test_f32_ceil, f32_ceil, f32: 3.14f32 => 4.0f32);
    test_op!(test_f32_floor, f32_floor, f32: 3.14f32 => 3.0f32);
    test_op!(test_f32_trunc, f32_trunc, f32: 3.99f32 => 3.0f32);
    test_op!(test_f32_sqrt, f32_sqrt, f32: 16.0f32 => 4.0f32);
    test_op!(test_f32_add, f32_add, f32, f32: 1.5f32, 2.5f32 => 4.0f32);
    test_op!(test_f32_sub, f32_sub, f32, f32: 5.5f32, 2.5f32 => 3.0f32);
    test_op!(test_f32_mul, f32_mul, f32, f32: 2.5f32, 4.0f32 => 10.0f32);
    test_op!(test_f32_div, f32_div, f32, f32: 10.0f32, 2.5f32 => 4.0f32);

    // ===== f64 Operations =====
    test_op!(test_f64_abs_positive, f64_abs, f64: 3.14 => 3.14);
    test_op!(test_f64_abs_negative, f64_abs, f64: -3.14 => 3.14);
    test_op!(test_f64_neg, f64_neg, f64: 3.14 => -3.14);
    test_op!(test_f64_ceil, f64_ceil, f64: 3.14 => 4.0);
    test_op!(test_f64_floor, f64_floor, f64: 3.14 => 3.0);
    test_op!(test_f64_trunc, f64_trunc, f64: 3.99 => 3.0);
    test_op!(test_f64_sqrt, f64_sqrt, f64: 16.0 => 4.0);
    test_op!(test_f64_add, f64_add, f64, f64: 1.5, 2.5 => 4.0);
    test_op!(test_f64_sub, f64_sub, f64, f64: 5.5, 2.5 => 3.0);
    test_op!(test_f64_mul, f64_mul, f64, f64: 2.5, 4.0 => 10.0);
    test_op!(test_f64_div, f64_div, f64, f64: 10.0, 2.5 => 4.0);

    // ===== Conversion Operations =====
    #[test]
    fn test_i32_wrap_i64() {
        let (interpreter, mut state) = create_test_context();

        state.stack.write_u64(0, 0x123456789ABCDEF0);
        state.sp = 2;

        (&interpreter).i32_wrap_i64(&mut state).unwrap();

        assert_eq!(state.sp, 1);
        assert_eq!(state.stack.read_u32(0), 0x9ABCDEF0);
    }

    #[test]
    fn test_i32_trunc_f32_s() {
        let (interpreter, mut state) = create_test_context();

        state.stack.write_f32(0, 3.99);
        state.sp = 1;

        (&interpreter).i32_trunc_f32_s(&mut state).unwrap();

        assert_eq!(state.sp, 1);
        assert_eq!(state.stack.read_u32(0) as i32, 3);
    }

    #[test]
    fn test_i32_trunc_f32_u() {
        let (interpreter, mut state) = create_test_context();

        state.stack.write_f32(0, 3.99);
        state.sp = 1;

        (&interpreter).i32_trunc_f32_u(&mut state).unwrap();

        assert_eq!(state.sp, 1);
        assert_eq!(state.stack.read_u32(0), 3);
    }

    #[test]
    fn test_i32_trunc_f64_s() {
        let (interpreter, mut state) = create_test_context();

        state.stack.write_f64(0, 3.99);
        state.sp = 2;

        (&interpreter).i32_trunc_f64_s(&mut state).unwrap();

        assert_eq!(state.sp, 1);
        assert_eq!(state.stack.read_u32(0) as i32, 3);
    }

    #[test]
    fn test_i64_extend_i32_s() {
        let (interpreter, mut state) = create_test_context();

        state.stack.write_u32(0, (-1i32) as u32);
        state.sp = 1;

        (&interpreter).i64_extend_i32_s(&mut state).unwrap();

        assert_eq!(state.sp, 2);
        assert_eq!(state.stack.read_u64(0) as i64, -1);
    }

    #[test]
    fn test_i64_extend_i32_u() {
        let (interpreter, mut state) = create_test_context();

        state.stack.write_u32(0, 0xFFFFFFFF);
        state.sp = 1;

        (&interpreter).i64_extend_i32_u(&mut state).unwrap();

        assert_eq!(state.sp, 2);
        assert_eq!(state.stack.read_u64(0), 0xFFFFFFFF);
    }

    #[test]
    fn test_i64_trunc_f32_s() {
        let (interpreter, mut state) = create_test_context();

        state.stack.write_f32(0, 123.99);
        state.sp = 1;

        (&interpreter).i64_trunc_f32_s(&mut state).unwrap();

        assert_eq!(state.sp, 2);
        assert_eq!(state.stack.read_u64(0) as i64, 123);
    }

    #[test]
    fn test_i64_trunc_f64_s() {
        let (interpreter, mut state) = create_test_context();

        state.stack.write_f64(0, 123.99);
        state.sp = 2;

        (&interpreter).i64_trunc_f64_s(&mut state).unwrap();

        assert_eq!(state.sp, 2);
        assert_eq!(state.stack.read_u64(0) as i64, 123);
    }

    #[test]
    fn test_f32_convert_i32_s() {
        let (interpreter, mut state) = create_test_context();

        state.stack.write_u32(0, (-42i32) as u32);
        state.sp = 1;

        (&interpreter).f32_convert_i32_s(&mut state).unwrap();

        assert_eq!(state.sp, 1);
        assert!((state.stack.read_f32(0) - (-42.0f32)).abs() < 0.0001);
    }

    #[test]
    fn test_f32_convert_i32_u() {
        let (interpreter, mut state) = create_test_context();

        state.stack.write_u32(0, 42);
        state.sp = 1;

        (&interpreter).f32_convert_i32_u(&mut state).unwrap();

        assert_eq!(state.sp, 1);
        assert!((state.stack.read_f32(0) - 42.0f32).abs() < 0.0001);
    }

    #[test]
    fn test_f32_convert_i64_s() {
        let (interpreter, mut state) = create_test_context();

        state.stack.write_u64(0, (-42i64) as u64);
        state.sp = 2;

        (&interpreter).f32_convert_i64_s(&mut state).unwrap();

        assert_eq!(state.sp, 1);
        assert!((state.stack.read_f32(0) - (-42.0f32)).abs() < 0.0001);
    }

    #[test]
    fn test_f32_demote_f64() {
        let (interpreter, mut state) = create_test_context();

        state.stack.write_f64(0, 3.141592653589793);
        state.sp = 2;

        (&interpreter).f32_demote_f64(&mut state).unwrap();

        assert_eq!(state.sp, 1);
        assert!((state.stack.read_f32(0) - 3.14159265f32).abs() < 0.0001);
    }

    #[test]
    fn test_f64_convert_i32_s() {
        let (interpreter, mut state) = create_test_context();

        state.stack.write_u32(0, (-42i32) as u32);
        state.sp = 1;

        (&interpreter).f64_convert_i32_s(&mut state).unwrap();

        assert_eq!(state.sp, 2);
        assert!((state.stack.read_f64(0) - (-42.0)).abs() < 0.0001);
    }

    #[test]
    fn test_f64_convert_i64_s() {
        let (interpreter, mut state) = create_test_context();

        state.stack.write_u64(0, (-42i64) as u64);
        state.sp = 2;

        (&interpreter).f64_convert_i64_s(&mut state).unwrap();

        assert_eq!(state.sp, 2);
        assert!((state.stack.read_f64(0) - (-42.0)).abs() < 0.0001);
    }

    #[test]
    fn test_f64_promote_f32() {
        let (interpreter, mut state) = create_test_context();

        state.stack.write_f32(0, 3.14159f32);
        state.sp = 1;

        (&interpreter).f64_promote_f32(&mut state).unwrap();

        assert_eq!(state.sp, 2);
        assert!((state.stack.read_f64(0) - 3.14159).abs() < 0.001);
    }

    // ===== Parametric Operations =====
    #[test]
    fn test_drop() {
        let (interpreter, mut state) = create_test_context();

        state.stack.write_u32(0, 42);
        state.sp = 1;

        (&interpreter).drop(ValType::I32, &mut state).unwrap();

        assert_eq!(state.sp, 0);
    }

    #[test]
    fn test_select_true() {
        let (interpreter, mut state) = create_test_context();

        state.stack.write_u32(0, 10); // val1
        state.stack.write_u32(1, 20); // val2
        state.stack.write_u32(2, 1); // condition (true)
        state.sp = 3;

        (&interpreter).select(ValType::I32, &mut state).unwrap();

        assert_eq!(state.sp, 1);
        assert_eq!(state.stack.read_u32(0), 10);
    }

    #[test]
    fn test_select_false() {
        let (interpreter, mut state) = create_test_context();

        state.stack.write_u32(0, 10); // val1
        state.stack.write_u32(1, 20); // val2
        state.stack.write_u32(2, 0); // condition (false)
        state.sp = 3;

        (&interpreter).select(ValType::I32, &mut state).unwrap();

        assert_eq!(state.sp, 1);
        assert_eq!(state.stack.read_u32(0), 20);
    }

    // ===== Memory Size/Grow Operations =====
    #[test]
    fn test_memory_size() {
        let (interpreter, mut state) = create_test_context();

        state.sp = 0;

        (&interpreter).memory_size(&mut state).unwrap();

        assert_eq!(state.sp, 1);
        assert_eq!(state.stack.read_u32(0), 1); // 65536 / 65536 = 1 page
    }

    // ===== Float Comparison Operations =====
    #[test]
    fn test_f32_eq() {
        let (interpreter, mut state) = create_test_context();

        state.stack.write_f32(0, 3.14);
        state.stack.write_f32(1, 3.14);
        state.sp = 2;

        (&interpreter).f32_eq(&mut state).unwrap();

        assert_eq!(state.sp, 1);
        assert_eq!(state.stack.read_u32(0), 1);
    }

    #[test]
    fn test_f32_ne() {
        let (interpreter, mut state) = create_test_context();

        state.stack.write_f32(0, 3.14);
        state.stack.write_f32(1, 2.71);
        state.sp = 2;

        (&interpreter).f32_ne(&mut state).unwrap();

        assert_eq!(state.sp, 1);
        assert_eq!(state.stack.read_u32(0), 1);
    }

    #[test]
    fn test_f32_lt() {
        let (interpreter, mut state) = create_test_context();

        state.stack.write_f32(0, 2.0);
        state.stack.write_f32(1, 3.0);
        state.sp = 2;

        (&interpreter).f32_lt(&mut state).unwrap();

        assert_eq!(state.sp, 1);
        assert_eq!(state.stack.read_u32(0), 1);
    }

    #[test]
    fn test_f64_eq() {
        let (interpreter, mut state) = create_test_context();

        state.stack.write_f64(0, 3.14159);
        state.stack.write_f64(2, 3.14159);
        state.sp = 4;

        (&interpreter).f64_eq(&mut state).unwrap();

        assert_eq!(state.sp, 1);
        assert_eq!(state.stack.read_u32(0), 1);
    }

    // ===== i64 Comparison Operations (return i32) =====
    #[test]
    fn test_i64_eq_true() {
        let (interpreter, mut state) = create_test_context();

        state.stack.write_u64(0, 0x123456789ABCDEF0);
        state.stack.write_u64(2, 0x123456789ABCDEF0);
        state.sp = 4;

        (&interpreter).i64_eq(&mut state).unwrap();

        assert_eq!(state.sp, 1);
        assert_eq!(state.stack.read_u32(0), 1);
    }

    #[test]
    fn test_i64_ne_true() {
        let (interpreter, mut state) = create_test_context();

        state.stack.write_u64(0, 0x123456789ABCDEF0);
        state.stack.write_u64(2, 0xFEDCBA9876543210);
        state.sp = 4;

        (&interpreter).i64_ne(&mut state).unwrap();

        assert_eq!(state.sp, 1);
        assert_eq!(state.stack.read_u32(0), 1);
    }

    #[test]
    fn test_i64_lt_s_true() {
        let (interpreter, mut state) = create_test_context();

        state.stack.write_u64(0, (-5i64) as u64);
        state.stack.write_u64(2, 5);
        state.sp = 4;

        (&interpreter).i64_lt_s(&mut state).unwrap();

        assert_eq!(state.sp, 1);
        assert_eq!(state.stack.read_u32(0), 1);
    }

    // ===== Additional Edge Cases =====
    #[test]
    fn test_i32_add_wrapping() {
        let (interpreter, mut state) = create_test_context();

        state.stack.write_u32(0, 0xFFFFFFFF);
        state.stack.write_u32(1, 2);
        state.sp = 2;

        (&interpreter).i32_add(&mut state).unwrap();

        assert_eq!(state.sp, 1);
        assert_eq!(state.stack.read_u32(0), 1); // Wraps around
    }

    #[test]
    fn test_i64_add_wrapping() {
        let (interpreter, mut state) = create_test_context();

        state.stack.write_u64(0, 0xFFFFFFFFFFFFFFFF);
        state.stack.write_u64(2, 2);
        state.sp = 4;

        (&interpreter).i64_add(&mut state).unwrap();

        assert_eq!(state.sp, 2);
        assert_eq!(state.stack.read_u64(0), 1); // Wraps around
    }
}
