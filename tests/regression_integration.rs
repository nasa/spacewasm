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
