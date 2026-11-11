//! `spacewasm lower`: rewrite a module built for a richer feature set into one this
//! release loads, as `wasm2mvp.sh` did.
//!
//! For a module the `spacewasm` linker did not build, e.g. a WASI SDK program. Only the
//! proposals this release lacks are lowered; one it implements is left as it is.

use crate::Outcome;
use crate::args::LowerArgs;
use spacewasm_tools::{find_unsupported, lowering_flags};
use std::process::Command;

pub fn run(args: &LowerArgs) -> Outcome {
    let output = args.output.as_ref().unwrap_or(&args.input);
    let status = Command::new(&args.wasm_opt)
        // Accept whatever the input uses: `wasm-opt` detects its features, and the
        // lowering passes take out what this release lacks.
        .args(lowering_flags())
        // `wasm-opt` would rewrite it to list every feature it was told to read, which
        // claims proposals the module does not use; nothing on board reads it.
        .arg("--strip-target-features")
        .arg(&args.input)
        .arg("-o")
        .arg(output)
        .status()
        .map_err(|err| format!("could not run `{}`: {err}", args.wasm_opt.display()))?;
    if !status.success() {
        return Err(format!("`{}` failed with {status}", args.wasm_opt.display()).into());
    }

    // A proposal with no lowering pass survives: say so rather than leave it to the load.
    let lowered = std::fs::read(output).map_err(|err| format!("{}: {err}", output.display()))?;
    match find_unsupported(&lowered) {
        Some(unsupported) => {
            eprintln!("{}: still unloadable: {unsupported}", output.display());
            Ok(false)
        }
        None => Ok(true),
    }
}
