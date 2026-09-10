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

    println!("cargo:rerun-if-env-changed=SPACEWASM_CONFIG");
    println!("cargo:rustc-env=SPACEWASM_CONFIG={config}");
    generate_header();
}

/// Regenerate `include/spacewasm.h` from the Rust source with cbindgen. Only
/// compiled in when the `codegen` feature is on; otherwise a no-op so
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

    // Attatch [noreturn] to spacewasm_panic
    annotate_noreturn(&header);
}

/// Prefix the generated `spacewasm_panic` declaration with `SPACEWASM_NORETURN`.
#[cfg(feature = "codegen")]
fn annotate_noreturn(header: &Path) {
    use std::fs;

    const DECL: &str = "extern void spacewasm_panic(";
    const ANNOTATED: &str = "SPACEWASM_NORETURN extern void spacewasm_panic(";

    let contents = fs::read_to_string(header).expect("failed to read generated header");
    // Idempotent: nothing to do if the annotation is already present.
    if contents.contains(ANNOTATED) {
        return;
    }
    let patched = contents.replacen(DECL, ANNOTATED, 1);
    assert!(
        patched != contents,
        "expected `{DECL}` in the generated header to annotate with SPACEWASM_NORETURN"
    );
    fs::write(header, patched).expect("failed to write annotated header");
}

#[cfg(not(feature = "codegen"))]
fn generate_header() {}
