//! `spacewasm split`.

use crate::Outcome;
use crate::args::SplitArgs;
use spacewasm_cli::{Optimizer, split_file};

pub fn run(args: &SplitArgs) -> Outcome {
    let output = args.output.clone().unwrap_or_else(|| args.input.clone());
    let symbols = args
        .symbols
        .clone()
        .unwrap_or_else(|| output.with_extension("sym"));
    let optimizer = args
        .optimize
        .as_ref()
        .map(|extra| Optimizer::held(args.wasm_opt.clone(), extra));
    split_file(&args.input, &output, &symbols, optimizer.as_ref())?;
    Ok(true)
}
