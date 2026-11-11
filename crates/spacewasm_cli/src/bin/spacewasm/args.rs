//! The command line: every flag, and the only place clap appears.

use clap::{Args, Parser, Subcommand};
use spacewasm::CompilerOptions;
use spacewasm_cli::DecoderLimits;
use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "spacewasm",
    version,
    about = "Ground tooling for the spacewasm interpreter: link, validate, run and trace \
             modules, split debug info into a symbol file, and resolve backtraces"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// A Cargo linker: `rust-lld`, then `wasm-opt` held to this release's features.
    ///
    /// Set this binary as `linker` for the Wasm target; it recognises rustc's
    /// `-flavor wasm` and needs no subcommand there. Every argument goes to
    /// `rust-lld`. An optimised link (`-O1` and up) is then run through `wasm-opt` with
    /// exactly the features `spacewasm features` lists, its debug info split into
    /// `<bin>.sym`, and the result validated. Settings come from `SPACEWASM_*`
    /// variables, which Cargo's `[env]` table can set; see the README.
    #[command(disable_help_flag = true)]
    Link {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Split a module with debug info into the module to uplink and its symbol file.
    Split(SplitArgs),
    /// Load modules exactly as the flight interpreter would.
    Validate(ValidateArgs),
    /// Resolve a backtrace, or module offsets, against a symbol file.
    Backtrace(BacktraceArgs),
    /// The proposals this release implements, as `wasm-opt` and `rustc` flags.
    Features(FeaturesArgs),
    /// Run a WASI 0.1 (`wasip1`) module.
    Run(RunArgs),
    /// Run every exported function, printing the last instructions each executed.
    Trace(TraceArgs),
    /// Rewrite a module built for more proposals than this release has, e.g. by the WASI
    /// SDK, into one it loads.
    Lower(LowerArgs),
}

#[derive(Args)]
pub struct LowerArgs {
    /// The module.
    pub input: PathBuf,

    /// Where to write the result. Defaults to the input, replaced.
    #[arg(long, short)]
    pub output: Option<PathBuf>,

    /// The `wasm-opt` to run.
    #[arg(long, value_name = "PATH", default_value = "wasm-opt")]
    pub wasm_opt: PathBuf,
}

#[derive(Args)]
pub struct RunArgs {
    /// Mount the current working directory as the root directory (/) in the guest.
    #[arg(long)]
    pub cwd_is_root: bool,

    /// Override argv[0].
    #[arg(long, value_name = "ARGV0")]
    pub argv0: Option<String>,

    /// Mount a host directory, at the same path or at `GUEST_DIR`. Repeatable.
    #[arg(short, long, value_name = "HOST_DIR[::GUEST_DIR]")]
    pub dir: Vec<String>,

    /// Set an environment variable; `KEY` alone copies it from this process. Repeatable.
    #[arg(short, long, value_name = "KEY[=VALUE]")]
    pub env: Vec<String>,

    /// Inherit this process's whole environment.
    #[arg(long)]
    pub inherit_env: bool,

    /// Put the terminal in raw mode for the run.
    #[arg(long)]
    pub raw_tty: bool,

    /// The function to run.
    #[arg(long, default_value = "_start")]
    pub entry: String,

    /// The module.
    pub file: PathBuf,

    /// Arguments for the module.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,
}

#[derive(Args)]
pub struct TraceArgs {
    /// The module. Read from stdin if omitted, e.g. piped from `seed_to_wasm --stdout`.
    pub file: Option<PathBuf>,

    /// How many of the last instructions to print, per function.
    #[arg(long, default_value = "200")]
    pub limit: usize,

    /// Instructions each function may run before it is stopped.
    #[arg(long, default_value = "10000")]
    pub fuel: usize,
}

#[derive(Args)]
pub struct SplitArgs {
    /// The module as linked, with its debug info.
    pub input: PathBuf,

    /// Where to write the module to uplink. Defaults to the input, replaced.
    #[arg(long, short)]
    pub output: Option<PathBuf>,

    /// Where to write the symbol file. Defaults to the output with a `.sym` extension.
    #[arg(long)]
    pub symbols: Option<PathBuf>,

    /// Run `wasm-opt` with this release's feature set, then these extra arguments, e.g.
    /// `--optimize=-Os`. Without it the module is only stripped.
    #[arg(
        long,
        value_name = "ARGS",
        value_delimiter = ',',
        allow_hyphen_values = true,
        num_args = 0..
    )]
    pub optimize: Option<Vec<String>>,

    /// The `wasm-opt` to run.
    #[arg(long, value_name = "PATH", default_value = "wasm-opt")]
    pub wasm_opt: PathBuf,
}

#[derive(Args)]
pub struct ValidateArgs {
    /// The modules to load; a symbol file validates the module it describes.
    #[arg(required = true)]
    pub modules: Vec<PathBuf>,

    #[command(flatten)]
    pub compiler: CompilerArgs,
}

#[derive(Args)]
#[command(group(clap::ArgGroup::new("position").required(true).args(["pc", "offset"])))]
pub struct BacktraceArgs {
    /// The symbol file.
    pub symbols: PathBuf,

