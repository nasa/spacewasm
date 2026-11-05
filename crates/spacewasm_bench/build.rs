/// A build script for linking the device layouts in memory/ for embedded
/// applications.
///
/// Copyright 2026 California Institute of Technology
///
/// Licensed under the Apache License, Version 2.0 (the "License");
/// you may not use this file except in compliance with the License.
/// You may obtain a copy of the License at
///
/// <http://www.apache.org/licenses/LICENSE-2.0>
///
/// ---
/// Large portions of this file are derived from the Rust Embedded Book
/// and associated examples at <https://docs.rust-embedded.org/book/start/qemu.html>
use std::env;
use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;

fn main() {
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();

    // Some targets can run on more than one board (e.g. thumbv7em-none-eabihf
    // on both a Cortex-M4 and a Cortex-M7). SPACEWASM_BENCH_BOARD selects a
    // non-default board, which has its own memory layout and clock setup.
    println!("cargo:rustc-check-cfg=cfg(board_mps2_an500)");
    println!("cargo:rerun-if-env-changed=SPACEWASM_BENCH_BOARD");
    let board = env::var("SPACEWASM_BENCH_BOARD").ok();

    // we don't need to have a custom build script for these:
    if arch == "aarch64" || arch == "x86_64" {
        return;
    }

    let layout = match board.as_deref() {
        None | Some("") => format!("memory/{arch}.x"),
        Some("mps2-an500") if arch == "arm" => {
            println!("cargo:rustc-cfg=board_mps2_an500");
            "memory/arm-mps2-an500.x".to_string()
        }
        Some(other) => panic!("unsupported SPACEWASM_BENCH_BOARD `{other}` for {arch}"),
    };

    let mut memory_layout: Vec<u8> = Vec::new();
    let _ = File::open(&layout).unwrap().read_to_end(&mut memory_layout);

    let out = &PathBuf::from(env::var_os("OUT_DIR").unwrap());
    File::create(out.join("memory.x"))
        .unwrap()
        .write_all(&memory_layout)
        .unwrap();
    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rerun-if-changed={layout}");
    println!("cargo:rustc-link-arg=--nmagic");

    // riscv memory.x layouts include builtin link.x, but
    // arm memory.x layouts are included BY builtin link.x:
    if arch == "riscv64" || arch == "riscv32" {
        println!("cargo:rustc-link-arg=-Tmemory.x");
    } else {
        println!("cargo:rustc-link-arg=-Tlink.x");
    }
}
