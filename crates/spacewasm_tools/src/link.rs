//! The Cargo linker: `rust-lld`, then `wasm-opt` held to this release's features, then
//! the debug info split into a symbol file and the result validated.
//!
//! Any binary can be the linker by handing [`link`] its arguments whenever
//! [`is_link`] says rustc invoked it as one, so a tool that wraps this crate does not
//! also need `spacewasm` installed.
//!
//! Every argument belongs to `rust-lld`, so the link's own settings are environment
//! variables, which Cargo's `[env]` table sets:
//!
//! * `SPACEWASM_OPT` — `wasm-opt` optimisation flags, e.g. `-Os --converge`. The feature
//!   set is not among them: it is always exactly this release's ([`wasm_opt_flags`]).
//!   Unset, `-Os`. `none` links without optimising.
//! * `SPACEWASM_WASM_OPT` — the `wasm-opt` to run. Default `wasm-opt`.
//! * `SPACEWASM_WASM_OPT_REQUIRED=1` — fail when `wasm-opt` is missing, instead of warning.
//! * `SPACEWASM_LINKER` — the linker to run first. Default `rust-lld`.
//! * `SPACEWASM_VALIDATE=0` — skip validating the result.
//! * `SPACEWASM_DECODER_LIMITS` — the deployment's `DecoderStack` lengths to validate
//!   against, as `FRAMES/DEPTH`. Default `64/256`.
//!
//! Validating decodes through `spacewasm`, so the binary must have installed its
//! allocator (`spacewasm::global_allocator!`).
//!
//! [`wasm_opt_flags`]: crate::wasm_opt_flags

use crate::hosts::host_modules_for;
use crate::replay::{DecoderLimits, ReplayError};
use crate::split::{Optimizer, SplitError, split};
use crate::validate::{Invalid, validate};
use spacewasm::CompilerOptions;
use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

const ENV_OPT: &str = "SPACEWASM_OPT";
const ENV_WASM_OPT: &str = "SPACEWASM_WASM_OPT";
const ENV_WASM_OPT_REQUIRED: &str = "SPACEWASM_WASM_OPT_REQUIRED";
const ENV_LINKER: &str = "SPACEWASM_LINKER";
const ENV_VALIDATE: &str = "SPACEWASM_VALIDATE";
const ENV_DECODER_LIMITS: &str = "SPACEWASM_DECODER_LIMITS";

/// Code pages a link-time check allows: generous, since the deployment's real budget is
/// not known here, but small enough that the page table itself allocates. The table is a
/// pointer per page, out of the same 64 KiB pages as everything else.
const LINK_CODE_PAGES: usize = 4096;

/// Why a link failed, beyond the linker itself failing.
#[derive(Debug)]
pub enum LinkError {
    /// The linker or `wasm-opt` could not be run as configured.
    Setup(String),
    /// A file the link reads or writes.
    Io(PathBuf, std::io::Error),
    Split(PathBuf, SplitError),
    /// The module uses an instruction this release does not implement.
    Unsupported(PathBuf, Invalid),
    /// The module's imports could not be stood in for.
    Hosts(PathBuf, ReplayError),
}

impl fmt::Display for LinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LinkError::Setup(why) => f.write_str(why),
            LinkError::Io(path, err) => write!(f, "{}: {err}", path.display()),
            LinkError::Split(path, err) => write!(f, "{}: {err}", path.display()),
            LinkError::Unsupported(path, err) => write!(f, "{}: {err}", path.display()),
            LinkError::Hosts(path, err) => write!(f, "{}: {err}", path.display()),
        }
    }
}

impl std::error::Error for LinkError {}

/// Whether `args`, without the program name, are rustc invoking a linker: its first
/// argument to a wasm linker is always `-flavor`.
pub fn is_link(args: &[OsString]) -> bool {
    args.first().is_some_and(|first| first == "-flavor")
}

/// Link, as Cargo's `linker`. `args` are rustc's, without the program name.
///
/// `Ok(false)`: the linker ran and failed, and has said why.
pub fn link(args: &[OsString]) -> Result<bool, LinkError> {
    // `rust-lld` is on the PATH rustc gives its linker, not the ambient one.
    let linker = std::env::var_os(ENV_LINKER).unwrap_or_else(|| "rust-lld".into());
    let status = Command::new(&linker).args(args).status().map_err(|err| {
        LinkError::Setup(format!(
            "could not run `{}`: {err}",
            linker.to_string_lossy()
        ))
    })?;
    if !status.success() {
        return Ok(false);
    }

    let Some(output) = output_of(args) else {
        // Nothing to split: some invocation that writes no module, e.g. `--version`.
        return Ok(true);
    };

    // An unoptimised link is for a debugger, which wants the DWARF where it is.
    if !optimized(args) {
        return Ok(true);
    }

    let optimizer = wasm_opt(&output)?;

    // Beside the `<bin>.wasm` cargo copies out of `deps/`, under the bin's own name.
    let directory = output.parent().unwrap_or(Path::new("."));
    let directory = match directory.file_name() {
        Some(name) if name == "deps" => directory.parent().unwrap_or(directory),
        _ => directory,
    };
    let stem = std::env::var("CARGO_BIN_NAME")
        .ok()
        .or_else(|| output.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "module".into());
    let symbols = directory.join(format!("{stem}.sym"));

    let module = split_file(&output, &output, &symbols, optimizer.as_ref())?;

    if std::env::var(ENV_VALIDATE).is_ok_and(|validate| validate == "0") {
        return Ok(true);
    }
    // Against stand-in hosts and the decoder limits given, if any: what can be known at
    // link time. The rest of a deployment's settings are `spacewasm validate`'s to check.
    let limits = match std::env::var(ENV_DECODER_LIMITS) {
        Ok(text) => text
            .parse::<DecoderLimits>()
            .map_err(|why| LinkError::Setup(format!("{ENV_DECODER_LIMITS}: {why}")))?,
        Err(_) => DecoderLimits::default(),
    };
    let hosts = host_modules_for(&module).map_err(|err| LinkError::Hosts(output.clone(), err))?;
    let options = CompilerOptions::new(true, None, LINK_CODE_PAGES);
    match validate(&module, options, hosts, limits) {
        Ok(_) => Ok(true),
        Err(err @ Invalid::Unsupported { .. }) => Err(LinkError::Unsupported(output, err)),
        // A limit or a host a link cannot know about: worth seeing, not worth failing on.
        Err(err @ Invalid::Rejected(_)) => {
            eprintln!(
                "spacewasm: {} may not load on board: {err}. Check it against the \
                 deployment with `spacewasm validate`",
                output.display()
            );
            Ok(true)
        }
    }
}

