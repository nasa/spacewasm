//! Which WebAssembly proposals this release of `spacewasm` implements, and how each
//! tool spells them.
//!
//! The one place that list lives: `link` hands it to `wasm-opt`, `features` prints it
//! for `rustc`, and `validate` names a proposal when a module uses one that is missing.
//! It also records the release each proposal arrived in, and what is planned for the
//! rest, so `features --matrix` and `features --release` can answer for any release.
//! The decoder is the ground truth; the test below holds this table to it by
//! decoding one instruction from every proposal listed.

use std::fmt;

/// A post-MVP proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Proposal {
    /// Its name in the WebAssembly proposals repository.
    pub name: &'static str,
    /// Its row in the README's feature matrix.
    pub title: &'static str,
    /// `wasm-opt`'s `--enable-NAME` / `--disable-NAME` suffix.
    pub wasm_opt: &'static str,
    /// `rustc`'s `-Ctarget-feature` name, if it gates instructions there. It is LLVM's,
    /// so `clang`'s `-mNAME` / `-mno-NAME` too.
    pub rustc: Option<&'static str>,
    /// Since which release `spacewasm` implements it, or what is planned for it.
    pub status: Status,
    /// The `spacewasm` issue or pull request tracking it.
    pub tracking: Option<&'static str>,
    /// The `wasm-opt` pass that rewrites this proposal's instructions into MVP ones, for
    /// a module built without regard to `spacewasm` (`spacewasm lower`).
    pub lowering: Option<&'static str>,
}

impl Proposal {
    /// Whether this release of `spacewasm` implements it.
    pub fn supported(&self) -> bool {
        matches!(self.status, Status::Since(_))
    }

    /// Whether `release` of `spacewasm` implements it.
    pub fn supported_in(&self, release: Version) -> bool {
        matches!(self.status, Status::Since(since) if since <= release)
    }
}

/// Where `spacewasm` stands on a proposal: the feature matrix in the README.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Implemented, from this release on.
    Since(Version),
    /// Not implemented yet, but intended to be.
    Planned,
    /// Not implemented; whether it will be is open.
    UnderConsideration,
    /// Not implemented, and not intended to be.
    NotPlanned,
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Status::Since(Version::FIRST) => f.write_str("all versions"),
            Status::Since(version) => write!(f, ">={version}"),
            Status::Planned => f.write_str("planned"),
            Status::UnderConsideration => f.write_str("under consideration"),
            Status::NotPlanned => f.write_str("not planned"),
        }
    }
}

