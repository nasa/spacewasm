//! Convert fuzzer seed artifacts to Wasm binaries.
//!
//! Fuzzer artifacts are arbitrary byte streams that get passed through
//! wasm-smith to generate valid Wasm. This tool converts seeds to Wasm
//! for analysis with other tools (disassemblers, trace utilities, etc.).
//!
//! The `no_traps` and `validate` fuzz targets configure wasm-smith
//! differently, so a seed must be decoded with the same generator that
//! produced it. Select it with `--target` (default: `no_traps`).
//!
//! Usage:
//!   seed_to_wasm [--target no_traps|validate] <seed-file> [output.wasm]
//!   seed_to_wasm [--target no_traps|validate] <seed-file> --stdout

use spacewasm_fuzzing::generators::{wasm_from_seed, Target};
use std::env;
use std::fs;
use std::io::{self, Write};
use std::process;

fn usage(program: &str) -> ! {
    eprintln!("Usage: {program} [--target no_traps|validate] <seed-file> [output.wasm]");
    eprintln!("       {program} [--target no_traps|validate] <seed-file> --stdout");
    eprintln!();
    eprintln!("Convert fuzzer seed artifacts to Wasm binaries.");
    eprintln!();
    eprintln!("Options:");
    eprintln!(
        "  --target <no_traps|validate>  Generator to decode the seed with (default: no_traps)"
    );
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  {program} fuzz/artifacts/no_traps/crash-xxx output.wasm");
    eprintln!(
        "  {program} --target validate fuzz/artifacts/validate/crash-xxx --stdout | wasm2wat"
    );
    process::exit(1);
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let program = args
        .first()
        .cloned()
        .unwrap_or_else(|| "seed_to_wasm".to_string());

    // Pull the optional `--target` flag out of the argument list; everything
    // else stays positional so existing invocations keep working.
    let mut target = Target::NoTraps;
    let mut positional: Vec<String> = Vec::new();
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--target" {
            target = match args.get(i + 1).map(String::as_str) {
                Some("no_traps") => Target::NoTraps,
                Some("validate") => Target::Validate,
                other => {
                    eprintln!(
                        "Invalid --target '{}' (expected no_traps or validate)",
                        other.unwrap_or("")
                    );
                    usage(&program);
                }
            };
            i += 2;
        } else {
            positional.push(args[i].clone());
            i += 1;
        }
    }

    let Some(seed_file) = positional.first().cloned() else {
        usage(&program);
    };

    let seed_bytes = fs::read(&seed_file).unwrap_or_else(|e| {
        eprintln!("Failed to read seed file '{seed_file}': {e}");
        process::exit(1);
    });

    let wasm_bytes = wasm_from_seed(&seed_bytes, target).unwrap_or_else(|e| {
        eprintln!("Failed to generate Wasm from seed: {e:?}");
        eprintln!("The seed may be too small or corrupted.");
        process::exit(1);
    });

    // Output: `--stdout` or an optional file path (default: <seed>.wasm).
    if positional.get(1).map(String::as_str) == Some("--stdout") {
        io::stdout().write_all(&wasm_bytes).unwrap_or_else(|e| {
            eprintln!("Failed to write to stdout: {e}");
            process::exit(1);
        });
    } else {
        let output_file = positional
            .get(1)
            .cloned()
            .unwrap_or_else(|| format!("{seed_file}.wasm"));

        eprintln!("Writing to: {output_file}");

        fs::write(&output_file, &wasm_bytes).unwrap_or_else(|e| {
            eprintln!("Failed to write '{output_file}': {e}");
            process::exit(1);
        });

        eprintln!(
            "Generated {} bytes: {} -> {}",
            wasm_bytes.len(),
            seed_file,
            output_file
        );
    }
}
