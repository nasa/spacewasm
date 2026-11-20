//! `spacewasm features`: what this release implements, for the tools that build for it;
//! and what every release implements, for whoever is choosing one.

use crate::Outcome;
use crate::args::FeaturesArgs;
use spacewasm_tools::{
    PROPOSALS, Status, Version, clang_flags, clang_flags_for, rustc_target_features,
    rustc_target_features_for, supported_in, wasm_opt_flags, wasm_opt_flags_for,
};

pub fn run(args: &FeaturesArgs) -> Outcome {
    let release = args.release.map(Version::known).transpose()?;
    if args.wasm_opt {
        let flags = release.map_or_else(wasm_opt_flags, wasm_opt_flags_for);
        println!("{}", flags.join(" "));
        return Ok(true);
    }
    if args.rustc {
        let features = release.map_or_else(rustc_target_features, rustc_target_features_for);
        println!("{features}");
        return Ok(true);
    }
    if args.clang {
        let flags = release.map_or_else(clang_flags, clang_flags_for);
        println!("{}", flags.join(" "));
        return Ok(true);
    }
    if args.matrix {
        matrix();
        return Ok(true);
    }
    if let Some(release) = release {
        return release_features(release);
    }

    println!(
        "spacewasm {}: WebAssembly 1.0, plus",
        env!("CARGO_PKG_VERSION")
    );
    for proposal in PROPOSALS.iter().filter(|proposal| proposal.supported()) {
        println!("  {}", proposal.name);
    }
    println!();
    println!("wasm-opt:  {}", wasm_opt_flags().join(" "));
    println!(
        "rustc:     -Ctarget-cpu=mvp -Ctarget-feature={}",
        rustc_target_features()
    );
    println!("clang:     {}", clang_flags().join(" "));
    Ok(true)
}

/// The README's table: implemented proposals first, in the order they arrived, then
/// planned, under consideration, and not planned.
fn matrix() {
    let mut rows: Vec<(&str, String, &str)> = vec![("WebAssembly 1.0", "all versions".into(), "")];
    let mut proposals: Vec<_> = PROPOSALS.iter().collect();
    proposals.sort_by_key(|proposal| match proposal.status {
        Status::Since(version) => (0, version),
        Status::Planned => (1, Version::FIRST),
        Status::UnderConsideration => (2, Version::FIRST),
        Status::NotPlanned => (3, Version::FIRST),
    });
    rows.extend(proposals.iter().map(|proposal| {
        (
            proposal.title,
            proposal.status.to_string(),
            proposal.tracking.unwrap_or(""),
        )
    }));

    let title = rows.iter().map(|row| row.0.len()).max().unwrap_or(0);
    let status = rows.iter().map(|row| row.1.len()).max().unwrap_or(0);
    println!("{:title$}  {:status$}  Tracking", "Feature", "Status");
    for (name, since, tracking) in rows {
        let row = format!("{name:title$}  {since:status$}  {tracking}");
        println!("{}", row.trim_end());
    }
}

fn release_features(release: Version) -> Outcome {
    println!("spacewasm {release}: WebAssembly 1.0, plus");
    for proposal in supported_in(release) {
        println!("  {}", proposal.name);
    }
    println!();
    println!("wasm-opt:  {}", wasm_opt_flags_for(release).join(" "));
    println!(
        "rustc:     -Ctarget-cpu=mvp -Ctarget-feature={}",
        rustc_target_features_for(release)
    );
    println!("clang:     {}", clang_flags_for(release).join(" "));
    Ok(true)
}
