//! Compiles `examples/ctest.c` against the generated header and the built
//! staticlib, then runs it. This proves the C ABI + header link and execute
//! end-to-end. Skipped (passes trivially) if no C compiler is available.

use std::path::PathBuf;
use std::process::Command;

fn have_cc(cc: &str) -> bool {
    Command::new(cc)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Locate the freshly built `libspacewasm_c.a`. Cargo places test/build output
/// under target/<profile>/deps, with the staticlib at target/<profile>/.
fn staticlib_dir() -> PathBuf {
    // The test binary lives at target/<profile>/deps/<bin>; the staticlib is two
    // levels up in target/<profile>/.
    let mut dir = std::env::current_exe().unwrap();
    dir.pop(); // remove test bin name
    if dir.ends_with("deps") {
        dir.pop();
    }
    dir
}

#[test]
fn c_program_links_and_runs() {
    let cc = if have_cc("cc") {
        "cc"
    } else if have_cc("clang") {
        "clang"
    } else if have_cc("gcc") {
        "gcc"
    } else {
        eprintln!("no C compiler found; skipping C ABI link test");
        return;
    };

    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let src = manifest.join("examples/ctest.c");
    // The C header is generated into and shipped from the spacewasm_ffi crate.
    let include = manifest.join("../spacewasm_ffi/include");
    let libdir = staticlib_dir();

    let out = std::env::temp_dir().join("spacewasm_ctest_bin");

    // Compile and link against the staticlib.
    let mut cmd = Command::new(cc);
    cmd.arg(&src)
        .arg(format!("-I{}", include.display()))
        .arg(format!("-L{}", libdir.display()))
        .arg("-lspacewasm_c_example")
        .arg("-o")
        .arg(&out);

    // Platform system libraries the Rust staticlib depends on.
    if cfg!(target_os = "macos") {
        cmd.args(["-framework", "CoreFoundation", "-framework", "Security"]);
    } else if cfg!(target_os = "linux") {
        cmd.args(["-lpthread", "-ldl", "-lm"]);
    }

    let status = cmd.status().expect("failed to launch C compiler");
    assert!(
        status.success(),
        "C compile/link failed; is libspacewasm_c_example.a built? (run `cargo build -p spacewasm_c_example`)"
    );

    let run = Command::new(&out)
        .output()
        .expect("failed to run C program");
    assert!(
        run.status.success(),
        "C program failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("add(20, 22) = 42"),
        "unexpected output: {}",
        String::from_utf8_lossy(&run.stdout),
    );
}
