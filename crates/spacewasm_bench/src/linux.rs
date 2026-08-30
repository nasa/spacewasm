//! Linux/Unix functionality for the main coremark benchmark program.
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

use std::sync::LazyLock;

use cputicks::Ticks;

static TICK_START: LazyLock<Ticks> = LazyLock::new(Ticks::now);

pub fn clock_setup() {
    // set up in LazyLock initializer above
}

pub fn clock_get_ms() -> i64 {
    TICK_START.elapsed().as_duration().as_millis() as i64
}

pub fn exit(code: i32) -> ! {
    std::process::exit(code);
}
