//! Convert fuzzer seed artifacts to Wasm binaries.
//!
//! Fuzzer artifacts are arbitrary byte streams that get passed through
//! wasm-smith to generate valid Wasm. This tool converts seeds to Wasm
//! for analysis with other tools (disassemblers, trace utilities, etc.).
//!
//! Usage:
//!   seed_to_wasm <seed-file> [output.wasm]
//!   seed_to_wasm <seed-file> --stdout

use arbitrary::{Arbitrary, Unstructured};
use spacewasm_fuzzing::generators::NoTrapsModule;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <seed-file> [output.wasm]", args[0]);
        eprintln!("       {} <seed-file> --stdout", args[0]);
        eprintln!();
        eprintln!("Convert fuzzer seed artifacts to Wasm binaries.");
        eprintln!();
        eprintln!("Examples:");
        eprintln!("  # Write to file");
        eprintln!(
            "  {} fuzz/artifacts/no_traps/crash-xxx output.wasm",
            args[0]
        );
        eprintln!();
        eprintln!("  # Write to stdout");
        eprintln!(
            "  {} fuzz/artifacts/no_traps/crash-xxx --stdout | wasm2wat",
            args[0]
        );
        process::exit(1);
    }

    let seed_file = &args[1];
    let seed_bytes = fs::read(seed_file).unwrap_or_else(|e| {
        eprintln!("Failed to read seed file '{}': {}", seed_file, e);
        process::exit(1);
    });

    // Generate Wasm from seed
    let mut unstructured = Unstructured::new(&seed_bytes);
    let module = match NoTrapsModule::arbitrary(&mut unstructured) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Failed to generate Wasm from seed: {:?}", e);
            eprintln!("The seed may be too small or corrupted.");
            process::exit(1);
        }
    };

    let wasm_bytes = module.wasm();

    // Output handling
    if args.len() > 2 && args[2] == "--stdout" {
        // Write to stdout
        io::stdout().write_all(wasm_bytes).unwrap_or_else(|e| {
            eprintln!("Failed to write to stdout: {}", e);
            process::exit(1);
        });
    } else {
        // Write to file
        let output_file = if args.len() > 2 {
            args[2].clone()
        } else {
            // Default: seed filename + .wasm
            format!("{}.wasm", seed_file)
        };

        eprintln!("Writing to: {}", output_file);

        fs::write(&output_file, wasm_bytes).unwrap_or_else(|e| {
            eprintln!("Failed to write '{}': {}", output_file, e);
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
