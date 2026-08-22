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

pub fn clock_setup() {
    // nothing to do here
}

pub fn clock_get_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

pub fn exit(code: i32) -> ! {
    std::process::exit(code);
}
