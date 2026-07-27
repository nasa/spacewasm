// Each integration-test binary compiles this module independently and uses
// only a subset of its helpers, so unused items are expected per-crate.
#[allow(dead_code)]
mod spectest;
pub use spectest::*;

mod inspector;
