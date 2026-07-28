mod util;
use std::{ops::ControlFlow, sync::Mutex};

use spacewasm::vec;
use spacewasm::*;
use util::{run_wast_test_file, spectest_host_module};

use crate::util::MutableStaticGlobal;

/// Extra host module used only by the regression integration tests.
pub fn regression_host_module() -> HostModule {
    HostModule {
        name: "regression".into(),
        globals: vec![
            HostGlobal {
                name: "mut_global_i32".into(),
                value: spacewasm::Box::new(MutableStaticGlobal {
                    value: Mutex::new(Value::I32(0)),
                    ty: ValType::I32,
                })
                .unwrap()
                .into_global_value_dyn(),
            },
            HostGlobal {
                name: "mut_global_i64".into(),
                value: spacewasm::Box::new(MutableStaticGlobal {
                    value: Mutex::new(Value::I64(0)),
                    ty: ValType::I64,
                })
                .unwrap()
                .into_global_value_dyn(),
            },
            HostGlobal {
                name: "mut_global_f32".into(),
                value: spacewasm::Box::new(MutableStaticGlobal {
                    value: Mutex::new(Value::F32(0.0)),
                    ty: ValType::F32,
                })
                .unwrap()
                .into_global_value_dyn(),
            },
            HostGlobal {
                name: "mut_global_f64".into(),
                value: spacewasm::Box::new(MutableStaticGlobal {
                    value: Mutex::new(Value::F64(0.0)),
                    ty: ValType::F64,
                })
                .unwrap()
                .into_global_value_dyn(),
            },
        ],
        functions: vec![
            HostFunction::new(
                "return_i32_from_all_args",
                "iIfd".into(),
                "i".into(),
                |_, args| {
                    let Value::I32(v) = args[0] else {
                        unreachable!()
                    };
                    ControlFlow::Continue(Some(Value::I32(v)))
                },
            ),
            HostFunction::new("return_i64", "".into(), "I".into(), |_, _| {
                ControlFlow::Continue(Some(Value::I64(0x123456789)))
            }),
            HostFunction::new("return_f32", "".into(), "f".into(), |_, _| {
                ControlFlow::Continue(Some(Value::F32(12.5)))
            }),
            HostFunction::new("return_f64", "".into(), "d".into(), |_, _| {
                ControlFlow::Continue(Some(Value::F64(42.25)))
            }),
            HostFunction::new("noop", "".into(), "".into(), |_, _| {
                ControlFlow::Continue(None)
            }),
            HostFunction::new("pause", "".into(), "".into(), |_, _| {
                ControlFlow::Break(HostFunctionBreak::Pause)
            }),
            HostFunction::new("pause_i32", "".into(), "i".into(), |_, _| {
                ControlFlow::Break(HostFunctionBreak::Pause)
            }),
            HostFunction::new("pause_i64", "".into(), "I".into(), |_, _| {
                ControlFlow::Break(HostFunctionBreak::Pause)
            }),
            HostFunction::new("pause_f32", "".into(), "f".into(), |_, _| {
                ControlFlow::Break(HostFunctionBreak::Pause)
            }),
            HostFunction::new("pause_f64", "".into(), "d".into(), |_, _| {
                ControlFlow::Break(HostFunctionBreak::Pause)
            }),
            HostFunction::new(
                "invalid_should_return_some",
                "".into(),
                "i".into(),
                |_, _| {
                    // The return value string says it returns an i32 but we are returning void
                    ControlFlow::Continue(None)
                },
            ),
            HostFunction::new(
                "invalid_should_return_none",
                "".into(),
                "".into(),
                |_, _| {
                    // The return value string says it returns void but we are returning Some(I32(1))
                    ControlFlow::Continue(Some(Value::I32(1)))
                },
            ),
        ],
        memory: vec![],
        table: vec![],
    }
}

fn run(test_name: &str) {
    run_wast_test_file(test_name, || {
        vec![spectest_host_module(), regression_host_module()]
    });
}

#[test]
fn host_funcs() {
    run("regression/host_funcs");
}

#[test]
fn host_globals() {
    run("regression/host_globals");
}

#[test]
fn extern_globals() {
    run("regression/extern_globals");
}

#[test]
fn extern_funcs() {
    run("regression/extern_funcs");
}

#[test]
fn extern_globals_chained() {
    run("regression/extern_globals_chained");
}

#[test]
fn extern_tables() {
    run("regression/extern_tables");
}

#[test]
fn extern_memory() {
    run("regression/extern_memory");
}

#[test]
fn start_stack_overflow() {
    run("regression/start_stack_overflow");
}

#[test]
fn decode_errors() {
    run("regression/decode-errors");
}

#[test]
fn pause_resume() {
    run("regression/pause-resume");
}

#[test]
fn host_func_invalid_traps_none() {
    run("regression/host_func_invalid_traps_none");
}

#[test]
fn host_func_invalid_traps_some() {
    run("regression/host_func_invalid_traps_some");
}
