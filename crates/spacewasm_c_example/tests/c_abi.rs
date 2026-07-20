//! Compiles the C programs under `examples/` against the generated header and
//! the freshly built `spacewasm_c_api` staticlib, then runs them. This proves
//! the standalone C library links and executes end-to-end with only C-supplied
//! hooks (`spacewasm_panic` + the registered heap allocator). Skipped (passes
//! trivially) if no C compiler is available.

use std::path::PathBuf;
use std::process::Command;

use spacewasm_c_example::{build_staticlib, compile_c, find_cc};

/// Compile `examples/<name>` against the staticlib, run it, and assert success.
/// `expect` (if set) must appear in stdout.
fn compile_and_run(name: &str, expect: Option<&str>) {
    let Some(cc) = find_cc() else {
        eprintln!("no C compiler found; skipping C ABI test for {name}");
        return;
    };

    build_staticlib();

    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join(name);
    let out = std::env::temp_dir().join(format!("spacewasm_{}", name.replace('.', "_")));

    assert!(
        compile_c(cc, &src, &out),
        "C compile/link failed for {name}; is libspacewasm_c_api.a built? \
         (run `cargo build -p spacewasm_c_api`)"
    );

    let program = out.to_string_lossy().to_string();

    let run = Command::new(&out)
        .output()
        .expect("failed to run C program");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success(),
        "{name} ({program}) failed:\nstdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&run.stderr),
    );
    if let Some(expect) = expect {
        assert!(
            stdout.contains(expect),
            "{name} unexpected output: {stdout}"
        );
    }
}

/// The minimal example: load and invoke `add(20, 22)`.
#[test]
fn ctest_links_and_runs() {
    compile_and_run("ctest.c", Some("add(20, 22) = 42"));
}

/// The full ported ABI suite: modules, streaming, host functions, error paths,
/// statistics, and the no-leak lifecycle check.
#[test]
fn ctest_suite_passes() {
    compile_and_run("ctest_suite.c", Some("C ABI tests passed"));
}
