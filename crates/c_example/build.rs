use std::env;

fn main() {
    // Forward the target triple so the staticlib can be built with an explicit
    // `--target`, which pins its output to `<target-dir>/<triple>/<profile>/`.
    let target = env::var("TARGET").expect("TARGET not set");
    println!("cargo:rustc-env=C_EXAMPLE_TARGET={target}");
}
