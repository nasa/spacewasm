/// This build script was adapted from the Rust Embedded Book and associated examples:
/// https://docs.rust-embedded.org/book/start/qemu.html
use std::env;
use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;

fn main() {
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();

    let mut memory_layout: Vec<u8> = Vec::new();
    let _ = File::open(format!("memory/{arch}.x")).unwrap().read_to_end(&mut memory_layout);

    let out = &PathBuf::from(env::var_os("OUT_DIR").unwrap());
    File::create(out.join("memory.x"))
        .unwrap()
        .write_all(&memory_layout)
        .unwrap();
    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rerun-if-changed=memory/{arch}.x");
    println!("cargo:rustc-link-arg=--nmagic");

    if arch == "riscv64" || arch == "riscv32" {
        println!("cargo:rustc-link-arg=-Tmemory.x");
    }
    else {
        println!("cargo:rustc-link-arg=-Tlink.x");
    }
}
