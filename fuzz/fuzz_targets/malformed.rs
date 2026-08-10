#![no_main]

use libfuzzer_sys::fuzz_target;
use spacewasm_fuzzing::generators::MalformedModule;
use spacewasm_fuzzing::oracles;

fuzz_target!(|module: MalformedModule| {
    oracles::decode(module.wasm());
});
