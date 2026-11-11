//! `spacewasm validate`: the decoder's verdict, and the load measured against the flight
//! heap.

use assert_cmd::cargo::*;
use predicates::prelude::*;

const MODULE: &str = "tests/wasi/hello_universe.wasm";

#[test]
fn reports_the_heap_a_module_takes() {
    cargo_bin_cmd!("spacewasm")
        .args(["validate", MODULE])
        .assert()
        .success()
        .stdout(
            predicate::str::is_match(r"ok, .* heap bytes in \d+ of 16 8192-byte pages\n$").unwrap(),
        );
}

#[test]
fn verbose_charges_the_heap_to_sections() {
    let output = cargo_bin_cmd!("spacewasm")
        .args(["validate", "--verbose", MODULE])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();

    let heap = |section: &str| -> usize {
        let row = stdout
            .lines()
            .find(|line| line.split_whitespace().next() == Some(section))
            .unwrap_or_else(|| panic!("no `{section}` row in:\n{stdout}"));
        row.split_whitespace().nth(2).unwrap().parse().unwrap()
    };
    assert!(heap("code") > 0, "the code section allocates:\n{stdout}");
    assert_eq!(heap("data"), 0, "data goes to linear memory:\n{stdout}");

    // The rows add up to the total, and the total to the summary line.
    const SECTIONS: [&str; 11] = [
        "header", "type", "import", "function", "table", "memory", "global", "export", "element",
        "code", "data",
    ];
    let rows: usize = SECTIONS.into_iter().map(heap).sum();
    assert_eq!(rows, heap("total"), "{stdout}");
    assert!(stdout.contains("ok, 25502 bytes"), "{stdout}");
    assert!(
        stdout.contains(&format!(" {} heap bytes", heap("total"))),
        "{stdout}"
    );
}

#[test]
fn a_heap_too_small_fails() {
    cargo_bin_cmd!("spacewasm")
        .args(["validate", "--heap-pages", "1", MODULE])
        .assert()
        .failure()
        .stderr(predicate::str::contains("of 1 8192-byte pages. Raise"));
}

#[test]
fn an_allocation_larger_than_a_page_fails() {
    cargo_bin_cmd!("spacewasm")
        .args(["validate", "--heap-page-size", "64", MODULE])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "more than a 64-byte heap page holds",
        ));
}
