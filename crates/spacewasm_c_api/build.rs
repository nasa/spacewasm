use std::env;
use std::path::Path;

fn main() {
    let config = match env::var("SPACEWASM_CONFIG") {
        Ok(raw) => raw.trim().to_string(),
        Err(env::VarError::NotPresent) => {
            let crate_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
            let crate_dir = Path::new(&crate_dir);
            let header = crate_dir.join("include").join("config.rs");
            header.to_str().unwrap().to_string()
        }
        Err(env::VarError::NotUnicode(_)) => {
            panic!("SPACEWASM_CONFIG must be valid UTF-8")
        }
    };

    println!("cargo:rustc-env=SPACEWASM_CONFIG={config}");
    generate_header();
}

/// Regenerate `include/spacewasm.h` from the Rust source with cbindgen. Only
/// compiled in when the `generate-header` feature is on; otherwise a no-op so
/// lean builds carry no cbindgen dependency and consume the committed header.
#[cfg(feature = "codegen")]
fn generate_header() {
    let crate_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let crate_dir = Path::new(&crate_dir);
    let header = crate_dir.join("include").join("spacewasm.h");

    // Rerun when the header inputs change. cbindgen reads the whole crate, but
    // these are the files that shape the public surface + rendering.
    println!("cargo:rerun-if-changed=cbindgen.toml");
    println!("cargo:rerun-if-changed=src");

    let config = cbindgen::Config::from_file(crate_dir.join("cbindgen.toml"))
        .expect("failed to read cbindgen.toml");

    cbindgen::Builder::new()
        .with_crate(crate_dir)
        .with_config(config)
        .generate()
        .expect("cbindgen failed to generate the C header")
        // `write_to_file` is content-aware: it leaves the file untouched (and
        // its mtime unchanged) when the output is identical, so regenerating
        // does not trigger a rebuild loop.
        .write_to_file(&header);
}

#[cfg(not(feature = "codegen"))]
fn generate_header() {}
