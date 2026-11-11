//! `spacewasm features`: what this release implements, for the tools that build for it.

use crate::Outcome;
use crate::args::FeaturesArgs;
use spacewasm_tools::{PROPOSALS, rustc_target_features, wasm_opt_flags};

pub fn run(args: &FeaturesArgs) -> Outcome {
    if args.wasm_opt {
        println!("{}", wasm_opt_flags().join(" "));
        return Ok(true);
    }
    if args.rustc {
        println!("{}", rustc_target_features());
        return Ok(true);
    }

    println!(
        "spacewasm {}: WebAssembly 1.0, plus",
        env!("CARGO_PKG_VERSION")
    );
    for proposal in PROPOSALS.iter().filter(|proposal| proposal.supported) {
        println!("  {}", proposal.name);
    }
    println!();
    println!("wasm-opt:  {}", wasm_opt_flags().join(" "));
    println!(
        "rustc:     -Ctarget-cpu=mvp -Ctarget-feature={}",
        rustc_target_features()
    );
    Ok(true)
}
