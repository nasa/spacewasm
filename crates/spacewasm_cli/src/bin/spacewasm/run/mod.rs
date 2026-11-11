//! `spacewasm run`: execute a WASI 0.1 (`wasip1`) module on the interpreter.
//!
//! What `spacewasi` was. `wasi-common` implements the system calls; [`wasi_preview1`]
//! binds each into a `HostModule` the interpreter calls.

mod wasi_preview1;

use crate::Outcome;
use crate::args::RunArgs;
use crate::load::load;
use spacewasm::{Interpreter, InterpreterResult, InterpreterRunner, InvokeError, TrapReason};
use std::process::ExitCode;
use wasi_common::sync::{Dir, WasiCtxBuilder, ambient_authority};

/// Operand stack for a WASI program, in 32-bit words: generous, as for any ground run.
const STACK_WORDS: usize = 0x100_000;

/// What `main` exits with: a guest's `proc_exit` status passes straight through.
pub enum Ran {
    Exited(ExitCode),
    Outcome(Outcome),
}

/// Restores the terminal from raw mode when dropped.
struct RawTtyGuard;

impl Drop for RawTtyGuard {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

pub fn run(args: &RunArgs) -> Ran {
    match execute(args) {
        Ok(ran) => ran,
        Err(err) => Ran::Outcome(Err(err)),
    }
}

fn execute(args: &RunArgs) -> Result<Ran, Box<dyn std::error::Error>> {
    let bytes = std::fs::read(&args.file)
        .map_err(|err| format!("could not read {}: {err}", args.file.display()))?;

    let (wasi, exit_code) = wasi_preview1::make_wasi_preview1_module(context(args)?.build());
    let mut loaded = load(&bytes, spacewasm::vec![wasi], STACK_WORDS)?;

    let _tty = match args.raw_tty {
        true => {
            crossterm::terminal::enable_raw_mode()
                .map_err(|err| format!("could not enable raw terminal mode: {err}"))?;
            Some(RawTtyGuard)
        }
        false => None,
    };

    let started = loaded.start(usize::MAX);
    // A guest may call `proc_exit` from its start function; honour that before treating
    // the host trap it leaves as a failure.
    if let Some(code) = exit_code.get() {
        return Ok(Ran::Exited(ExitCode::from(code as u8)));
    }
    if started != InterpreterResult::Finished {
        return Err(format!("the start function did not complete: {started:?}").into());
    }

    let entry = loaded.export(&args.entry).ok_or_else(|| {
        format!(
            "the module does not export a `{}` function. A WASI command exports `_start`",
            args.entry
        )
    })?;
    let mut result = match loaded.engine.invoke(entry, &[]) {
        Ok(()) => InterpreterResult::OutOfFuel,
        Err(InvokeError::StackOverflow) => InterpreterResult::Trap(TrapReason::StackOverflow),
        Err(err) => return Err(format!("could not invoke `{}`: {err:?}", args.entry).into()),
    };
    while result == InterpreterResult::OutOfFuel {
        result = Interpreter.run(loaded.code.pages(), &mut loaded.engine, usize::MAX);
    }

    if let Some(code) = exit_code.get() {
        return Ok(Ran::Exited(ExitCode::from(code as u8)));
    }
    match result {
        InterpreterResult::Finished => Ok(Ran::Outcome(Ok(true))),
        other => Err(format!("interpreter failed: {other:?}").into()),
    }
}

/// The WASI context the command line describes: arguments, environment, mounts.
fn context(args: &RunArgs) -> Result<WasiCtxBuilder, Box<dyn std::error::Error>> {
    let mut builder = WasiCtxBuilder::new();

    let argv0 = args
        .argv0
        .clone()
        .unwrap_or_else(|| args.file.display().to_string());
    builder
        .arg(&argv0)
        .map_err(|err| format!("could not set argv[0]: {err}"))?;
    builder
        .args(&args.args)
        .map_err(|err| format!("could not set the arguments: {err}"))?;

    if args.inherit_env {
        builder
            .inherit_env()
            .map_err(|err| format!("could not inherit the environment: {err}"))?;
    }
    for variable in &args.env {
        let (key, value) = match variable.split_once('=') {
            Some((key, value)) => (key.to_string(), value.to_string()),
            None => (
                variable.clone(),
                std::env::var(variable).unwrap_or_default(),
            ),
        };
        builder
            .env(&key, &value)
            .map_err(|err| format!("could not set {key}: {err}"))?;
    }

    builder.inherit_stdio();

    let mut mounts: Vec<(String, String)> = args
        .dir
        .iter()
        .map(|dir| match dir.split_once("::") {
            Some((host, guest)) => (host.to_string(), guest.to_string()),
            None => (dir.clone(), dir.clone()),
        })
        .collect();
    if args.cwd_is_root {
        mounts.push((".".into(), "/".into()));
    }
    for (host, guest) in mounts {
        let dir = Dir::open_ambient_dir(&host, ambient_authority())
            .map_err(|err| format!("could not open host directory {host}: {err}"))?;
        builder
            .preopened_dir(dir, &guest)
            .map_err(|err| format!("could not mount {host} at {guest}: {err}"))?;
    }
    Ok(builder)
}
