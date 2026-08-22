//! RISC-V functionality for the main coremark benchmark program.
//!
//! Copyright 2026 California Institute of Technology
//!
//! Licensed under the Apache License, Version 2.0 (the "License");
//! you may not use this file except in compliance with the License.
//! You may obtain a copy of the License at
//!
//! <http://www.apache.org/licenses/LICENSE-2.0>
//!
//! ---
//! Portions of this file are derived from <https://github.com/rust-embedded/riscv>
//! and the riscv and riscv-rt crates developed by the Rust Embedded community.
//!
//! Portions of this file are derived from <https://github.com/taiki-e/semihosting>
//! and the semihosting crate developed by Taiki Endo
pub use riscv_rt::entry;

const CLOCK_HZ: u32 = 1_000_000_000; // 1 GHz

pub fn clock_setup() {
    // nothing to do here
}

pub fn clock_get_ms() -> i64 {
    let ticks = riscv::register::time::read() as f64;
    (ticks / ((CLOCK_HZ / 1000) as f64)) as i64
}

pub fn exit(code: i32) -> ! {
    semihosting::process::exit(code);
}
