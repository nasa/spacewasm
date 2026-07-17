mod util;
use util::run_wast_test_file;

#[test]
fn host_funcs() {
    run_wast_test_file("regression/host_funcs");
}

#[test]
fn host_globals() {
    run_wast_test_file("regression/host_globals");
}

#[test]
fn extern_globals() {
    run_wast_test_file("regression/extern_globals");
}

#[test]
fn extern_funcs() {
    run_wast_test_file("regression/extern_funcs");
}

#[test]
fn extern_globals_chained() {
    run_wast_test_file("regression/extern_globals_chained");
}

#[test]
fn extern_tables() {
    run_wast_test_file("regression/extern_tables");
}

#[test]
fn extern_memory() {
    run_wast_test_file("regression/extern_memory");
}

#[test]
fn start_stack_overflow() {
    run_wast_test_file("regression/start_stack_overflow");
}
