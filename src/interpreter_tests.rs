#[cfg(test)]
#[allow(clippy::approx_constant)]
#[allow(clippy::excessive_precision)]
mod tests {
    use crate::{
        AllocError, BaseVisitor, Engine, Interpreter, InterpreterResult, InterpreterRunner,
        InvokeError, IrVisitor, MemArg, MemType, Memory, MemoryKind, Module, ModuleRef, ResultType,
        ResumeError, ValType, Value, WasmRef,
    };
    use crate::{
        CodeBuilder, CompilerOptions, ControlFrame, DecoderStack, HostFunction, HostFunctionBreak,
        HostFunctionResult, HostModule, HostPause, JumpTarget, OperandType,
    };
    use core::ops::ControlFlow;

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

    fn with_test_context<F>(f: F)
    where
        F: FnOnce(&mut Engine),
    {
        let mut engine = Engine::new(1024, 1, crate::Vec::zero()).unwrap();

        // Create a minimal valid module
        let module = Module {
            name: "test".try_into().unwrap(),
            types: crate::Vec::zero().into(),
            functions: crate::Vec::zero().into(),
            table: None,
            memory: Some(MemoryKind::Owned(
                crate::Rc::new(
                    Memory::new(
                        MemType {
                            initial_pages: 1,
                            max_pages: None,
                            page_size: crate::MemPageSize::_65536,
                        },
                        crate::Rc::new(TestAllocator)
                            .unwrap()
                            .into_wasm_memory_allocator(),
                    )
                    .unwrap(),
                )
                .unwrap(),
            )),
            globals: crate::Vec::zero().into(),
            start: None,
            imports: crate::Vec::zero().into(),
            exports: crate::Vec::zero().into(),
        };

        let module_ref = engine.push_module(module).unwrap();
        if let Some(start) = engine.module_start(module_ref) {
            engine.invoke(start, &[]).unwrap();
            match Interpreter.run(&[], &mut engine, usize::MAX) {
                InterpreterResult::Finished => {}
                InterpreterResult::OutOfFuel => panic!("insufficient fuel for initialization"),
                InterpreterResult::Trap(t) => panic!("trap during initialization {t:?}"),
                InterpreterResult::Pause => panic!("pause during init"),
            }
        }

        engine.memory = engine.store.get_memory(ModuleRef(0)).clone();
        engine.table = engine.store.get_table(ModuleRef(0)).clone();

        f(&mut engine);
    }