/// [`split`] from file to files: `input` into the module at `output` and the symbol file
/// at `symbols`. `output` may be `input`. Returns the module written.
pub fn split_file(
    input: &Path,
    output: &Path,
    symbols: &Path,
    optimizer: Option<&Optimizer>,
) -> Result<Vec<u8>, LinkError> {
    let linked = std::fs::read(input).map_err(|err| LinkError::Io(input.into(), err))?;
    let scratch = Scratch::new(output).map_err(|err| LinkError::Io(output.into(), err))?;
    let split =
        split(&linked, optimizer, &scratch.0).map_err(|err| LinkError::Split(input.into(), err))?;
    std::fs::write(output, &split.module).map_err(|err| LinkError::Io(output.into(), err))?;
    std::fs::write(symbols, &split.symbols).map_err(|err| LinkError::Io(symbols.into(), err))?;
    Ok(split.module)
}

/// A directory beside `output` for `wasm-opt`'s files, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(output: &Path) -> std::io::Result<Scratch> {
        let name = output
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let dir = output
            .parent()
            .unwrap_or(Path::new("."))
            .join(format!(".{name}.spacewasm-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        Ok(Scratch(dir))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The `wasm-opt` to run, if any.
fn wasm_opt(output: &Path) -> Result<Option<Optimizer>, LinkError> {
    let extra = std::env::var(ENV_OPT).unwrap_or_else(|_| "-Os".into());
    if extra.trim() == "none" {
        return Ok(None);
    }
    let program = std::env::var_os(ENV_WASM_OPT)
        .map(PathBuf::from)
        .unwrap_or_else(|| "wasm-opt".into());

    if !runs(&program) {
        let shown = program.display();
        if std::env::var(ENV_WASM_OPT_REQUIRED).is_ok_and(|required| required == "1") {
            return Err(LinkError::Setup(format!(
                "`{shown}` not found, and {ENV_WASM_OPT_REQUIRED}=1. It ships with binaryen: \
                 https://github.com/WebAssembly/binaryen"
            )));
        }
        // Not fatal: the module is linked and valid, just larger than it needs to be.
        eprintln!(
            "spacewasm: `{shown}` not found, leaving {} unoptimised. It ships with binaryen: \
             https://github.com/WebAssembly/binaryen",
            output.display()
        );
        return Ok(None);
    }

    let extra: Vec<String> = extra.split_whitespace().map(str::to_string).collect();
    if let Some(flag) = extra.iter().find(|flag| is_feature_flag(flag)) {
        return Err(LinkError::Setup(format!(
            "{ENV_OPT} sets `{flag}`, but the feature set is this release's to decide \
             (`spacewasm features`); leave it out"
        )));
    }
    Ok(Some(Optimizer::held(program, &extra)))
}

/// A `wasm-opt` flag that changes the feature set rather than the optimisation.
fn is_feature_flag(flag: &str) -> bool {
    flag.starts_with("--enable-")
        || flag.starts_with("--disable-")
        || matches!(
            flag,
            "--mvp-features" | "-mvp" | "--all-features" | "-all" | "--detect-features"
        )
}

/// Whether `program` can be started at all.
fn runs(program: &Path) -> bool {
    Command::new(program)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

fn output_of(args: &[OsString]) -> Option<PathBuf> {
    args.windows(2)
        .rev()
        .find(|pair| pair[0] == "-o")
        .map(|pair| PathBuf::from(&pair[1]))
}

/// Whether rustc asked for an optimised link: `-O1` and up, which release profiles get.
fn optimized(args: &[OsString]) -> bool {
    args.iter().rev().find_map(|arg| {
        let arg = arg.to_str()?;
        arg.strip_prefix("-O").map(|level| level != "0")
    }) == Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn finds_the_output_and_profile() {
        let release = args(&["-flavor", "wasm", "x.o", "-o", "out/deps/seq-1.wasm", "-O2"]);
        assert!(is_link(&release));
        assert_eq!(
            output_of(&release),
            Some(PathBuf::from("out/deps/seq-1.wasm"))
        );
        assert!(optimized(&release));

        let debug = args(&["-o", "a.wasm", "-O0"]);
        assert!(!is_link(&debug));
        assert!(!optimized(&debug));
        assert!(!optimized(&args(&["-o", "a.wasm"])));
    }

    #[test]
    fn feature_flags_are_recognised() {
        assert!(is_feature_flag("--enable-bulk-memory"));
        assert!(is_feature_flag("--mvp-features"));
        assert!(!is_feature_flag("-Os"));
        assert!(!is_feature_flag("--converge"));
    }
}
