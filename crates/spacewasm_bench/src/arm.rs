/// ARM functionality for the main coremark benchmark program.
///
/// Copyright 2026 California Institute of Technology
///
/// Licensed under the Apache License, Version 2.0 (the "License");
/// you may not use this file except in compliance with the License.
/// You may obtain a copy of the License at
///
/// <http://www.apache.org/licenses/LICENSE-2.0>
///
/// ---
/// Portions of this file are derived from <https://github.com/rust-embedded/cortex-m>
/// and the cortex-m and cortex-m-rt crates developed by the Rust Embedded community.
/// 
/// Portions of this file are derived from <https://github.com/taiki-e/semihosting>
/// and the semihosting crate developed by Taiki Endo
/// 
/// Portions of this file are derived from <https://github.com/rtic-rs/rtic-monotonic>
/// and the rtic-monotonics crate developed by the RTIC Rust community.

use rtic_monotonics::systick::prelude::*;

pub use cortex_m_rt::entry;

mod alloc;

const CLOCK_HZ: u32 = 1_000_000_000; // 1 GHz

systick_monotonic!(Mono, 1_000);

pub fn clock_setup() {
    let p = cortex_m::Peripherals::take().unwrap();
    Mono::start(p.SYST, CLOCK_HZ);
}

pub fn clock_get_ms() -> i64 {
    Mono::now().ticks() as i64
}

pub fn exit(code: i32) -> ! {
    semihosting::process::exit(code);
}

pub fn init_allocator() {
    alloc::init();
}