/// A release of `spacewasm`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    /// The first release.
    pub const FIRST: Version = Version::new(0, 1, 0);

    pub const fn new(major: u32, minor: u32, patch: u32) -> Version {
        Version {
            major,
            minor,
            patch,
        }
    }

    /// This release, or `None` for a build from an untagged tree, whose version is the
    /// `0.0.0` placeholder the release workflow replaces.
    pub fn this_release() -> Option<Version> {
        let version: Version = env!("CARGO_PKG_VERSION")
            .parse()
            .expect("Cargo's version is MAJOR.MINOR.PATCH");
        (version != Version::new(0, 0, 0)).then_some(version)
    }

    /// `self`, if this build can say what it implements: no earlier than the first
    /// release, and no later than this one.
    pub fn known(self) -> Result<Version, String> {
        if self < Version::FIRST {
            return Err(format!(
                "there is no spacewasm {self}; the first is {}",
                Version::FIRST
            ));
        }
        // A build from an untagged tree is ahead of every tagged release, so it can answer
        // for all of them.
        match Version::this_release() {
            Some(this) if self > this => Err(format!(
                "this is spacewasm {this}, which cannot know what {self} implements. \
                 Upgrade spacewasm_tools to build for it"
            )),
            _ => Ok(self),
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl std::str::FromStr for Version {
    type Err = String;

    /// `MAJOR.MINOR.PATCH`, as a tag spells it (`v0.8.0`) or Cargo does (`0.8.0`). A
    /// pre-release or build suffix is ignored: `0.8.0-rc1` has 0.8.0's features.
    fn from_str(text: &str) -> Result<Self, String> {
        let core = text.strip_prefix('v').unwrap_or(text);
        let core = core.split(['-', '+']).next().unwrap_or(core);
        let parts = core
            .split('.')
            .map(|part| {
                part.parse::<u32>()
                    .map_err(|_| format!("`{text}` is not MAJOR.MINOR.PATCH"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        match parts[..] {
            [major, minor, patch] => Ok(Version::new(major, minor, patch)),
            _ => Err(format!("`{text}` is not MAJOR.MINOR.PATCH")),
        }
    }
}

/// Every proposal a Wasm toolchain might emit for a `wasm32` target, supported or not.
/// The unsupported rows are what `validate` explains and `link` keeps out.
///
/// The `status` column is the README's feature matrix. A proposal implemented for the
/// next, untagged release is `Since` that release: the release build's tests fail if
/// its version is older than any listed here.
pub const PROPOSALS: &[Proposal] = &[
    Proposal {
        name: "mutable-globals",
        title: "Mutable globals",
        wasm_opt: "mutable-globals",
        rustc: Some("mutable-globals"),
        status: Status::Since(Version::FIRST),
        tracking: None,
        lowering: None,
    },
    Proposal {
        name: "custom-page-sizes",
        title: "Custom page sizes",
        wasm_opt: "custom-page-sizes",
        // Chosen by the linker's `--page-size`, not an instruction set.
        rustc: None,
        status: Status::Since(Version::new(0, 2, 0)),
        tracking: Some("https://github.com/nasa/spacewasm/pull/84"),
        lowering: None,
    },
    Proposal {
        name: "sign-extension-ops",
        title: "Sign-extension operators",
        wasm_opt: "sign-ext",
        rustc: Some("sign-ext"),
        status: Status::Since(Version::new(0, 8, 0)),
        tracking: Some("https://github.com/nasa/spacewasm/pull/216"),
        lowering: Some("--signext-lowering"),
    },
    Proposal {
        name: "nontrapping-float-to-int-conversions",
        title: "Non-trapping float-to-int conversions",
        wasm_opt: "nontrapping-float-to-int",
        rustc: Some("nontrapping-fptoint"),
        status: Status::Since(Version::new(0, 8, 0)),
        tracking: Some("https://github.com/nasa/spacewasm/pull/212"),
        lowering: Some("--llvm-nontrapping-fptoint-lowering"),
    },
    Proposal {
        name: "bulk-memory-operations",
        title: "Bulk memory operations",
        wasm_opt: "bulk-memory",
        rustc: Some("bulk-memory"),
        status: Status::Planned,
        tracking: Some("https://github.com/nasa/spacewasm/issues/54"),
        lowering: Some("--llvm-memory-copy-fill-lowering"),
    },
    Proposal {
        name: "simd",
        title: "SIMD",
        wasm_opt: "simd",
        rustc: Some("simd128"),
        status: Status::UnderConsideration,
        tracking: Some("https://github.com/nasa/spacewasm/issues/187"),
        lowering: None,
    },
    Proposal {
        name: "multi-value",
        title: "Multi-value",
        wasm_opt: "multivalue",
        rustc: Some("multivalue"),
        status: Status::UnderConsideration,
        tracking: None,
        lowering: None,
    },
    Proposal {
        name: "multi-memory",
        title: "Multiple memories",
        wasm_opt: "multimemory",
        // `multimemory` is unstable, so naming it warns on every build; LLVM never
        // emits a second memory unasked.
        rustc: None,
        status: Status::UnderConsideration,
        tracking: None,
        lowering: None,
    },
    Proposal {
        name: "reference-types",
        title: "Reference types",
        wasm_opt: "reference-types",
        rustc: Some("reference-types"),
        status: Status::NotPlanned,
        tracking: None,
        lowering: None,
    },
    Proposal {
        name: "extended-const",
        title: "Extended constant expressions",
        wasm_opt: "extended-const",
        rustc: Some("extended-const"),
        status: Status::NotPlanned,
        tracking: None,
        lowering: None,
    },
    Proposal {
        name: "threads",
        title: "Threads",
        wasm_opt: "threads",
        // `atomics` is unstable, so naming it warns on every build; `wasm32v1-none`
        // never turns it on unasked.
        rustc: None,
        status: Status::NotPlanned,
        tracking: None,
        lowering: None,
    },
    Proposal {
        name: "tail-call",
        title: "Tail calls",
        wasm_opt: "tail-call",
        rustc: Some("tail-call"),
        status: Status::NotPlanned,
        tracking: None,
        lowering: None,
    },
];

pub fn supported() -> impl Iterator<Item = &'static Proposal> {
    PROPOSALS.iter().filter(|proposal| proposal.supported())
}

/// What `release` implements, for a deployment that has not upgraded.
pub fn supported_in(release: Version) -> impl Iterator<Item = &'static Proposal> {
    PROPOSALS
        .iter()
        .filter(move |proposal| proposal.supported_in(release))
}

/// `wasm-opt` flags for exactly this release's language: MVP, plus each supported
/// proposal. Starting from MVP rather than subtracting from `wasm-opt`'s default keeps a
/// proposal `wasm-opt` enables by default in a later version out.
pub fn wasm_opt_flags() -> Vec<String> {
    held_to(supported())
}

/// [`wasm_opt_flags`] for an earlier `release`.
pub fn wasm_opt_flags_for(release: Version) -> Vec<String> {
    held_to(supported_in(release))
}

fn held_to<'a>(supported: impl Iterator<Item = &'a Proposal>) -> Vec<String> {
    std::iter::once("--mvp-features".to_string())
        .chain(supported.map(|proposal| format!("--enable-{}", proposal.wasm_opt)))
        .collect()
}

/// `wasm-opt` passes that make a module built for a richer feature set loadable here:
/// lower each unsupported proposal that has a lowering, and leave every supported one
/// alone, so a module keeps every instruction it can. A proposal with no lowering is not
/// touched; `spacewasm lower` reports what survives.
pub fn lowering_flags() -> Vec<String> {
    // Read everything this table knows, so the input validates whatever it uses.
    // `--all-features` is too much: it turns on GC without reference types, which
    // `wasm-opt` rejects on its own. Multi-memory is left out too: with it on,
    // `wasm-opt` refuses to lower `memory.copy`, and a second memory cannot be lowered
    // away in any case.
    let read = PROPOSALS
        .iter()
        .filter(|proposal| proposal.name != "multi-memory")
        .map(|proposal| format!("--enable-{}", proposal.wasm_opt));
    let lower = PROPOSALS
        .iter()
        .filter(|proposal| !proposal.supported())
        .filter_map(|proposal| proposal.lowering.map(str::to_string));
    read.chain(lower).collect()
}

/// The `-Ctarget-feature` value for `rustc`: every proposal it can gate, on if supported
/// and off if not, so a newer `rustc` default cannot slip one in.
pub fn rustc_target_features() -> String {
    rustc_features(Proposal::supported)
}

/// [`rustc_target_features`] for an earlier `release`.
pub fn rustc_target_features_for(release: Version) -> String {
    rustc_features(|proposal| proposal.supported_in(release))
}

fn rustc_features(supported: impl Fn(&Proposal) -> bool) -> String {
    llvm_features(supported)
        .map(|(feature, on)| format!("{}{feature}", if on { '+' } else { '-' }))
        .collect::<Vec<_>>()
        .join(",")
}

/// `clang` flags for exactly this release's language: `-mcpu=mvp`, then every proposal
/// LLVM can gate, on if supported and off if not, as [`rustc_target_features`] does.
/// They name no `--target`, so they go with whichever `wasm32` one the build uses.
pub fn clang_flags() -> Vec<String> {
    clang(Proposal::supported)
}

/// [`clang_flags`] for an earlier `release`.
pub fn clang_flags_for(release: Version) -> Vec<String> {
    clang(|proposal| proposal.supported_in(release))
}

fn clang(supported: impl Fn(&Proposal) -> bool) -> Vec<String> {
    std::iter::once("-mcpu=mvp".to_string())
        .chain(
            llvm_features(supported)
                .map(|(feature, on)| format!("-m{}{feature}", if on { "" } else { "no-" })),
        )
        .collect()
}

/// Each LLVM target feature a proposal has, and whether `supported` turns it on. `rustc`
/// and `clang` both spell LLVM's names.
fn llvm_features(
    supported: impl Fn(&Proposal) -> bool,
) -> impl Iterator<Item = (&'static str, bool)> {
    PROPOSALS
        .iter()
        .filter_map(move |proposal| Some((proposal.rustc?, supported(proposal))))
}

/// The proposal an instruction belongs to, from its first opcode byte (and, for the
/// `0xFC` prefix, the sub-opcode after it).
pub fn proposal_of(opcode: u8, sub_opcode: Option<u32>) -> Option<&'static Proposal> {
    let name = match (opcode, sub_opcode) {
        (0xC0..=0xC4, _) => "sign-extension-ops",
        (0xFC, Some(0..=7)) => "nontrapping-float-to-int-conversions",
        (0xFC, Some(8..=17)) => "bulk-memory-operations",
        (0xFC, Some(_)) => "reference-types",
        (0xD0..=0xD2 | 0x25 | 0x26 | 0x1C, _) => "reference-types",
        (0xFD, _) => "simd",
        (0xFE, _) => "threads",
        (0x12 | 0x13, _) => "tail-call",
        _ => return None,
    };
    PROPOSALS.iter().find(|proposal| proposal.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wasm_opt_starts_from_mvp() {
        let flags = wasm_opt_flags();
        assert_eq!(flags[0], "--mvp-features");
        assert!(flags.contains(&"--enable-sign-ext".to_string()));
        assert!(flags.contains(&"--enable-nontrapping-float-to-int".to_string()));
        assert!(!flags.iter().any(|flag| flag.contains("bulk-memory")));
    }

    /// Only what this release cannot run is lowered: `wasm2mvp.sh` lowered sign-ext and
    /// saturating truncation too, after the interpreter had gained both.
    #[test]
    fn lowering_leaves_supported_proposals_alone() {
        let flags = lowering_flags();
        assert!(
            flags.contains(&"--llvm-memory-copy-fill-lowering".to_string()),
            "{flags:?}"
        );
        assert!(
            !flags.contains(&"--signext-lowering".to_string()),
            "{flags:?}"
        );
        assert!(
            !flags.contains(&"--llvm-nontrapping-fptoint-lowering".to_string()),
            "{flags:?}"
        );
    }

    #[test]
    fn rustc_turns_unsupported_ones_off() {
        let features = rustc_target_features();
        for expected in [
            "+mutable-globals",
            "+sign-ext",
            "+nontrapping-fptoint",
            "-bulk-memory",
            "-simd128",
        ] {
            assert!(
                features.split(',').any(|f| f == expected),
                "{expected}: {features}"
            );
        }
    }

    #[test]
    fn clang_spells_the_same_features_as_rustc() {
        let flags = clang_flags();
        assert_eq!(flags[0], "-mcpu=mvp");
        for expected in ["-mmutable-globals", "-msign-ext", "-mno-bulk-memory"] {
            assert!(flags.iter().any(|f| f == expected), "{expected}: {flags:?}");
        }
        let old = clang_flags_for(Version::new(0, 7, 1));
        assert!(old.iter().any(|f| f == "-mno-sign-ext"), "{old:?}");
        assert!(
            old.iter().any(|f| f == "-mno-nontrapping-fptoint"),
            "{old:?}"
        );
    }

    /// A proposal marked for the next release, then tagged as an older one, would claim
    /// more for that release than it has. Holds once the release workflow has injected
    /// the tag's version; an untagged build is ahead of every release.
    #[test]
    fn no_proposal_is_from_a_later_release() {
        let Some(this) = Version::this_release() else {
            return;
        };
        for proposal in PROPOSALS {
            if let Status::Since(since) = proposal.status {
                assert!(
                    since <= this,
                    "{} is marked >={since}, but this is {this}",
                    proposal.name
                );
            }
        }
    }

    /// 0.7.1 had neither proposal 0.8.0 added: an F Prime still flying it must not get them.
    #[test]
    fn an_earlier_release_gets_only_its_own_flags() {
        let old = Version::new(0, 7, 1);
        assert_eq!(
            wasm_opt_flags_for(old),
            [
                "--mvp-features",
                "--enable-mutable-globals",
                "--enable-custom-page-sizes"
            ]
        );
        let features = rustc_target_features_for(old);
        for expected in ["+mutable-globals", "-sign-ext", "-nontrapping-fptoint"] {
            assert!(
                features.split(',').any(|f| f == expected),
                "{expected}: {features}"
            );
        }
        assert_eq!(
            wasm_opt_flags_for(Version::new(0, 8, 0)),
            wasm_opt_flags(),
            "0.8.0 is this release's set"
        );
    }

    #[test]
    fn only_releases_this_build_can_answer_for_are_known() {
        assert!(Version::new(0, 0, 9).known().is_err());
        assert_eq!(Version::new(0, 7, 1).known(), Ok(Version::new(0, 7, 1)));
        if let Some(this) = Version::this_release() {
            let next = Version::new(this.major, this.minor + 1, 0);
            assert!(next.known().is_err());
        }
    }

    #[test]
    fn versions_read_as_tags_and_cargo_spell_them() {
        assert_eq!("0.8.0".parse(), Ok(Version::new(0, 8, 0)));
        assert_eq!("v0.7.1".parse(), Ok(Version::new(0, 7, 1)));
        assert_eq!("1.2.3-rc.1".parse(), Ok(Version::new(1, 2, 3)));
        assert!("0.8".parse::<Version>().is_err());
        assert!("latest".parse::<Version>().is_err());
        assert!(Version::new(0, 10, 0) > Version::new(0, 9, 9));
    }

    #[test]
    fn a_release_has_what_arrived_by_then() {
        let names = |release| {
            PROPOSALS
                .iter()
                .filter(|proposal| proposal.supported_in(release))
                .map(|proposal| proposal.name)
                .collect::<Vec<_>>()
        };
        assert_eq!(names(Version::new(0, 1, 0)), ["mutable-globals"]);
        assert_eq!(
            names(Version::new(0, 7, 1)),
            ["mutable-globals", "custom-page-sizes"]
        );
        assert!(names(Version::new(0, 8, 0)).contains(&"sign-extension-ops"));
    }

    #[test]
    fn every_opcode_family_names_a_listed_proposal() {
        assert_eq!(proposal_of(0xC3, None).unwrap().name, "sign-extension-ops");
        assert_eq!(
            proposal_of(0xFC, Some(5)).unwrap().name,
            "nontrapping-float-to-int-conversions"
        );
        assert_eq!(
            proposal_of(0xFC, Some(10)).unwrap().name,
            "bulk-memory-operations"
        );
        assert!(proposal_of(0x20, None).is_none(), "local.get is MVP");
    }
}
