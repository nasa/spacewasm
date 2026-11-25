#![no_main]

use libfuzzer_sys::fuzz_target;
use spacewasm_fuzzing::generators::ExecModule;
use spacewasm_fuzzing::oracles;

fuzz_target!(|module: ExecModule| {
    oracles::exec_differential(module.wasm(), &module.args);
});
