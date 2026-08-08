#![cfg(not(miri))]

mod util;

#[test]
#[ignore]
fn convert() {
    util::convert_wast_for_miri();
}
