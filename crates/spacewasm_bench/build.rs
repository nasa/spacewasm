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

    // we don't need to have a custom build script for these:
    if arch == "aarch64" || arch == "x86_64" {
        return;
    }

    let mut memory_layout: Vec<u8> = Vec::new();
    let _ = File::open(format!("memory/{arch}.x"))
        .unwrap()
        .read_to_end(&mut memory_layout);

    let out = &PathBuf::from(env::var_os("OUT_DIR").unwrap());
    File::create(out.join("memory.x"))
        .unwrap()
        .write_all(&memory_layout)
        .unwrap();
    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rerun-if-changed=memory/{arch}.x");
    println!("cargo:rustc-link-arg=--nmagic");

    // riscv memory.x layouts include builtin link.x, but
    // arm memory.x layouts are included BY builtin link.x:
    if arch == "riscv64" || arch == "riscv32" {
        println!("cargo:rustc-link-arg=-Tmemory.x");
    } else {
        println!("cargo:rustc-link-arg=-Tlink.x");
    }
}
