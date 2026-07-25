mod util;
use spacewasm::vec;
use util::{regression_host_module, run_wast_test_file, spectest_host_module};

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