    /// Interpreter PCs, innermost first, as `spacewasm_backtrace` reports them:
    /// comma-separated or repeated. Hex with `0x`.
    #[arg(long, value_parser = number::<u32>, value_delimiter = ',', num_args = 1..)]
    pub pc: Vec<u32>,

    /// Byte offsets into the uplinked module instead, innermost first.
    #[arg(long, value_parser = number::<u64>, value_delimiter = ',', num_args = 1..)]
    pub offset: Vec<u64>,

    /// The module that flew, checked against the one the symbols were built for.
    #[arg(long, value_name = "PATH")]
    pub module: Option<PathBuf>,

    /// Where the module's code starts in the flight engine's IR, if other modules were
    /// loaded into its store before it. Subtracted from every PC.
    #[arg(long, value_parser = number::<u32>, default_value = "0")]
    pub base: u32,

    #[command(flatten)]
    pub compiler: CompilerArgs,
}

#[derive(Args)]
pub struct FeaturesArgs {
    /// Print only the `wasm-opt` flags, one line.
    #[arg(long, conflicts_with = "rustc")]
    pub wasm_opt: bool,

    /// Print only the `-Ctarget-feature` value for `rustc`.
    #[arg(long)]
    pub rustc: bool,
}

/// The flight `CompilerOptions`: what decides whether a module loads, and the IR layout a
/// PC is an address in.
#[derive(Args)]
pub struct CompilerArgs {
    /// `CompilerOptions::max_code_pages`.
    #[arg(long, default_value = "256")]
    pub max_code_pages: usize,

    /// `CompilerOptions::max_backpatch_iterations`; 0 for unlimited.
    #[arg(long, default_value = "0")]
    pub max_backpatch_iterations: u32,

    /// `CompilerOptions::allow_memory_grow`.
    #[arg(long, default_value = "true", action = clap::ArgAction::Set)]
    pub allow_memory_grow: bool,

    /// The decoder's `MAX_CONTROL_FRAMES` / `MAX_STACK_DEPTH`, as `FRAMES/DEPTH`.
    #[arg(long, default_value = "64/256", value_parser = limits)]
    pub decoder_limits: DecoderLimits,
}

impl CompilerArgs {
    pub fn options(&self) -> CompilerOptions {
        CompilerOptions::new(
            self.allow_memory_grow,
            (self.max_backpatch_iterations != 0).then_some(self.max_backpatch_iterations),
            self.max_code_pages,
        )
    }
}

fn number<T: TryFrom<u64>>(text: &str) -> Result<T, String> {
    let parsed = match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => u64::from_str_radix(hex, 16),
        None => text.parse(),
    }
    .map_err(|err| format!("`{text}` is not a number: {err}"))?;
    T::try_from(parsed).map_err(|_| format!("`{text}` is out of range"))
}

fn limits(text: &str) -> Result<DecoderLimits, String> {
    let (frames, depth) = text
        .split_once('/')
        .ok_or_else(|| format!("`{text}` is not FRAMES/DEPTH"))?;
    Ok(DecoderLimits {
        max_control_frames: number(frames)?,
        max_stack_depth: number(depth)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_is_internally_consistent() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    #[test]
    fn pcs_take_a_list_or_repeat() {
        let pcs = |args: &[&str]| match Cli::try_parse_from(args).unwrap().command {
            Command::Backtrace(backtrace) => backtrace.pc,
            _ => unreachable!(),
        };
        assert_eq!(
            pcs(&["s", "backtrace", "a.sym", "--pc", "0x10,4"]),
            [0x10, 4]
        );
        assert_eq!(
            pcs(&["s", "backtrace", "a.sym", "--pc", "1", "--pc", "2"]),
            [1, 2]
        );
        assert!(Cli::try_parse_from(["s", "backtrace", "a.sym"]).is_err());
    }

    /// `spacewasi`'s flags, unchanged, and arguments after the module go to it.
    #[test]
    fn run_keeps_spacewasi_flags() {
        let Command::Run(run) = Cli::try_parse_from([
            "s", "run", "--dir", "a::/", "-e", "K=v", "--argv0", "x", "m.wasm", "--flag", "y",
        ])
        .unwrap()
        .command
        else {
            unreachable!()
        };
        assert_eq!(run.dir, ["a::/"]);
        assert_eq!(run.env, ["K=v"]);
        assert_eq!(run.argv0.as_deref(), Some("x"));
        assert_eq!(run.file, PathBuf::from("m.wasm"));
        assert_eq!(run.args, ["--flag", "y"]);
    }

    #[test]
    fn trace_reads_stdin_without_a_file() {
        let Command::Trace(trace) = Cli::try_parse_from(["s", "trace", "--limit", "50"])
            .unwrap()
            .command
        else {
            unreachable!()
        };
        assert_eq!(trace.file, None);
        assert_eq!(trace.limit, 50);
    }

    #[test]
    fn optimize_takes_extra_wasm_opt_arguments() {
        let optimize = |args: &[&str]| match Cli::try_parse_from(args).unwrap().command {
            Command::Split(split) => split.optimize,
            _ => unreachable!(),
        };
        assert_eq!(optimize(&["s", "split", "a.wasm"]), None);
        assert_eq!(
            optimize(&["s", "split", "a.wasm", "--optimize=-Os,--converge"]),
            Some(vec!["-Os".to_string(), "--converge".to_string()])
        );
    }
}
