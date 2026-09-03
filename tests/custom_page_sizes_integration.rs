mod util;
use spacewasm::vec;
use util::{run_wast_test_file, spectest_host_module};

fn run(test_name: &str) {
    run_wast_test_file(test_name, || vec![spectest_host_module()]);
}

#[test]
fn custom_page_sizes_invalid() {
    run("custom-page-sizes/custom-page-sizes-invalid");
}

#[test]
fn custom_page_sizes() {
    run("custom-page-sizes/custom-page-sizes");
}

#[test]
#[cfg(target_pointer_width = "64")]
fn memory_max() {
    run("custom-page-sizes/memory_max");
}

#[test]
#[cfg(target_pointer_width = "32")]
fn memory_max() {
    run("custom-page-sizes/memory_max_32");
}
