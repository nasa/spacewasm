use assert_cmd::cargo::*;
use predicates::prelude::*;

/// A `.wasm` fixture written to a unique temp path for the duration of a test,
/// removed on drop. Used by the negative-path tests, which need module bytes
/// that no committed fixture provides (garbage, empty, header-only) without
/// polluting `tests/wasm/`.
struct TempWasm(std::path::PathBuf);

impl TempWasm {
    fn new(tag: &str, bytes: &[u8]) -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!("spacewasi_it_{}_{tag}.wasm", std::process::id()));
        std::fs::write(&path, bytes).expect("write temp wasm fixture");
        TempWasm(path)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempWasm {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn fake_file() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = cargo_bin_cmd!("spacewasi");

    cmd.arg("this_file_is_not_real");
    cmd.assert().failure().stderr(predicate::str::contains(
        "error: wasm module path does not exist",
    ));

    Ok(())
}

#[test]
fn hello_universe() -> Result<(), Box<dyn std::error::Error>> {
    let path = "tests/wasm/hello_universe.wasm";

    let mut cmd = cargo_bin_cmd!("spacewasi");

    cmd.arg(path);
    let assertion = cmd.assert();

    assertion.success().stdout("hello universe!\n");

    Ok(())
}

#[test]
fn argv() -> Result<(), Box<dyn std::error::Error>> {
    let path = "tests/wasm/argv.wasm";

    let mut cmd = cargo_bin_cmd!("spacewasi");

    cmd.arg(path).arg("arg1").arg("arg2");
    let assertion = cmd.assert();

    assertion.success().stdout(format!("3 {path} arg1 arg2\n"));

    Ok(())
}

#[test]
fn argv0() -> Result<(), Box<dyn std::error::Error>> {
    let path = "tests/wasm/argv0.wasm";

    let mut cmd = cargo_bin_cmd!("spacewasi");

    cmd.arg("--argv0")
        .arg("arg0")
        .arg(path)
        .arg("arg1")
        .arg("arg2");
    let assertion = cmd.assert();

    assertion.success().stdout("arg0\n".to_string());

    Ok(())
}

#[test]
fn file_system() -> Result<(), Box<dyn std::error::Error>> {
    let path = "tests/wasm/fs.wasm";

    let mut cmd = cargo_bin_cmd!("spacewasi");

    cmd.arg("--dir").arg("tests/wasm/::/").arg(path);
    let assertion = cmd.assert();

    assertion
        .success()
        .stdout("SpaceWasm is cool!\n".to_string());

    Ok(())
}

#[test]
fn env() -> Result<(), Box<dyn std::error::Error>> {
    let path = "tests/wasm/env.wasm";

    let mut cmd = cargo_bin_cmd!("spacewasi");

    cmd.arg("--env").arg("TESTKEY=testvalue").arg(path);
    let assertion = cmd.assert();

    assertion.success().stdout("testvalue\n".to_string());

    Ok(())
}

#[test]
fn return_code() -> Result<(), Box<dyn std::error::Error>> {
    let path = "tests/wasm/rc.wasm";

    let mut cmd = cargo_bin_cmd!("spacewasi");

    cmd.arg(path);
    let assertion = cmd.assert();

    assertion.failure().code(87);

    Ok(())
}

// --- Negative paths -------------------------------------------------------
//
// These observe the CLI's own error surface (exit status + diagnostic on
// stderr) rather than just smoke-testing that a good module runs.

#[test]
fn malformed_module_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    // Bytes whose magic number is not `\0asm`: the parser must reject them.
    let wasm = TempWasm::new("malformed", b"this is definitely not a wasm module");

    let mut cmd = cargo_bin_cmd!("spacewasi");
    cmd.arg(wasm.path());
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("failed to parse WASM module"));

    Ok(())
}

#[test]
fn empty_module_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    // An empty file has no magic/version header at all.
    let wasm = TempWasm::new("empty", b"");

    let mut cmd = cargo_bin_cmd!("spacewasi");
    cmd.arg(wasm.path());
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("failed to parse WASM module"));

    Ok(())
}

#[test]
fn module_without_start_export_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    // A well-formed but empty module: the 4-byte magic and version-1 header
    // with no sections. It parses cleanly yet exports no `_start`, so the CLI
    // must reject it with a clear message instead of crashing.
    let wasm = TempWasm::new("nostart", &[0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00]);

    let mut cmd = cargo_bin_cmd!("spacewasi");
    cmd.arg(wasm.path());
    cmd.assert().failure().stderr(predicate::str::contains(
        "does not correctly export a _start function",
    ));

    Ok(())
}
