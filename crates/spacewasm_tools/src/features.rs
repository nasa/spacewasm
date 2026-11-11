//! Which WebAssembly proposals this release of `spacewasm` implements, and how each
//! tool spells them.
//!
//! The one place that list lives: `link` hands it to `wasm-opt`, `features` prints it
//! for `rustc`, and `validate` names a proposal when a module uses one that is missing.
//! The decoder is the ground truth; the test below holds this table to it by
//! decoding one instruction from every proposal listed.

/// A post-MVP proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Proposal {
    /// Its name in the WebAssembly proposals repository.
    pub name: &'static str,
    /// `wasm-opt`'s `--enable-…` / `--disable-…` suffix.
    pub wasm_opt: &'static str,
    /// `rustc`'s `-Ctarget-feature` name, if it gates instructions there.
    pub rustc: Option<&'static str>,
    /// Whether this release of `spacewasm` implements it.
    pub supported: bool,
    /// The `wasm-opt` pass that rewrites this proposal's instructions into MVP ones, for
    /// a module built without regard to `spacewasm` (`spacewasm lower`).
    pub lowering: Option<&'static str>,
}

/// Every proposal a Wasm toolchain might emit for a `wasm32` target, supported or not.
/// The unsupported rows are what `validate` explains and `link` keeps out.
pub const PROPOSALS: &[Proposal] = &[
    Proposal {
        name: "mutable-globals",
        wasm_opt: "mutable-globals",
        rustc: Some("mutable-globals"),
        supported: true,
        lowering: None,
    },
    Proposal {
        name: "custom-page-sizes",
        wasm_opt: "custom-page-sizes",
        // Chosen by the linker's `--page-size`, not an instruction set.
        rustc: None,
        supported: true,
        lowering: None,
    },
    Proposal {
        name: "sign-extension-ops",
        wasm_opt: "sign-ext",
        rustc: Some("sign-ext"),
        supported: true,
        lowering: Some("--signext-lowering"),
    },
    Proposal {
        name: "nontrapping-float-to-int-conversions",
        wasm_opt: "nontrapping-float-to-int",
        rustc: Some("nontrapping-fptoint"),
        supported: true,
        lowering: Some("--llvm-nontrapping-fptoint-lowering"),
    },
    Proposal {
        name: "bulk-memory-operations",
        wasm_opt: "bulk-memory",
        rustc: Some("bulk-memory"),
        supported: false,
        lowering: Some("--llvm-memory-copy-fill-lowering"),
    },
    Proposal {
        name: "reference-types",
        wasm_opt: "reference-types",
        rustc: Some("reference-types"),
        supported: false,
        lowering: None,
    },
    Proposal {
        name: "multi-value",
        wasm_opt: "multivalue",
        rustc: Some("multivalue"),
        supported: false,
        lowering: None,
    },
    Proposal {
        name: "extended-const",
        wasm_opt: "extended-const",
        rustc: Some("extended-const"),
        supported: false,
        lowering: None,
    },
    Proposal {
        name: "simd",
        wasm_opt: "simd",
        rustc: Some("simd128"),
        supported: false,
        lowering: None,
    },
    Proposal {
        name: "threads",
        wasm_opt: "threads",
        // `atomics` is unstable, so naming it warns on every build; `wasm32v1-none`
        // never turns it on unasked.
        rustc: None,
        supported: false,
        lowering: None,
    },
    Proposal {
        name: "tail-call",
        wasm_opt: "tail-call",
        rustc: Some("tail-call"),
        supported: false,
        lowering: None,
    },
];

pub fn supported() -> impl Iterator<Item = &'static Proposal> {
    PROPOSALS.iter().filter(|proposal| proposal.supported)
}

/// `wasm-opt` flags for exactly this release's language: MVP, plus each supported
/// proposal. Starting from MVP rather than subtracting from `wasm-opt`'s default keeps a
/// proposal `wasm-opt` enables by default in a later version out.
pub fn wasm_opt_flags() -> Vec<String> {
    std::iter::once("--mvp-features".to_string())
        .chain(supported().map(|proposal| format!("--enable-{}", proposal.wasm_opt)))
        .collect()
}

/// `wasm-opt` passes that make a module built for a richer feature set loadable here:
/// lower each unsupported proposal that has a lowering, and leave every supported one
/// alone, so a module keeps every instruction it can. A proposal with no lowering is not
/// touched; `spacewasm lower` reports what survives.
pub fn lowering_flags() -> Vec<String> {
    // Read everything this table knows, so the input validates whatever it uses.
    // `--all-features` is too much: it turns on GC without reference types, which
    // `wasm-opt` rejects on its own.
    let read = PROPOSALS
        .iter()
        .map(|proposal| format!("--enable-{}", proposal.wasm_opt));
    let lower = PROPOSALS
        .iter()
        .filter(|proposal| !proposal.supported)
        .filter_map(|proposal| proposal.lowering.map(str::to_string));
    read.chain(lower).collect()
}

/// The `-Ctarget-feature` value for `rustc`: every proposal it can gate, on if supported
/// and off if not, so a newer `rustc` default cannot slip one in.
pub fn rustc_target_features() -> String {
    PROPOSALS
        .iter()
        .filter_map(|proposal| {
            let feature = proposal.rustc?;
            Some(format!(
                "{}{feature}",
                if proposal.supported { '+' } else { '-' }
            ))
        })
        .collect::<Vec<_>>()
        .join(",")
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
