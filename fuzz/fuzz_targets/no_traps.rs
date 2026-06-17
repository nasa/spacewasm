#![no_main]

use libfuzzer_sys::fuzz_target;
use spacewasm_fuzzing::generators::NoTrapsModule;
use spacewasm_fuzzing::oracles;

fuzz_target!(|module: NoTrapsModule| {
    oracles::no_traps(module.wasm());
});
