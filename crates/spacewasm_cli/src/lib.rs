//! Ground tooling for `spacewasm`: building modules it will load, and from a flight PC
//! back to the line of source it ran.
//!
//! * [`PROPOSALS`] — the WebAssembly proposals this release implements, and
//!   [`wasm_opt_flags`] / [`rustc_target_features`] holding those tools to exactly them
//! * [`validate`] — load a module as the flight interpreter would, and name the proposal
//!   when it uses one that is missing
//!
//! A module flies without debug info, and usually after `wasm-opt`, whose optimiser
//! leaves the DWARF unusable anyway. So the linked module is split in two:
//!
//! * [`split`] — the module to uplink, and a symbol file to keep on the ground: the
//!   module as linked, with its DWARF, plus a map from each optimised byte back
//! * [`Symbols`] — a symbol file, read, resolving a byte offset in the uplinked module to
//!   file, line and inline chain
//! * [`ir_map`] — a flight PC is an IR word, not a byte offset; this replays the load to
//!   map one to the other, given the host modules the flight store had, or stand-ins
//!   from [`host_modules_for`]
//! * [`resolve_pcs`] — the two together, over a whole backtrace
//!
//! * [`link`] — all of it as Cargo's `linker`: link, optimise, split and validate as
//!   part of the build. The `spacewasm` binary is one; any binary that hands rustc's
//!   arguments to [`link`] when [`is_link`] sees them is another.
//!
//! [`validate`], [`ir_map`] and [`link`] decode through `spacewasm`, which allocates
//! through a process-global allocator that this library does not install: a binary
//! using them must, with `spacewasm::global_allocator!`. [`spacewasm`] is re-exported
//! so that it is the same release.

pub use spacewasm;

mod error;
mod features;
mod format;
mod hosts;
mod ir;
mod link;
mod offsets;
mod replay;
mod source;
mod split;
mod trace;
mod validate;

pub use error::{Error, Result};
pub use features::{PROPOSALS, Proposal, lowering_flags, rustc_target_features, wasm_opt_flags};
pub use format::{MODULE_SECTION, OFFSETS_SECTION, Symbols};
pub use hosts::host_modules_for;
pub use ir::IrMap;
pub use link::{LinkError, is_link, link, split_file};
pub use replay::{DecoderLimits, Loaded, ReplayError, ir_map, load};
pub use source::{Frame, Location};
pub use split::{Optimizer, Split, SplitError, split, strip};
pub use trace::{Resolved, resolve_offsets, resolve_pcs};
pub use validate::{Invalid, find_unsupported, validate};
