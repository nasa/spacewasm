#![no_main]

use libfuzzer_sys::fuzz_target;
use spacewasm_fuzzing::generators::FuzzModule;
use spacewasm_fuzzing::oracles;

fuzz_target!(|module: FuzzModule| {
    oracles::validate(module.wasm());
});
