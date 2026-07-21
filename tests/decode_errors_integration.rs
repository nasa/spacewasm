mod util;
use util::run_wast_test_file;

#[test]
fn decode_errors() {
    run_wast_test_file("decode-errors/decode-errors");
}
