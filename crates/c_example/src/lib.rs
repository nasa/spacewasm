//! # c_example
//!
//! A C-ABI test runner for [`spacewasm_c_api`].

use std::path::{Path, PathBuf};
use std::process::Command;

/// Return the first available C compiler command, or `None` to skip C tests.
pub fn find_cc() -> Option<&'static str> {
    for cc in ["cc", "clang", "gcc"] {
        let ok = Command::new(cc)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if ok {
            return Some(cc);
        }
    }
    None
}

/// Path to the `spacewasm_c_api` crate's committed header directory.
pub fn include_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../spacewasm_c_api/include")
}

/// handler + staticlib) under `target_dir`, and return the directory holding
/// it. Panics on build failure.
///
pub fn build_staticlib(target_dir: &Path) -> PathBuf {
    let target = env!("C_EXAMPLE_TARGET");
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    let mut cmd = Command::new(env!("CARGO"));
    cmd.args(["build", "-p", "spacewasm_c_api", "--target", target]);
    if profile == "release" {
        cmd.arg("--release");
    }
    cmd.arg("--target-dir").arg(target_dir);

    for var in ["RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS"] {
        let Ok(flags) = std::env::var(var) else {
            continue;
        };
        // `CARGO_ENCODED_RUSTFLAGS` uses `\x1f` separators; `RUSTFLAGS` uses
        // whitespace. `-C instrument-coverage` may appear as a `-C`/value pair
        // (whitespace form) or fused as `-Cinstrument-coverage`.
        let sep = if var == "CARGO_ENCODED_RUSTFLAGS" {
            '\x1f'
        } else {
            ' '
        };
        let mut kept: Vec<&str> = Vec::new();
        let mut tokens = flags.split(sep).filter(|t| !t.is_empty()).peekable();
        while let Some(tok) = tokens.next() {
            if tok == "-C" && tokens.peek() == Some(&"instrument-coverage") {
                tokens.next(); // drop the paired value
                continue;
            }
            if tok == "-Cinstrument-coverage" || tok == "--cfg=coverage" {
                continue;
            }
            kept.push(tok);
        }
        cmd.env(var, kept.join(&sep.to_string()));
    }
    let status = cmd.status().expect("failed to launch cargo");
    assert!(
        status.success(),
        "failed to build spacewasm_c_api staticlib"
    );
    target_dir.join(target).join(profile)
}

/// Compile a C source file against the staticlib in `lib_dir` + header,
/// producing `out`. Returns whether compilation/linking succeeded.
pub fn compile_c(cc: &str, lib_dir: &Path, src: &Path, out: &Path) -> bool {
    let mut cmd = Command::new(cc);
    cmd.arg(src)
        .arg(format!("-I{}", include_dir().display()))
        .arg(format!("-L{}", lib_dir.display()))
        .arg("-lspacewasm_c_api")
        .arg("-g")
        .arg("-o")
        .arg(out);

    cmd.status().expect("failed to launch C compiler").success()
}
