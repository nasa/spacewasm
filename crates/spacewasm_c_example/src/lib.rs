//! # spacewasm_c_example
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

/// The directory holding the freshly built `libspacewasm_c_api.a`. Cargo places
/// the test binary under `target/<profile>/deps/`; the staticlib is one level up
/// in `target/<profile>/`.
pub fn staticlib_dir() -> PathBuf {
    let mut dir = std::env::current_exe().unwrap();
    dir.pop(); // test binary name
    if dir.ends_with("deps") {
        dir.pop();
    }
    dir
}

/// Path to the `spacewasm_c_api` crate's committed header directory.
pub fn include_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../spacewasm_c_api/include")
}

/// Ensure `libspacewasm_c_api.a` is built (default features → allocator + panic
/// handler + staticlib). Panics on build failure.
///
/// The build must land in the same `target/<profile>/` directory that
/// [`staticlib_dir`] hands to the linker. A test binary compiled with
/// `--release` runs from `target/release/deps/`, so a plain (debug) `cargo
/// build` would drop the staticlib in `target/debug/` and the `-L` search would
/// miss it. Mirror the profile by inspecting the directory name.
pub fn build_staticlib() {
    let mut cmd = Command::new(env!("CARGO"));
    cmd.args(["build", "-p", "spacewasm_c_api"]);
    if staticlib_dir().file_name().is_some_and(|n| n == "release") {
        cmd.arg("--release");
    }
    let status = cmd.status().expect("failed to launch cargo");
    assert!(
        status.success(),
        "failed to build spacewasm_c_api staticlib"
    );
}

/// Compile a C source file against the staticlib + header, producing `out`.
/// Returns whether compilation/linking succeeded.
pub fn compile_c(cc: &str, src: &Path, out: &Path) -> bool {
    let mut cmd = Command::new(cc);
    cmd.arg(src)
        .arg(format!("-I{}", include_dir().display()))
        .arg(format!("-L{}", staticlib_dir().display()))
        .arg("-lspacewasm_c_api")
        .arg("-g")
        .arg("-o")
        .arg(out);

    cmd.status().expect("failed to launch C compiler").success()
}
