//! Build script for `spacewasm_ffi`. Two jobs:
//!
//! 1. Generate `config.rs` (included by `src/config.rs`) with the compile-time
//!    interpreter capacities used by the `capi` entry points. Each value is
//!    read from an environment variable, falling back to a sane default when
//!    unset. C cannot supply const-generic parameters, so an integrator tunes
//!    the build by setting these variables (e.g. in a `.cargo/config.toml`
//!    `[env]` block or on the `cargo build` command line) rather than editing
//!    Rust source.
//!
//! 2. When the `generate-header` feature is enabled, regenerate the committed
//!    C header `include/spacewasm.h` from the Rust source with cbindgen.

use std::env;
use std::fs;
use std::path::Path;

/// One tunable capacity: its env var, the identifier emitted into `config.rs`,
/// and the default used when the variable is unset.
struct Setting {
    env: &'static str,
    ident: &'static str,
    default: usize,
}

const SETTINGS: &[Setting] = &[
    Setting {
        env: "SPACEWASM_CONTROL_FRAMES",
        ident: "MAX_CONTROL_FRAMES",
        default: 64,
    },
    Setting {
        env: "SPACEWASM_STACK_DEPTH",
        ident: "MAX_STACK_DEPTH",
        default: 256,
    },
];

fn main() {
    let mut out = String::new();

    for s in SETTINGS {
        // Rebuild whenever the controlling variable changes.
        println!("cargo:rerun-if-env-changed={}", s.env);

        let value = match env::var(s.env) {
            Ok(raw) => raw.trim().parse::<usize>().unwrap_or_else(|_| {
                panic!("{} must be a non-negative integer, got {raw:?}", s.env)
            }),
            Err(env::VarError::NotPresent) => s.default,
            Err(env::VarError::NotUnicode(_)) => {
                panic!("{} must be valid UTF-8", s.env)
            }
        };

        out.push_str(&format!(
            "/// `{}` const-generic parameter (env `{}`, default {}).\n\
             pub const {}: usize = {};\n",
            s.ident, s.env, s.default, s.ident, value
        ));
    }

    let out_dir: String = env::var("OUT_DIR").expect("OUT_DIR not set");
    println!("cargo:rustc-env=SPACEWASM_CONFIG_DIR={out_dir}");

    let dest = Path::new(&out_dir).join("spacewasm_config.rs");
    fs::write(&dest, out).expect("failed to write generated config.rs");

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
