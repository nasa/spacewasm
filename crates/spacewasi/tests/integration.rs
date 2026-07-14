use assert_cmd::cargo::*;
use predicates::prelude::*;

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

    cmd.arg(&path);
    let assertion = cmd.assert();

    assertion.success().stdout("hello universe!\n");

    Ok(())
}

#[test]
fn argv() -> Result<(), Box<dyn std::error::Error>> {
    let path = "tests/wasm/argv.wasm";

    let mut cmd = cargo_bin_cmd!("spacewasi");

    cmd.arg(&path).arg("arg1").arg("arg2");
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
        .arg(&path)
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

    cmd.arg("--dir").arg("tests/wasm/::/").arg(&path);
    let assertion = cmd.assert();

    assertion
        .success()
        .stdout("spacewasm is cool!\n".to_string());

    Ok(())
}

#[test]
fn env() -> Result<(), Box<dyn std::error::Error>> {
    let path = "tests/wasm/env.wasm";

    let mut cmd = cargo_bin_cmd!("spacewasi");

    cmd.arg("--env").arg("TESTKEY=testvalue").arg(&path);
    let assertion = cmd.assert();

    assertion.success().stdout("testvalue\n".to_string());

    Ok(())
}

#[test]
fn return_code() -> Result<(), Box<dyn std::error::Error>> {
    let path = "tests/wasm/rc.wasm";

    let mut cmd = cargo_bin_cmd!("spacewasi");

    cmd.arg(&path);
    let assertion = cmd.assert();

    assertion.failure().code(87);

    Ok(())
}
