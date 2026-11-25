//! `spacewasm trace`: run every exported function and print the last instructions each
//! executed, with `pc`/`sp`/`fp`.
//!
//! What `spacewasm-trace` was: a debugging aid for the interpreter itself, usually fed a
//! fuzzer's crashing input. Host imports are stood in for, and trap if called.

mod tracer;

use crate::Outcome;
use crate::args::TraceArgs;
use crate::load::load;
use spacewasm::{Interpreter, InterpreterResult};
use spacewasm_tools::host_modules_for;
use std::io::Read;
use tracer::StateTracer;

/// Operand stack for a traced run, in 32-bit words: a fuzzer's module is small.
const STACK_WORDS: usize = 512;

pub fn run(args: &TraceArgs) -> Outcome {
    let bytes = match &args.file {
        Some(path) => std::fs::read(path)
            .map_err(|err| format!("could not read {}: {err}", path.display()))?,
        None => {
            let mut bytes = Vec::new();
            std::io::stdin()
                .read_to_end(&mut bytes)
                .map_err(|err| format!("could not read the module from stdin: {err}"))?;
            bytes
        }
    };
    eprintln!("Loaded {} bytes", bytes.len());

    let mut loaded = load(
        &bytes,
        host_modules_for(&bytes)?,
        args.decoder_limits,
        STACK_WORDS,
    )?;

    // A panic here is usually the interpreter's own assertion (`strict-assertions`)
    // tripping on the start function, which is worth naming before it unwinds.
    let started =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| loaded.start(args.fuel)));
    match started {
        Ok(InterpreterResult::Finished) => {}
        Ok(other) => return Err(format!("the start function did not complete: {other:?}").into()),
        Err(panic) => {
            eprintln!("\nPanicked in the start function: an overflow, unbounded recursion, or");
            eprintln!("an interpreter assertion. `wasm-tools print` shows what it does.");
            std::panic::resume_unwind(panic);
        }
    }

    let exports = loaded.exports();
    if exports.is_empty() {
        return Err("the module exports no functions to trace".into());
    }
    eprintln!("Found {} exported function(s)", exports.len());

    let mut all = true;
    for (name, function, arguments) in exports {
        loaded.engine.reset();
        loaded
            .engine
            .invoke(function, &arguments)
            .map_err(|err| format!("could not invoke `{name}`: {err:?}"))?;

        let tracer = StateTracer::new(&Interpreter, args.limit);
        let pages = loaded.code.pages();
        let engine = &mut loaded.engine;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            spacewasm::InterpreterRunner::run(&tracer, pages, engine, args.fuel)
        }));

        // Always the trace: it is what explains a panic.
        println!("`{name}`:{}", tracer.dump_history());

        match result {
            Ok(InterpreterResult::Finished) => eprintln!("\n`{name}`: completed"),
            Ok(InterpreterResult::OutOfFuel) => {
                eprintln!("\n`{name}`: out of fuel after {} instructions", args.fuel)
            }
            Ok(InterpreterResult::Pause) => eprintln!("\n`{name}`: paused"),
            Ok(InterpreterResult::Trap(reason)) => {
                eprintln!("\n`{name}`: trapped, {reason:?}");
                all = false;
            }
            Err(panic) => {
                eprintln!("\n`{name}`: panicked");
                std::panic::resume_unwind(panic);
            }
        }
    }
    Ok(all)
}
