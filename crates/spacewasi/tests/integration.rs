use assert_cmd::cargo::*;
use predicates::prelude::*;

use std::{fs, process::Command as ProcessCommand};

fn compile_c_to_wasm(source: &str) -> String {
    let output = source.replace(".c", ".wasm");
    let _ = ProcessCommand::new("emcc")
        .arg(&source)
        .arg("-O3")
        .arg("-mcpu=mvp")
        .arg("-mno-sign-ext")
        .arg("-mno-bulk-memory")
        .arg("-o")
        .arg(&output)
        .output()
        .unwrap_or_else(|e| panic!("Failed to run emcc: {e}"));
    let _ = ProcessCommand::new("scripts/wasm2mvp.sh")
        .arg(&output)
        .arg(&output)
        .output()
        .unwrap_or_else(|e| panic!("Failed to run emcc: {e}"));

    output
}

#[test]
fn fake_file() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = cargo_bin_cmd!("spacewasi");

    cmd.arg("this_file_is_not_real");
    cmd.assert().failure().stderr(predicate::str::contains("error: wasm module path does not exist"));

    Ok(())
}

#[test]
fn hello_universe() -> Result<(), Box<dyn std::error::Error>> {
    let path = compile_c_to_wasm("tests/wasm/hello_universe.c");

    let mut cmd = cargo_bin_cmd!("spacewasi");

    cmd.arg(&path);
    let assertion = cmd.assert();

    let _ = fs::remove_file(path);

    assertion.success().stdout("hello universe!\n");

    Ok(())
}

#[test]
fn argv() -> Result<(), Box<dyn std::error::Error>> {
    let path = compile_c_to_wasm("tests/wasm/argv.c");

    let mut cmd = cargo_bin_cmd!("spacewasi");

    cmd.arg(&path).arg("arg1").arg("arg2");
    let assertion = cmd.assert();

    let _ = fs::remove_file(&path);

    assertion.success().stdout(format!("3 {path} arg1 arg2\n"));

    Ok(())
}

#[test]
fn argv0() -> Result<(), Box<dyn std::error::Error>> {
    let path = compile_c_to_wasm("tests/wasm/argv0.c");

    let mut cmd = cargo_bin_cmd!("spacewasi");

    cmd.arg("--argv0").arg("arg0").arg(&path).arg("arg1").arg("arg2");
    let assertion = cmd.assert();

    let _ = fs::remove_file(&path);

    assertion.success().stdout(format!("arg0\n"));

    Ok(())
}

#[test]
fn fs() -> Result<(), Box<dyn std::error::Error>> {
    let path = compile_c_to_wasm("tests/wasm/fs.c");

    let mut cmd = cargo_bin_cmd!("spacewasi");

    cmd.arg("--dir").arg("tests/wasm/::/").arg(&path);
    let assertion = cmd.assert();

    // let _ = fs::remove_file(&path);

    assertion.success().stdout(format!("spacewasm is cool\n"));

    Ok(())
}