    // Helper macro for testing operations
    macro_rules! test_op {
        // i32 unary operation
        ($(#[$attr:meta])* $test_name:ident, $op:ident, i32: $input:expr => $expected:expr) => {
            #[test]
            $(#[$attr])*
            fn $test_name() {
                with_test_context(|state| {
                    state.stack.write_u32(0, $input);
                    state.sp = 1;

                    Interpreter.$op(state).unwrap();

                    assert_eq!(state.sp, 1);
                    assert_eq!(state.stack.read_u32(0), $expected);
                });
            }
        };

        // i32 binary operation
        ($(#[$attr:meta])* $test_name:ident, $op:ident, i32, i32: $a:expr, $b:expr => $expected:expr) => {
            #[test]
            $(#[$attr])*
            fn $test_name() {
                with_test_context(|state| {
                    state.stack.write_u32(0, $a);
                    state.stack.write_u32(1, $b);
                    state.sp = 2;

                    Interpreter.$op(state).unwrap();

                    assert_eq!(state.sp, 1);
                    assert_eq!(state.stack.read_u32(0), $expected);
                });
            }
        };

        // i64 unary operation (2 words)
        ($(#[$attr:meta])* $test_name:ident, $op:ident, i64: $input:expr => $expected:expr) => {
            #[test]
            $(#[$attr])*
            fn $test_name() {
                with_test_context(|state| {
                    let input_val = $input as u64;
                    state.stack.write_u64(0, input_val);
                    state.sp = 2;

                    Interpreter.$op(state).unwrap();

                    assert_eq!(state.sp, 2);
                    let result = state.stack.read_u64(0);
                    assert_eq!(result, $expected as u64);
                });
            }
        };

        // i64 unary bool operation (2 words)
        ($(#[$attr:meta])* $test_name:ident, $op:ident, i64 bool: $input:expr => $expected:expr) => {
            #[test]
            $(#[$attr])*
            fn $test_name() {
                with_test_context(|state| {
                    let input_val = $input as u64;
                    state.stack.write_u64(0, input_val);
                    state.sp = 2;

                    Interpreter.$op(state).unwrap();

                    assert_eq!(state.sp, 1);
                    let result = state.stack.read_u32(0);
                    assert_eq!(result, $expected as u32);
                });
            }
        };

        // i64 binary operation (4 words -> 2 words)
        ($(#[$attr:meta])* $test_name:ident, $op:ident, i64, i64: $a:expr, $b:expr => $expected:expr) => {
            #[test]
            $(#[$attr])*
            fn $test_name() {
                with_test_context(|state| {
                    let a_val = $a as u64;
                    let b_val = $b as u64;
                    state.stack.write_u64(0, a_val);
                    state.stack.write_u64(2, b_val);
                    state.sp = 4;

                    Interpreter.$op(state).unwrap();

                    assert_eq!(state.sp, 2);
                    let result = state.stack.read_u64(0);
                    assert_eq!(result, $expected as u64);
                });
            }
        };

        // f32 unary operation
        ($(#[$attr:meta])* $test_name:ident, $op:ident, f32: $input:expr => $expected:expr) => {
            #[test]
            $(#[$attr])*
            fn $test_name() {
                with_test_context(|state| {
                    state.stack.write_f32(0, $input);
                    state.sp = 1;

                    Interpreter.$op(state).unwrap();

                    assert_eq!(state.sp, 1);
                    let result = state.stack.read_f32(0);
                    assert!((result - $expected).abs() < 0.0001);
                });
            }
        };

        // f32 binary operation
        ($(#[$attr:meta])* $test_name:ident, $op:ident, f32, f32: $a:expr, $b:expr => $expected:expr) => {
            #[test]
            $(#[$attr])*
            fn $test_name() {
                with_test_context(|state| {
                    state.stack.write_f32(0, $a);
                    state.stack.write_f32(1, $b);
                    state.sp = 2;

                    Interpreter.$op(state).unwrap();

                    assert_eq!(state.sp, 1);
                    let result = state.stack.read_f32(0);
                    assert!((result - $expected).abs() < 0.0001);
                });
            }
        };

        // f64 unary operation
        ($(#[$attr:meta])* $test_name:ident, $op:ident, f64: $input:expr => $expected:expr) => {
            #[test]
            $(#[$attr])*
            fn $test_name() {
                with_test_context(|state| {
                    state.stack.write_f64(0, $input);
                    state.sp = 2;

                    Interpreter.$op(state).unwrap();

                    assert_eq!(state.sp, 2);
                    let result = state.stack.read_f64(0);
                    assert!((result - $expected).abs() < 0.0001);
                });
            }
        };

        // f64 binary operation
        ($(#[$attr:meta])* $test_name:ident, $op:ident, f64, f64: $a:expr, $b:expr => $expected:expr) => {
            #[test]
            $(#[$attr])*
            fn $test_name() {
                with_test_context(|state| {
                    state.stack.write_f64(0, $a);
                    state.stack.write_f64(2, $b);
                    state.sp = 4;

                    Interpreter.$op(state).unwrap();

                    assert_eq!(state.sp, 2);
                    let result = state.stack.read_f64(0);
                    assert!((result - $expected).abs() < 0.0001);
                });
            }
        };
    }

    // ===== Const Operations =====
    #[test]
    fn test_i32_const() {
        with_test_context(|state| {
            Interpreter.i32_const(42, state).unwrap();
            assert_eq!(state.sp, 1);
            assert_eq!(state.stack.read_u32(0), 42);

            Interpreter.i32_const(-1, state).unwrap();
            assert_eq!(state.sp, 2);
            assert_eq!(state.stack.read_u32(1) as i32, -1);
        });
    }

    #[test]
    fn test_i64_const() {
        with_test_context(|state| {
            Interpreter.i64_const(0x123456789ABCDEF0i64, state).unwrap();
            assert_eq!(state.sp, 2);
            assert_eq!(state.stack.read_u64(0), 0x123456789ABCDEF0u64);

            Interpreter.i64_const(-1, state).unwrap();
            assert_eq!(state.sp, 4);
            assert_eq!(state.stack.read_u64(2) as i64, -1);
        });
    }

    #[test]
    fn test_f32_const() {
        with_test_context(|state| {
            Interpreter.f32_const(3.14f32, state).unwrap();
            assert_eq!(state.sp, 1);
            assert!((state.stack.read_f32(0) - 3.14f32).abs() < 0.0001);
        });
    }

    #[test]
    fn test_f64_const() {
        with_test_context(|state| {
            Interpreter.f64_const(3.14159265358979, state).unwrap();
            assert_eq!(state.sp, 2);
            assert!((state.stack.read_f64(0) - 3.14159265358979).abs() < 0.0001);
        });
    }

    // ===== Memory Load Operations =====
    #[test]
    fn test_i32_load() {
        with_test_context(|state| {
            // Store a value in memory
            state.memory.store_u32(100, 0x12345678).unwrap();

            // Push address onto stack
            state.stack.write_u32(0, 100);
            state.sp = 1;

            Interpreter
                .i32_load(
                    MemArg {
                        align: 2,
                        offset: 0,
                    },
                    state,
                )
                .unwrap();

            assert_eq!(state.sp, 1);
            assert_eq!(state.stack.read_u32(0), 0x12345678);
        });
    }

    #[test]
    fn test_i32_load_with_offset() {
        with_test_context(|state| {
            state.memory.store_u32(108, 0xDEADBEEF).unwrap();

            state.stack.write_u32(0, 100);
            state.sp = 1;

            Interpreter
                .i32_load(
                    MemArg {
                        align: 2,
                        offset: 8,
                    },
                    state,
                )
                .unwrap();

            assert_eq!(state.stack.read_u32(0), 0xDEADBEEF);
        });
    }

    #[test]
    fn test_i64_load() {
        with_test_context(|state| {
            state.memory.store_u64(100, 0x123456789ABCDEF0).unwrap();

            state.stack.write_u32(0, 100);
            state.sp = 1;

            Interpreter
                .i64_load(
                    MemArg {
                        align: 3,
                        offset: 0,
                    },
                    state,
                )
                .unwrap();

            assert_eq!(state.sp, 2);
            assert_eq!(state.stack.read_u64(0), 0x123456789ABCDEF0);
        });
    }

    #[test]
    fn test_i32_load8_s() {
        with_test_context(|state| {
            state.memory.store_u8(100, 0xFF).unwrap(); // -1 in i8

            state.stack.write_u32(0, 100);
            state.sp = 1;

            Interpreter
                .i32_load8_s(
                    MemArg {
                        align: 0,
                        offset: 0,
                    },
                    state,
                )
                .unwrap();

            assert_eq!(state.stack.read_u32(0) as i32, -1);
        });
    }

    #[test]
    fn test_i32_load8_u() {
        with_test_context(|state| {
            state.memory.store_u8(100, 0xFF).unwrap();

            state.stack.write_u32(0, 100);
            state.sp = 1;

            Interpreter
                .i32_load8_u(
                    MemArg {
                        align: 0,
                        offset: 0,
                    },
                    state,
                )
                .unwrap();

            assert_eq!(state.stack.read_u32(0), 0xFF);
        });
    }

    #[test]
    fn test_i32_load16_s() {
        with_test_context(|state| {
            state.memory.store_u16(100, 0xFFFF).unwrap(); // -1 in i16

            state.stack.write_u32(0, 100);
            state.sp = 1;

            Interpreter
                .i32_load16_s(
                    MemArg {
                        align: 1,
                        offset: 0,
                    },
                    state,
                )
                .unwrap();

            assert_eq!(state.stack.read_u32(0) as i32, -1);
        });
    }

    #[test]
    fn test_i32_load16_u() {
        with_test_context(|state| {
            state.memory.store_u16(100, 0xFFFF).unwrap();

            state.stack.write_u32(0, 100);
            state.sp = 1;

            Interpreter
                .i32_load16_u(
                    MemArg {
                        align: 1,
                        offset: 0,
                    },
                    state,
                )
                .unwrap();

            assert_eq!(state.stack.read_u32(0), 0xFFFF);
        });
    }

    #[test]
    fn test_i64_load8_s() {
        with_test_context(|state| {
            state.memory.store_u8(100, 0xFF).unwrap();

            state.stack.write_u32(0, 100);
            state.sp = 1;

            Interpreter
                .i64_load8_s(
                    MemArg {
                        align: 0,
                        offset: 0,
                    },
                    state,
                )
                .unwrap();

            assert_eq!(state.sp, 2);
            assert_eq!(state.stack.read_u64(0) as i64, -1);
        });
    }

    #[test]
    fn test_i64_load32_u() {
        with_test_context(|state| {
            state.memory.store_u32(100, 0xDEADBEEF).unwrap();

            state.stack.write_u32(0, 100);
            state.sp = 1;

            Interpreter
                .i64_load32_u(
                    MemArg {
                        align: 2,
                        offset: 0,
                    },
                    state,
                )
                .unwrap();

            assert_eq!(state.sp, 2);
            assert_eq!(state.stack.read_u64(0), 0xDEADBEEF);
        });
    }

    // ===== Memory Store Operations =====
    #[test]
    fn test_i32_store() {
        with_test_context(|state| {
            state.stack.write_u32(0, 100); // address
            state.stack.write_u32(1, 0x12345678); // value
            state.sp = 2;

            Interpreter
                .i32_store(
                    MemArg {
                        align: 2,
                        offset: 0,
                    },
                    state,
                )
                .unwrap();

            assert_eq!(state.sp, 0);
            assert_eq!(state.memory.load_u32(100).unwrap(), 0x12345678);
        });
    }

    #[test]
    fn test_i32_store_with_offset() {
        with_test_context(|state| {
            state.stack.write_u32(0, 100);
            state.stack.write_u32(1, 0xDEADBEEF);
            state.sp = 2;

            Interpreter
                .i32_store(
                    MemArg {
                        align: 2,
                        offset: 8,
                    },
                    state,
                )
                .unwrap();

            assert_eq!(state.memory.load_u32(108).unwrap(), 0xDEADBEEF);
        });
    }

    #[test]
    fn test_i64_store() {
        with_test_context(|state| {
            state.stack.write_u32(0, 100); // address
            state.stack.write_u64(1, 0x123456789ABCDEF0); // value
            state.sp = 3;

            Interpreter
                .i64_store(
                    MemArg {
                        align: 3,
                        offset: 0,
                    },
                    state,
                )
                .unwrap();

            assert_eq!(state.sp, 0);
            assert_eq!(state.memory.load_u64(100).unwrap(), 0x123456789ABCDEF0);
        });
    }

    #[test]
    fn test_i32_store8() {
        with_test_context(|state| {
            state.stack.write_u32(0, 100);
            state.stack.write_u32(1, 0x123456FF);
            state.sp = 2;

            Interpreter
                .i32_store8(
                    MemArg {
                        align: 0,
                        offset: 0,
                    },
                    state,
                )
                .unwrap();

            assert_eq!(state.sp, 0);
            assert_eq!(state.memory.load_u8(100).unwrap(), 0xFF);
        });
    }

    #[test]
    fn test_i32_store16() {
        with_test_context(|state| {
            state.stack.write_u32(0, 100);
            state.stack.write_u32(1, 0x1234FFFF);
            state.sp = 2;

            Interpreter
                .i32_store16(
                    MemArg {
                        align: 1,
                        offset: 0,
                    },
                    state,
                )
                .unwrap();

            assert_eq!(state.sp, 0);
            assert_eq!(state.memory.load_u16(100).unwrap(), 0xFFFF);
        });
    }

    #[test]
    fn test_i64_store8() {
        with_test_context(|state| {
            state.stack.write_u32(0, 100);
            state.stack.write_u64(1, 0x123456789ABCDEFF);
            state.sp = 3;

            Interpreter
                .i64_store8(
                    MemArg {
                        align: 0,
                        offset: 0,
                    },
                    state,
                )
                .unwrap();

            assert_eq!(state.sp, 0);
            assert_eq!(state.memory.load_u8(100).unwrap(), 0xFF);
        });
    }

    #[test]
    fn test_i64_store32() {
        with_test_context(|state| {
            state.stack.write_u32(0, 100);
            state.stack.write_u64(1, 0x12345678DEADBEEF);
            state.sp = 3;

            Interpreter
                .i64_store32(
                    MemArg {
                        align: 2,
                        offset: 0,
                    },
                    state,
                )
                .unwrap();

            assert_eq!(state.sp, 0);
            assert_eq!(state.memory.load_u32(100).unwrap(), 0xDEADBEEF);
        });
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
    test_op!(test_i32_gt_s_false, i32_gt_s, i32, i32: (-5i32) as u32, 5 => 0);
    test_op!(test_i32_gt_u_true, i32_gt_u, i32, i32: (-1i32) as u32, 1 => 1);
    test_op!(test_i32_gt_u_false, i32_gt_u, i32, i32: 1, (-1i32) as u32 => 0);
    test_op!(test_i32_le_s_false, i32_le_s, i32, i32: 6, 5 => 0);
    test_op!(test_i32_le_u_true, i32_le_u, i32, i32: 1, (-1i32) as u32 => 1);
    test_op!(test_i32_le_u_false, i32_le_u, i32, i32: (-1i32) as u32, 1 => 0);
    test_op!(test_i32_ge_s_false, i32_ge_s, i32, i32: (-1i32) as u32, 1 => 0);
    test_op!(test_i32_ge_u_true, i32_ge_u, i32, i32: (-1i32) as u32, 1 => 1);
    test_op!(test_i32_ge_u_false, i32_ge_u, i32, i32: 1, (-1i32) as u32 => 0);

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
    test_op!(#[cfg_attr(miri, ignore)] test_f32_sqrt, f32_sqrt, f32: 16.0f32 => 4.0f32);
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
    test_op!(#[cfg_attr(miri, ignore)] test_f64_sqrt, f64_sqrt, f64: 16.0 => 4.0);
    test_op!(test_f64_add, f64_add, f64, f64: 1.5, 2.5 => 4.0);
    test_op!(test_f64_sub, f64_sub, f64, f64: 5.5, 2.5 => 3.0);
    test_op!(test_f64_mul, f64_mul, f64, f64: 2.5, 4.0 => 10.0);
    test_op!(test_f64_div, f64_div, f64, f64: 10.0, 2.5 => 4.0);

    // ===== Conversion Operations =====
    #[test]
    fn test_i32_wrap_i64() {
        with_test_context(|state| {
            state.stack.write_u64(0, 0x123456789ABCDEF0);
            state.sp = 2;

            Interpreter.i32_wrap_i64(state).unwrap();

            assert_eq!(state.sp, 1);
            assert_eq!(state.stack.read_u32(0), 0x9ABCDEF0);
        });
    }

    #[test]
    fn test_i32_trunc_f32_s() {
        with_test_context(|state| {
            state.stack.write_f32(0, 3.99);
            state.sp = 1;

            Interpreter.i32_trunc_f32_s(state).unwrap();

            assert_eq!(state.sp, 1);
            assert_eq!(state.stack.read_u32(0) as i32, 3);
        });
    }

    #[test]
    fn test_i32_trunc_f32_u() {
        with_test_context(|state| {
            state.stack.write_f32(0, 3.99);
            state.sp = 1;

            Interpreter.i32_trunc_f32_u(state).unwrap();

            assert_eq!(state.sp, 1);
            assert_eq!(state.stack.read_u32(0), 3);
        });
    }

    #[test]
    fn test_i32_trunc_f64_s() {
        with_test_context(|state| {
            state.stack.write_f64(0, 3.99);
            state.sp = 2;

            Interpreter.i32_trunc_f64_s(state).unwrap();

            assert_eq!(state.sp, 1);
            assert_eq!(state.stack.read_u32(0) as i32, 3);
        });
    }

    #[test]
    fn test_i64_extend_i32_s() {
        with_test_context(|state| {
            state.stack.write_u32(0, (-1i32) as u32);
            state.sp = 1;

            Interpreter.i64_extend_i32_s(state).unwrap();

            assert_eq!(state.sp, 2);
            assert_eq!(state.stack.read_u64(0) as i64, -1);
        });
    }

    #[test]
    fn test_i64_extend_i32_u() {
        with_test_context(|state| {
            state.stack.write_u32(0, 0xFFFFFFFF);
            state.sp = 1;

            Interpreter.i64_extend_i32_u(state).unwrap();

            assert_eq!(state.sp, 2);
            assert_eq!(state.stack.read_u64(0), 0xFFFFFFFF);
        });
    }

    #[test]
    fn test_wrap_extend_memory_roundtrip_endianness() {
        with_test_context(|state| {
            // Push i32 0x9ABCDEF0, zero-extend to i64.
            state.stack.write_u32(0, 0x9ABC_DEF0);
            state.sp = 1;
            Interpreter.i64_extend_i32_u(state).unwrap();
            assert_eq!(state.sp, 2);
            assert_eq!(state.stack.read_u64(0), 0x0000_0000_9ABC_DEF0);

            // Store the i64 to memory@200 (addr on stack under the value), then
            // reload it; the low word must survive the stack<->memory trip.
            let stored = state.stack.read_u64(0);
            state.memory.store_u64(200, stored).unwrap();
            assert_eq!(state.memory.load_u64(200).unwrap(), 0x0000_0000_9ABC_DEF0);

            // Load a full 64-bit pattern and wrap it back to i32: only the low
            // 32 bits must remain, independent of word order.
            state.memory.store_u64(208, 0x1122_3344_5566_7788).unwrap();
            state
                .stack
                .write_u64(0, state.memory.load_u64(208).unwrap());
            state.sp = 2;
            Interpreter.i32_wrap_i64(state).unwrap();
            assert_eq!(state.sp, 1);
            assert_eq!(state.stack.read_u32(0), 0x5566_7788);
        });
    }

    #[test]
    fn test_i64_trunc_f32_s() {
        with_test_context(|state| {
            state.stack.write_f32(0, 123.99);
            state.sp = 1;

            Interpreter.i64_trunc_f32_s(state).unwrap();

            assert_eq!(state.sp, 2);
            assert_eq!(state.stack.read_u64(0) as i64, 123);
        });
    }

    #[test]
    fn test_i64_trunc_f64_s() {
        with_test_context(|state| {
            state.stack.write_f64(0, 123.99);
            state.sp = 2;

            Interpreter.i64_trunc_f64_s(state).unwrap();

            assert_eq!(state.sp, 2);
            assert_eq!(state.stack.read_u64(0) as i64, 123);
        });
    }

    #[test]
    fn test_f32_convert_i32_s() {
        with_test_context(|state| {
            state.stack.write_u32(0, (-42i32) as u32);
            state.sp = 1;

            Interpreter.f32_convert_i32_s(state).unwrap();

            assert_eq!(state.sp, 1);
            assert!((state.stack.read_f32(0) - (-42.0f32)).abs() < 0.0001);
        });
    }

    #[test]
    fn test_f32_convert_i32_u() {
        with_test_context(|state| {
            state.stack.write_u32(0, 42);
            state.sp = 1;

            Interpreter.f32_convert_i32_u(state).unwrap();

            assert_eq!(state.sp, 1);
            assert!((state.stack.read_f32(0) - 42.0f32).abs() < 0.0001);
        });
    }

    #[test]
    fn test_f32_convert_i64_s() {
        with_test_context(|state| {
            state.stack.write_u64(0, (-42i64) as u64);
            state.sp = 2;

            Interpreter.f32_convert_i64_s(state).unwrap();

            assert_eq!(state.sp, 1);
            assert!((state.stack.read_f32(0) - (-42.0f32)).abs() < 0.0001);
        });
    }

    #[test]
    fn test_f32_demote_f64() {
        with_test_context(|state| {
            state.stack.write_f64(0, 3.141592653589793);
            state.sp = 2;

            Interpreter.f32_demote_f64(state).unwrap();

            assert_eq!(state.sp, 1);
            assert!((state.stack.read_f32(0) - 3.14159265f32).abs() < 0.0001);
        });
    }

    #[test]
    fn test_f64_convert_i32_s() {
        with_test_context(|state| {
            state.stack.write_u32(0, (-42i32) as u32);
            state.sp = 1;

            Interpreter.f64_convert_i32_s(state).unwrap();

            assert_eq!(state.sp, 2);
            assert!((state.stack.read_f64(0) - (-42.0)).abs() < 0.0001);
        });
    }

    #[test]
    fn test_f64_convert_i64_s() {
        with_test_context(|state| {
            state.stack.write_u64(0, (-42i64) as u64);
            state.sp = 2;

            Interpreter.f64_convert_i64_s(state).unwrap();

            assert_eq!(state.sp, 2);
            assert!((state.stack.read_f64(0) - (-42.0)).abs() < 0.0001);
        });
    }

    #[test]
    fn test_f64_promote_f32() {
        with_test_context(|state| {
            state.stack.write_f32(0, 3.14159f32);
            state.sp = 1;

            Interpreter.f64_promote_f32(state).unwrap();

            assert_eq!(state.sp, 2);
            assert!((state.stack.read_f64(0) - 3.14159).abs() < 0.001);
        });
    }

    // ===== Parametric Operations =====
    #[test]
    fn test_drop() {
        with_test_context(|state| {
            state.stack.write_u32(0, 42);
            state.sp = 1;

            Interpreter.drop(ValType::I32, state).unwrap();

            assert_eq!(state.sp, 0);
        });
    }

    #[test]
    fn test_select_true() {
        with_test_context(|state| {
            state.stack.write_u32(0, 10); // val1
            state.stack.write_u32(1, 20); // val2
            state.stack.write_u32(2, 1); // condition (true)
            state.sp = 3;

            Interpreter.select(ValType::I32, state).unwrap();

            assert_eq!(state.sp, 1);
            assert_eq!(state.stack.read_u32(0), 10);
        });
    }

    #[test]
    fn test_select_false() {
        with_test_context(|state| {
            state.stack.write_u32(0, 10); // val1
            state.stack.write_u32(1, 20); // val2
            state.stack.write_u32(2, 0); // condition (false)
            state.sp = 3;

            Interpreter.select(ValType::I32, state).unwrap();

            assert_eq!(state.sp, 1);
            assert_eq!(state.stack.read_u32(0), 20);
        });
    }

    // ===== Memory Size/Grow Operations =====
    #[test]
    fn test_memory_size() {
        with_test_context(|state| {
            state.sp = 0;

            Interpreter.memory_size(state).unwrap();

            assert_eq!(state.sp, 1);
            assert_eq!(state.stack.read_u32(0), 1); // 65536 / 65536 = 1 page
        });
    }

    // ===== Float Comparison Operations =====
    #[test]
    fn test_f32_eq() {
        with_test_context(|state| {
            state.stack.write_f32(0, 3.14);
            state.stack.write_f32(1, 3.14);
            state.sp = 2;

            Interpreter.f32_eq(state).unwrap();

            assert_eq!(state.sp, 1);
            assert_eq!(state.stack.read_u32(0), 1);
        });
    }

    #[test]
    fn test_f32_ne() {
        with_test_context(|state| {
            state.stack.write_f32(0, 3.14);
            state.stack.write_f32(1, 2.71);
            state.sp = 2;

            Interpreter.f32_ne(state).unwrap();

            assert_eq!(state.sp, 1);
            assert_eq!(state.stack.read_u32(0), 1);
        });
    }

    #[test]
    fn test_f32_lt() {
        with_test_context(|state| {
            state.stack.write_f32(0, 2.0);
            state.stack.write_f32(1, 3.0);
            state.sp = 2;

            Interpreter.f32_lt(state).unwrap();

            assert_eq!(state.sp, 1);
            assert_eq!(state.stack.read_u32(0), 1);
        });
    }

    #[test]
    fn test_f64_eq() {
        with_test_context(|state| {
            state.stack.write_f64(0, 3.14159);
            state.stack.write_f64(2, 3.14159);
            state.sp = 4;

            Interpreter.f64_eq(state).unwrap();

            assert_eq!(state.sp, 1);
            assert_eq!(state.stack.read_u32(0), 1);
        });
    }

    // ===== i64 Comparison Operations (return i32) =====
    #[test]
    fn test_i64_eq_true() {
        with_test_context(|state| {
            state.stack.write_u64(0, 0x123456789ABCDEF0);
            state.stack.write_u64(2, 0x123456789ABCDEF0);
            state.sp = 4;

            Interpreter.i64_eq(state).unwrap();

            assert_eq!(state.sp, 1);
            assert_eq!(state.stack.read_u32(0), 1);
        });
    }

    #[test]
    fn test_i64_ne_true() {
        with_test_context(|state| {
            state.stack.write_u64(0, 0x123456789ABCDEF0);
            state.stack.write_u64(2, 0xFEDCBA9876543210);
            state.sp = 4;

            Interpreter.i64_ne(state).unwrap();

            assert_eq!(state.sp, 1);
            assert_eq!(state.stack.read_u32(0), 1);
        });
    }

    #[test]
    fn test_i64_lt_s_true() {
        with_test_context(|state| {
            state.stack.write_u64(0, (-5i64) as u64);
            state.stack.write_u64(2, 5);
            state.sp = 4;

            Interpreter.i64_lt_s(state).unwrap();

            assert_eq!(state.sp, 1);
            assert_eq!(state.stack.read_u32(0), 1);
        });
    }

    // ===== Additional Edge Cases =====
    #[test]
    fn test_i32_add_wrapping() {
        with_test_context(|state| {
            state.stack.write_u32(0, 0xFFFFFFFF);
            state.stack.write_u32(1, 2);
            state.sp = 2;

            Interpreter.i32_add(state).unwrap();

            assert_eq!(state.sp, 1);
            assert_eq!(state.stack.read_u32(0), 1); // Wraps around
        });
    }

    #[test]
    fn test_i64_add_wrapping() {
        with_test_context(|state| {
            state.stack.write_u64(0, 0xFFFFFFFFFFFFFFFF);
            state.stack.write_u64(2, 2);
            state.sp = 4;

            Interpreter.i64_add(state).unwrap();

            assert_eq!(state.sp, 2);
            assert_eq!(state.stack.read_u64(0), 1); // Wraps around
        });
    }

    // ===== Engine invoke/resume failure paths =====

    /// `invoke` on a non-idle engine must report `Busy` instead of panicking.
    #[test]
    fn test_invoke_busy_when_not_idle() {
        with_test_context(|state| {
            // Force a non-idle stack pointer to simulate a mid-execution engine.
            // The busy guard runs before any module/function lookup, so the
            // (empty) test module's function table is never touched.
            state.sp = 1;

            let err = state.invoke(
                WasmRef {
                    module: ModuleRef(0),
                    index: 0,
                },
                &[],
            );
            assert_eq!(err, Err(InvokeError::Busy));
        });
    }

    /// `resume` on an engine that is not paused must report `NotPaused`.
    #[test]
    fn test_resume_when_not_paused() {
        with_test_context(|state| {
            assert!(!state.is_paused());
            assert_eq!(state.resume(None), Err(ResumeError::NotPaused));
        });
    }

    /// `resume` with a value that does not match the paused host function's
    /// declared result type must report `ResultTypeMismatch` and preserve the
    /// pending pause so the caller can retry.
    #[test]
    fn test_resume_result_type_mismatch_preserves_pause() {
        with_test_context(|state| {
            // Simulate a host function that paused while expecting an i32 result.
            let pause = HostPause {
                result: ResultType(Some(ValType::I32)),
                resume_pc: JumpTarget(2),
            };
            state.host_pause = Some(pause);

            // Supplying no value (or a wrong-typed value) is a mismatch.
            assert_eq!(state.resume(None), Err(ResumeError::ResultTypeMismatch));
            // The pending pause is left intact for a retry.
            assert_eq!(state.host_pause, Some(pause));

            // Retrying with the correctly-typed value succeeds and consumes the
            // pause, pushing the value onto the operand stack and moving the PC
            // past the call.
            let sp_before = state.sp;
            assert_eq!(state.resume(Some(Value::I32(7))), Ok(()));
            assert!(!state.is_paused());
            assert_eq!(state.pc, JumpTarget(2));
            assert_eq!(state.sp, sp_before + 1);
            assert_eq!(state.stack.read_u32(sp_before), 7);
        });
    }

    // ===== Backtraces =====

    /// `main(x)` calls `f`, which reaches `g` through `call_indirect`. With a
    /// non-zero `x`, `g` calls the host function `env.probe`; with zero it
    /// executes `unreachable`.
    ///
    /// ```wat
    /// (module
    ///   (type $t_i (func (param i32)))
    ///   (import "env" "probe" (func $probe))
    ///   (table 1 funcref)
    ///   (elem (i32.const 0) $g)
    ///   (func $main (export "main") (param i32) local.get 0 call $f)
    ///   (func $f (param i32) (local i32 i64)
    ///     local.get 0 i32.const 0 call_indirect (type $t_i))
    ///   (func $g (param i32)
    ///     local.get 0 if call $probe else unreachable end))
    /// ```
    #[rustfmt::skip]
    static CHAIN_WASM: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x60,
        0x01, 0x7f, 0x00, 0x60, 0x00, 0x00, 0x02, 0x0d, 0x01, 0x03, 0x65, 0x6e,
        0x76, 0x05, 0x70, 0x72, 0x6f, 0x62, 0x65, 0x00, 0x01, 0x03, 0x04, 0x03,
        0x00, 0x00, 0x00, 0x04, 0x04, 0x01, 0x70, 0x00, 0x01, 0x07, 0x08, 0x01,
        0x04, 0x6d, 0x61, 0x69, 0x6e, 0x00, 0x01, 0x09, 0x07, 0x01, 0x00, 0x41,
        0x00, 0x0b, 0x01, 0x03, 0x0a, 0x22, 0x03, 0x06, 0x00, 0x20, 0x00, 0x10,
        0x02, 0x0b, 0x0d, 0x02, 0x01, 0x7f, 0x01, 0x7e, 0x20, 0x00, 0x41, 0x00,
        0x11, 0x00, 0x00, 0x0b, 0x0b, 0x00, 0x20, 0x00, 0x04, 0x40, 0x10, 0x00,
        0x05, 0x00, 0x0b, 0x0b,
    ];

    /// Hands a whole byte slice to the module reader as a single chunk.
    struct SliceStream(&'static [u8], bool);
    impl crate::WasmStream for SliceStream {
        fn read(&mut self) -> Result<Option<crate::InnerVec<u8>>, u8> {
            if self.1 {
                return Ok(None);
            }
            self.1 = true;
            Ok(Some(crate::InnerVec {
                ptr: self.0.as_ptr() as *mut u8,
                capacity: self.0.len(),
                len: self.0.len(),
            }))
        }
        fn return_(&mut self, _chunk: crate::InnerVec<u8>) {}
    }

    /// How many times `env.probe` has been called.
    type Calls = std::rc::Rc<core::cell::Cell<u32>>;

    /// An engine with `CHAIN_WASM` loaded, whose `env.probe` pauses. Counts the
    /// calls to `env.probe`.
    fn chain_engine() -> (Engine, CodeBuilder, Calls) {
        chain_engine_with(ControlFlow::Break(HostFunctionBreak::Pause))
    }

    /// [`chain_engine`], with `env.probe` returning `probe_returns`.
    fn chain_engine_with(probe_returns: HostFunctionResult) -> (Engine, CodeBuilder, Calls) {
        let calls: Calls = Default::default();
        let count = calls.clone();
        let probe = HostFunction::new("probe", "".into(), "".into(), move |_, _| {
            count.set(count.get() + 1);
            probe_returns
        });
        let host = HostModule {
            name: "env".into(),
            globals: crate::vec![],
            functions: crate::vec![probe],
            memory: crate::vec![],
            table: crate::vec![],
        };

        let mut engine = Engine::new(1024, 1, crate::vec![host]).unwrap();
        let mut code = CodeBuilder::new(CompilerOptions {
            allow_memory_grow: false,
            max_backpatch_iterations: None,
            max_code_pages: 16,
            #[cfg(feature = "ir-map")]
            pc_ir_callback: None,
        })
        .unwrap();
        let mut control_frames = [ControlFrame::EMPTY; 16];
        let mut operands = [OperandType::EMPTY; 64];
        let module = Module::new(
            "chain",
            &mut SliceStream(CHAIN_WASM, false),
            &mut engine.store,
            &mut code,
            &mut DecoderStack::new(&mut control_frames, &mut operands),
            crate::Rc::new(TestAllocator)
                .unwrap()
                .into_wasm_memory_allocator(),
        )
        .unwrap();
        engine.push_module(module).unwrap();
        (engine, code, calls)
    }

    /// The IR opcode of the instruction starting at `pc`.
    fn opcode_at(code: &CodeBuilder, pc: u32) -> u8 {
        let pc = JumpTarget(pc);
        (code.pages()[pc.page()].0[pc.offset()] >> 8) as u8
    }

    /// Index of the function in the chain module whose IR contains `pc`.
    /// Functions are compiled in order, so each one ends where the next begins.
    fn function_at(engine: &Engine, pc: u32) -> usize {
        let functions = &engine.store.modules()[0].functions;
        functions
            .iter()
            .rposition(|f| f.expr.0.0 <= pc)
            .expect("pc inside the module")
    }

    const MAIN: u16 = 0;

    fn run_all(engine: &mut Engine, code: &CodeBuilder) -> InterpreterResult {
        Interpreter.run(code.pages(), engine, 10_000)
    }

    fn backtrace_of(engine: &Engine) -> std::vec::Vec<u32> {
        engine.backtrace().unwrap().map(|pc| pc.0).collect()
    }

    /// Paused in a host call, element 0 is the `call` to the host function and
    /// each later element is the calling instruction one frame further out. Only
    /// `resume` moves past the call.
    #[test]
    fn test_backtrace_paused_in_host_call() {
        let (mut engine, code, calls) = chain_engine();
        let main = WasmRef {
            module: ModuleRef(0),
            index: MAIN,
        };
        engine.invoke(main, &[Value::I32(1)]).unwrap();
        assert_eq!(run_all(&mut engine, &code), InterpreterResult::Pause);
        assert!(engine.is_paused());

        let bt = backtrace_of(&engine);
        assert_eq!(bt.len(), 3, "g <- f <- main: {bt:x?}");
        assert_eq!(bt[0], engine.pc.0);

        // Each entry is the call instruction itself, in g, f and main.
        assert_eq!(opcode_at(&code, bt[0]), crate::opcode::CALL_HOST);
        assert_eq!(opcode_at(&code, bt[1]), crate::opcode::CALL_INDIRECT);
        assert_eq!(opcode_at(&code, bt[2]), crate::opcode::CALL);
        let funcs: std::vec::Vec<usize> = bt.iter().map(|&pc| function_at(&engine, pc)).collect();
        assert_eq!(funcs, [2, 1, 0]);

        engine.resume(None).unwrap();
        assert!(!engine.is_paused());
        assert_ne!(engine.pc.0, bt[0], "resume moved past the call");
        assert_eq!(run_all(&mut engine, &code), InterpreterResult::Finished);
        assert_eq!(calls.get(), 1, "the host function ran once");

        // Finished: the engine is idle and has no backtrace.
        assert!(engine.backtrace().is_none());
    }

    /// A trap keeps its backtrace after the engine resets to idle: element 0 is
    /// the faulting instruction, and the callers match a paused backtrace from
    /// the same call sites. A new invoke or a reset replaces it.
    #[test]
    fn test_trap_backtrace_survives_trap_reset() {
        let (mut engine, code, _) = chain_engine();
        let main = WasmRef {
            module: ModuleRef(0),
            index: MAIN,
        };
        engine.invoke(main, &[Value::I32(1)]).unwrap();
        assert_eq!(run_all(&mut engine, &code), InterpreterResult::Pause);
        let paused = backtrace_of(&engine);
        engine.reset();

        engine.invoke(main, &[Value::I32(0)]).unwrap();
        assert_eq!(
            run_all(&mut engine, &code),
            InterpreterResult::Trap(crate::TrapReason::Unreachable)
        );
        assert!(engine.is_idle());

        let bt = backtrace_of(&engine);
        assert_eq!(bt.len(), 3, "{bt:x?}");
        assert_eq!(JumpTarget(bt[0]), engine.trap_pc.unwrap());
        assert_eq!(opcode_at(&code, bt[0]), crate::opcode::UNREACHABLE);
        assert_eq!(function_at(&engine, bt[0]), 2);
        assert_eq!(bt[1..], paused[1..], "same call sites in f and main");

        // A new call replaces the trap's backtrace with its own: just `main`.
        engine.invoke(main, &[Value::I32(1)]).unwrap();
        assert_eq!(backtrace_of(&engine), [engine.pc.0]);
        engine.reset();
        assert!(engine.backtrace().is_none());
    }

    /// Between single-instruction run slices the backtrace always starts at the
    /// current PC, every caller entry is a call instruction, and the depth goes
    /// 1 -> 2 -> 3 and back as the call chain is entered and left.
    #[test]
    fn test_backtrace_between_run_slices() {
        let (mut engine, code, _) = chain_engine_with(ControlFlow::Continue(None));
        engine
            .invoke(
                WasmRef {
                    module: ModuleRef(0),
                    index: MAIN,
                },
                &[Value::I32(1)],
            )
            .unwrap();

        let mut depths = std::vec::Vec::new();
        loop {
            let bt = backtrace_of(&engine);
            assert_eq!(bt[0], engine.pc.0);
            for &pc in &bt[1..] {
                let op = opcode_at(&code, pc);
                assert!(
                    op == crate::opcode::CALL || op == crate::opcode::CALL_INDIRECT,
                    "caller entry {pc:#x} is opcode {op:#x}"
                );
            }
            assert_eq!(function_at(&engine, *bt.last().unwrap()), 0, "ends in main");
            if depths.last() != Some(&bt.len()) {
                depths.push(bt.len());
            }

            match Interpreter.run(code.pages(), &mut engine, 1) {
                InterpreterResult::OutOfFuel => {}
                InterpreterResult::Finished => break,
                other => panic!("unexpected {other:?}"),
            }
        }
        assert_eq!(depths, [1, 2, 3, 2, 1]);
    }

    /// GHSA-hm35-gr3g-4jxx: running a paused engine without resuming must not
    /// execute the guest without the host call's result.
    #[test]
    fn test_run_while_paused_runs_nothing() {
        let (mut engine, code, calls) = chain_engine();
        let main = WasmRef {
            module: ModuleRef(0),
            index: MAIN,
        };
        engine.invoke(main, &[Value::I32(1)]).unwrap();
        assert_eq!(run_all(&mut engine, &code), InterpreterResult::Pause);

        let (pc, sp, fp) = (engine.pc, engine.sp, engine.fp);
        for _ in 0..3 {
            assert_eq!(run_all(&mut engine, &code), InterpreterResult::Pause);
            assert_eq!((engine.pc, engine.sp, engine.fp), (pc, sp, fp));
        }
        assert_eq!(calls.get(), 1, "the host call is not repeated");

        engine.resume(None).unwrap();
        assert_eq!(run_all(&mut engine, &code), InterpreterResult::Finished);
    }

    /// Point `fp` at a hand-written frame and collect the backtrace.
    fn walk(state: &mut Engine, fp: u32) -> std::vec::Vec<u32> {
        state.pc = JumpTarget(0x10);
        state.fp = fp;
        let bt = state.backtrace().unwrap().map(|pc| pc.0).collect();
        state.pc = JumpTarget::SENTINEL;
        state.fp = 0;
        bt
    }

    fn frame(frame_length: u16) -> u32 {
        crate::CallFrame {
            frame_length,
            module_delta: 0,
            parameter_size: 0,
        }
        .into_bits()
    }

    /// A corrupt or inconsistent stack ends the walk early rather than reading
    /// out of bounds, panicking, or looping.
    #[test]
    fn test_backtrace_stops_on_corrupt_frames() {
        with_test_context(|state| {
            let len = state.stack.len();

            // Frame pointer past the end of the stack, or with no room for the
            // saved PC: only the current PC.
            assert_eq!(walk(state, u32::MAX), [0x10]);
            assert_eq!(walk(state, len as u32), [0x10]);
            assert_eq!(walk(state, (len - 1) as u32), [0x10]);

            // A zero-length frame would point at itself: the caller PC is
            // reported, then the walk stops instead of looping.
            state.stack.write_u32(100, frame(0));
            state.stack.write_u32(101, 0x20);
            assert_eq!(walk(state, 100), [0x10, 0x20]);

            // A frame length reaching below the bottom of the stack.
            state.stack.write_u32(100, frame(101));
            assert_eq!(walk(state, 100), [0x10, 0x20]);

            // A well-formed chain of two frames ending at SENTINEL.
            state.stack.write_u32(100, frame(40));
            state.stack.write_u32(60, frame(60));
            state.stack.write_u32(61, 0x30);
            state.stack.write_u32(0, frame(0));
            state.stack.write_u32(1, JumpTarget::SENTINEL.0);
            assert_eq!(walk(state, 100), [0x10, 0x20, 0x30]);

            // Arbitrary garbage terminates within one step per stack word.
            for i in 0..len {
                state
                    .stack
                    .write_u32(i, (i as u32).wrapping_mul(0x9E37_79B9) | 1);
            }
            for fp in [0, 1, 7, 500, len as u32 - 2] {
                assert!(walk(state, fp).len() <= len + 1);
            }
        });
    }
}
