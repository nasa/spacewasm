mod util;
use spacewasm::vec;
use util::{run_wast_test_file, spectest_host_module};

fn run(test_name: &str) {
    run_wast_test_file(test_name, || vec![spectest_host_module()]);
}

#[test]
fn binary_leb128() {
    run("nontrapping-float-to-int/binary-leb128");
}

#[test]
#[cfg_attr(miri, ignore = "soft-float NaN issues")]
fn conversions() {
    run("nontrapping-float-to-int/conversions");
}
