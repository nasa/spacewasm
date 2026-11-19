//! `spacewasm features`: what this release implements, for the tools that build for it;
//! and what every release implements, for whoever is choosing one.

use crate::Outcome;
use crate::args::FeaturesArgs;
use spacewasm_tools::{PROPOSALS, Status, Version, rustc_target_features, wasm_opt_flags};

pub fn run(args: &FeaturesArgs) -> Outcome {
    if args.wasm_opt {
        println!("{}", wasm_opt_flags().join(" "));
        return Ok(true);
    }
    if args.rustc {
        println!("{}", rustc_target_features());
        return Ok(true);
    }
    if args.matrix {
        matrix();
        return Ok(true);
    }
    if let Some(release) = args.release {
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
    if release < Version::FIRST {
        return Err(format!(
            "there is no spacewasm {release}; the first is {}",
            Version::FIRST
        )
        .into());
    }
    // A build from an untagged tree is ahead of every tagged release, so it can answer for
    // all of them.
    if let Some(this) = Version::this_release()
        && release > this
    {
        return Err(format!(
            "this is spacewasm {this}, which cannot know what {release} implements. \
             Upgrade spacewasm_tools to ask about it"
        )
        .into());
    }

    println!("spacewasm {release}: WebAssembly 1.0, plus");
    for proposal in PROPOSALS.iter().filter(|p| p.supported_in(release)) {
        println!("  {}", proposal.name);
    }
    Ok(true)
}
