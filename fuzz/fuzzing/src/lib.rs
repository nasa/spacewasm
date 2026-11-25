// Portions of this file are derived from the Wasmtime project
// (https://github.com/bytecodealliance/wasmtime), licensed under
// Apache-2.0 WITH LLVM-exception. These portions have been modified for
// SpaceWasm.

//! Fuzzing infrastructure for SpaceWasm.
//!
//! This crate provides test case generators and oracles for fuzzing SpaceWasm.
//! It is independent from the fuzzing engine (libfuzzer, AFL, etc.) and can be
//! reused across different fuzzing frameworks.

#![deny(missing_docs)]

pub mod generators;
pub mod oracles;

use std::sync::Once;

/// One-time initialization for fuzzing.
///
/// This should be called at the start of each fuzz target to ensure proper
/// logging and configuration.
pub fn init_fuzzing() {
    static INIT: Once = Once::new();

    INIT.call_once(|| {
        let _ = env_logger::try_init();
    });
}
