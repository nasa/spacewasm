use assert_cmd::cargo::*;
use predicates::prelude::*;

/// A `.wasm` fixture written to a unique temp path for the duration of a test,
/// removed on drop. Used by the negative-path tests, which need module bytes
/// that no committed fixture provides (garbage, empty, header-only) without
/// polluting `tests/wasi/`.
struct TempWasm(std::path::PathBuf);

impl TempWasm {
    fn new(tag: &str, bytes: &[u8]) -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!("spacewasm_run_{}_{tag}.wasm", std::process::id()));
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
    let mut cmd = cargo_bin_cmd!("spacewasm");
    cmd.arg("run");

    cmd.arg("this_file_is_not_real");
    cmd.assert().failure().stderr(predicate::str::contains(
        "could not read this_file_is_not_real",
    ));

    Ok(())
}

#[test]
fn hello_universe() -> Result<(), Box<dyn std::error::Error>> {
    let path = "tests/wasi/hello_universe.wasm";

    let mut cmd = cargo_bin_cmd!("spacewasm");
    cmd.arg("run");

    cmd.arg(path);
    let assertion = cmd.assert();

    assertion.success().stdout("hello universe!\n");

    Ok(())
}

#[test]
fn argv() -> Result<(), Box<dyn std::error::Error>> {
    let path = "tests/wasi/argv.wasm";

    let mut cmd = cargo_bin_cmd!("spacewasm");
    cmd.arg("run");

    cmd.arg(path).arg("arg1").arg("arg2");
    let assertion = cmd.assert();

    assertion.success().stdout(format!("3 {path} arg1 arg2\n"));

    Ok(())
}

#[test]
fn argv0() -> Result<(), Box<dyn std::error::Error>> {
    let path = "tests/wasi/argv0.wasm";

    let mut cmd = cargo_bin_cmd!("spacewasm");
    cmd.arg("run");

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
    let path = "tests/wasi/fs.wasm";

    let mut cmd = cargo_bin_cmd!("spacewasm");
    cmd.arg("run");

    cmd.arg("--dir").arg("tests/wasi/::/").arg(path);
    let assertion = cmd.assert();

    assertion
        .success()
        .stdout("SpaceWasm is cool!\n".to_string());

    Ok(())
}

#[test]
fn env() -> Result<(), Box<dyn std::error::Error>> {
    let path = "tests/wasi/env.wasm";

    let mut cmd = cargo_bin_cmd!("spacewasm");
    cmd.arg("run");

    cmd.arg("--env").arg("TESTKEY=testvalue").arg(path);
    let assertion = cmd.assert();

    assertion.success().stdout("testvalue\n".to_string());

    Ok(())
}

#[test]
fn return_code() -> Result<(), Box<dyn std::error::Error>> {
    let path = "tests/wasi/rc.wasm";

    let mut cmd = cargo_bin_cmd!("spacewasm");
    cmd.arg("run");

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

    let mut cmd = cargo_bin_cmd!("spacewasm");
    cmd.arg("run");
    cmd.arg(wasm.path());
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("does not load"));

    Ok(())
}

#[test]
fn empty_module_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    // An empty file has no magic/version header at all.
    let wasm = TempWasm::new("empty", b"");

    let mut cmd = cargo_bin_cmd!("spacewasm");
    cmd.arg("run");
    cmd.arg(wasm.path());
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("does not load"));

    Ok(())
}

#[test]
fn module_without_start_export_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    // A well-formed but empty module: the 4-byte magic and version-1 header
    // with no sections. It parses cleanly yet exports no `_start`, so the CLI
    // must reject it with a clear message instead of crashing.
    let wasm = TempWasm::new("nostart", &[0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00]);

    let mut cmd = cargo_bin_cmd!("spacewasm");
    cmd.arg("run");
    cmd.arg(wasm.path());
    cmd.assert().failure().stderr(predicate::str::contains(
        "does not export a `_start` function",
    ));

    Ok(())
}

/// `trace` runs every export and prints the last instructions each executed.
#[test]
fn trace_prints_each_exports_last_instructions() -> Result<(), Box<dyn std::error::Error>> {
    let module = wat::parse_str(
        r#"(module
             (func (export "add") (param i32 i32) (result i32)
               local.get 0 local.get 1 i32.add)
             (func (export "boom") unreachable))"#,
    )?;
    let wasm = TempWasm::new("trace", &module);

    let mut cmd = cargo_bin_cmd!("spacewasm");
    cmd.arg("trace").arg(wasm.path()).arg("--limit").arg("8");
    cmd.assert()
        .failure()
        .stdout(predicate::str::contains("`add`:"))
        .stdout(predicate::str::contains("i32_add"))
        .stdout(predicate::str::contains("`boom`:"))
        .stderr(predicate::str::contains("`add`: completed"))
        .stderr(predicate::str::contains("`boom`: trapped, Unreachable"));

    Ok(())
}

/// Piped in, as `make trace` feeds it a fuzzer seed.
#[test]
fn trace_reads_the_module_from_stdin() -> Result<(), Box<dyn std::error::Error>> {
    let module = wat::parse_str(r#"(module (func (export "f") nop))"#)?;

    let mut cmd = cargo_bin_cmd!("spacewasm");
    cmd.arg("trace").write_stdin(module);
    cmd.assert()
        .success()
        .stderr(predicate::str::contains("`f`: completed"));

    Ok(())
}

/// `lower` takes out what this release lacks and keeps what it has. Skipped where
/// `wasm-opt` is not installed.
#[test]
fn lower_keeps_supported_proposals_and_lowers_the_rest() -> Result<(), Box<dyn std::error::Error>> {
    if std::process::Command::new("wasm-opt")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("wasm-opt not found; skipping");
        return Ok(());
    }
    let module = wat::parse_str(
        r#"(module
             (memory 1)
             (func (export "main") (param f32 i32) (result i32)
               i32.const 0 i32.const 0 i32.const 8 memory.fill
               local.get 0 i32.trunc_sat_f32_s
               local.get 1 i32.extend8_s
               i32.add))"#,
    )?;
    let input = TempWasm::new("lower_in", &module);
    let output = TempWasm::new("lower_out", b"");

    let mut cmd = cargo_bin_cmd!("spacewasm");
    cmd.arg("lower")
        .arg(input.path())
        .arg("-o")
        .arg(output.path());
    cmd.assert().success();

    let lowered = wasmprinter::print_bytes(std::fs::read(output.path())?)?;
    assert!(
        !lowered.contains("memory.fill"),
        "bulk memory is lowered:\n{lowered}"
    );
    assert!(lowered.contains("i32.trunc_sat_f32_s"), "kept:\n{lowered}");
    assert!(lowered.contains("i32.extend8_s"), "kept:\n{lowered}");

    let mut validate = cargo_bin_cmd!("spacewasm");
    validate.arg("validate").arg(output.path());
    validate.assert().success();
    Ok(())
}
