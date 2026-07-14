mod util;
use util::run_wast_test_file;

#[test]
fn address0() {
    run_wast_test_file("multi-memory/address0");
}

#[test]
fn address1() {
    run_wast_test_file("multi-memory/address1");
}

#[test]
fn align0() {
    run_wast_test_file("multi-memory/align0");
}

#[test]
fn binary0() {
    run_wast_test_file("multi-memory/binary0");
}

#[test]
fn data0() {
    run_wast_test_file("multi-memory/data0");
}

#[test]
fn data1() {
    run_wast_test_file("multi-memory/data1");
}

#[test]
fn data_drop0() {
    run_wast_test_file("multi-memory/data_drop0");
}

#[test]
fn exports0() {
    run_wast_test_file("multi-memory/exports0");
}

#[test]
fn float_exprs0() {
    run_wast_test_file("multi-memory/float_exprs0");
}

#[test]
fn float_exprs1() {
    run_wast_test_file("multi-memory/float_exprs1");
}

#[test]
fn float_memory0() {
    run_wast_test_file("multi-memory/float_memory0");
}

#[test]
fn imports0() {
    run_wast_test_file("multi-memory/imports0");
}

#[test]
fn imports1() {
    run_wast_test_file("multi-memory/imports1");
}

#[test]
fn imports2() {
    run_wast_test_file("multi-memory/imports2");
}

#[test]
fn imports3() {
    run_wast_test_file("multi-memory/imports3");
}

#[test]
fn imports4() {
    run_wast_test_file("multi-memory/imports4");
}

#[test]
fn linking0() {
    run_wast_test_file("multi-memory/linking0");
}

#[test]
fn linking1() {
    run_wast_test_file("multi-memory/linking1");
}

#[test]
fn linking2() {
    run_wast_test_file("multi-memory/linking2");
}

#[test]
fn linking3() {
    run_wast_test_file("multi-memory/linking3");
}

#[test]
fn load0() {
    run_wast_test_file("multi-memory/load0");
}

#[test]
fn load1() {
    run_wast_test_file("multi-memory/load1");
}

#[test]
fn load2() {
    run_wast_test_file("multi-memory/load2");
}

#[test]
fn memory_copy0() {
    run_wast_test_file("multi-memory/memory_copy0");
}

#[test]
fn memory_copy1() {
    run_wast_test_file("multi-memory/memory_copy1");
}

#[test]
fn memory_fill0() {
    run_wast_test_file("multi-memory/memory_fill0");
}

#[test]
fn memory_init0() {
    run_wast_test_file("multi-memory/memory_init0");
}

#[test]
fn memory_size0() {
    run_wast_test_file("multi-memory/memory_size0");
}

#[test]
fn memory_size1() {
    run_wast_test_file("multi-memory/memory_size1");
}

#[test]
fn memory_size2() {
    run_wast_test_file("multi-memory/memory_size2");
}

#[test]
fn memory_size3() {
    run_wast_test_file("multi-memory/memory_size3");
}

#[test]
fn memory_trap0() {
    run_wast_test_file("multi-memory/memory_trap0");
}

#[test]
fn memory_trap1() {
    run_wast_test_file("multi-memory/memory_trap1");
}

#[test]
fn start0() {
    run_wast_test_file("multi-memory/start0");
}

#[test]
fn store0() {
    run_wast_test_file("multi-memory/store0");
}

#[test]
fn store1() {
    run_wast_test_file("multi-memory/store1");
}

#[test]
fn traps0() {
    run_wast_test_file("multi-memory/traps0");
}

#[test]
fn host_funcs() {
    run_wast_test_file("regression/host_funcs");
}
