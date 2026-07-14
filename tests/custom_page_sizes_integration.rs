mod util;
use util::run_wast_test_file;

#[test]
fn custom_page_sizes_invalid() {
    run_wast_test_file("custom-page-sizes/custom-page-sizes-invalid");
}

#[test]
fn custom_page_sizes() {
    run_wast_test_file("custom-page-sizes/custom-page-sizes");
}

#[test]
fn memory_max() {
    run_wast_test_file("custom-page-sizes/memory_max");
}
