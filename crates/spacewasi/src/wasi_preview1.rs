// WASI bindings for spacewasi using the wasi-common interfaces
//
// Copyright 2026 California Institute of Technology
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// <http://www.apache.org/licenses/LICENSE-2.0>
//
// ---
// Portions of this file are derived from <https://github.com/bytecodealliance/wasmtime>
// and the wasi-common crate developed by the wasmtime community.
use futures::executor::block_on;
use spacewasm::{HostFunction, HostFunctionBreak, HostFunctionResult, HostModule, Value, vec};
use std::cell::{Cell, RefCell};
use std::ops::ControlFlow;
use std::rc::Rc;
use wasi_common::I32Exit;
use wasi_common::snapshots::preview_1::wasi_snapshot_preview1;
use wiggle::GuestMemory;

/// Pop the next argument, asserting it is an `i32`.
///
/// The interpreter validates the guest's argument count and types against each
/// binding's declared signature before the host closure runs, so a mismatch
/// here is a host-side bug (a wrong signature string in a binding), not
/// guest-reachable input; panicking is therefore appropriate.
fn next_i32(it: &mut std::slice::Iter<'_, Value>) -> i32 {
    match it.next() {
        Some(Value::I32(v)) => *v,
        other => panic!("host binding expected an i32 argument, got {other:?}"),
    }
}

/// Pop the next argument, asserting it is an `i64`. See [`next_i32`].
fn next_i64(it: &mut std::slice::Iter<'_, Value>) -> i64 {
    match it.next() {
        Some(Value::I64(v)) => *v,
        other => panic!("host binding expected an i64 argument, got {other:?}"),
    }
}

/// Centralized result-mapping policy shared by every WASI host binding.
///
/// The wiggle-generated wrappers return `Result<i32, wiggle::anyhow::Error>`:
///
/// * `Ok(errno)` — the syscall ran to completion. `errno` is the WASI status
///   returned to the guest (`0` on success, or a `wasi_snapshot_preview1` errno
///   such as `EBADF`/`EFAULT`). wiggle has *already* folded the recoverable
///   guest-memory faults into an `Errno` at this point (e.g. a borrowed pointer
///   becomes `Errno::Fault`, an invalid enum becomes `Errno::Inval`), so those
///   are handed straight back to the guest as the `i32` result.
/// * `Err(trap)` — an unrecoverable host trap that the guest cannot observe as
///   an errno: an out-of-bounds or misaligned guest pointer (which the WASI
///   spec mandates trap on), an unexpected OS-level error, etc. These abort the
///   guest via [`HostFunctionBreak::Trap`] (surfacing to the embedder as
///   `TrapReason::Host`).
///
/// This replaces the previous per-binding `.unwrap()`, which turned every host
/// trap into a panic that unwound through the entire host process. `proc_exit`
/// is the one binding that does not route through here; it interprets its
/// `I32Exit` "error" specially (see [`proc_exit_binding`]).
fn finish(result: wiggle::anyhow::Result<i32>) -> HostFunctionResult {
    match result {
        Ok(code) => ControlFlow::Continue(Some(Value::I32(code))),
        Err(_trap) => ControlFlow::Break(HostFunctionBreak::Trap),
    }
}

/// Select the argument accessor for a signature character
/// (`i` = i32, `I` = i64), matching the interpreter's signature-string alphabet.
macro_rules! next_arg {
    (i, $it:expr) => {
        next_i32($it)
    };
    (I, $it:expr) => {
        next_i64($it)
    };
}

/// Generate a `wasi_snapshot_preview1` [`HostFunction`] binding.
///
/// This folds together the three pieces that were previously copy-pasted across
/// ~45 bindings: the sound `GuestMemory::Shared` view of linear memory
/// ([`spacewasm::Memory::as_shared_cells`]), typed argument extraction, and the
/// shared error mapping ([`finish`]).
///
/// * `$ctx`     — the shared `WasiCtx` handle to clone into the closure.
/// * `$name`    — the wiggle function, also used verbatim as the exported name.
/// * `$params`  — the interpreter parameter-signature string.
/// * `$results` — the interpreter result-signature string.
/// * `[ .. ]`   — the argument types in order (`i` = i32, `I` = i64).
macro_rules! wasi_binding {
    (
        $ctx:ident,
        $name:ident,
        $params:literal,
        $results:literal,
        [ $( $ty:ident ),* $(,)? ]
    ) => {{
        let ctx = Rc::clone(&$ctx);
        HostFunction::new(
            stringify!($name),
            $params.into(),
            $results.into(),
            move |state, args| {
                #[allow(unused_mut, unused_variables)]
                let mut it = args.iter();
                finish(block_on(wasi_snapshot_preview1::$name(
                    &mut *ctx.borrow_mut(),
                    &mut GuestMemory::Shared(state.memory.as_shared_cells()),
                    $( next_arg!($ty, &mut it) ),*
                )))
            },
        )
    }};
}

/// The `proc_exit` binding.
///
/// It cannot use [`wasi_binding!`]/[`finish`] because wasi-common models
/// `proc_exit` as a diverging call: its wiggle wrapper always returns `Err` —
/// an `I32Exit(code)` for a valid status, or a generic trap for an out-of-range
/// one. Rather than calling `std::process::exit` (which would skip the caller's
/// Drop-based cleanup, such as restoring the terminal from raw mode), it records
/// the requested status in `exit_code` and traps out of the interpreter. `main`
/// observes the recorded code after the run unwinds and exits with it, so all
/// RAII guards get a chance to run. An out-of-range status carries no `I32Exit`,
/// so nothing is recorded and `main` treats it as an ordinary host trap.
fn proc_exit_binding(
    wasi_ctx: &Rc<RefCell<wasi_common::WasiCtx>>,
    exit_code: &Rc<Cell<Option<i32>>>,
) -> HostFunction {
    let ctx = Rc::clone(wasi_ctx);
    let exit_code = Rc::clone(exit_code);
    HostFunction::new("proc_exit", "i".into(), "".into(), move |state, args| {
        let mut it = args.iter();
        let status = next_i32(&mut it);
        if let Err(e) = block_on(wasi_snapshot_preview1::proc_exit(
            &mut *ctx.borrow_mut(),
            &mut GuestMemory::Shared(state.memory.as_shared_cells()),
            status,
        )) && let Some(exit) = e.downcast_ref::<I32Exit>()
        {
            exit_code.set(Some(exit.0));
        }
        ControlFlow::Break(HostFunctionBreak::Trap)
    })
}

/// Build the `wasi_snapshot_preview1` host module.
///
/// Returns the module together with a shared exit-code cell. When the guest
/// calls `proc_exit`, the binding records the exit status in this cell and traps
/// out of the interpreter (see [`proc_exit_binding`]); the caller (`main`)
/// inspects the cell after the run finishes to decide the process exit code.
pub fn make_wasi_preview1_module(
    wasi_ctx: wasi_common::WasiCtx,
) -> (HostModule, Rc<Cell<Option<i32>>>) {
    let wasi_ctx_two = Rc::new(RefCell::new(wasi_ctx));
    let exit_code: Rc<Cell<Option<i32>>> = Rc::new(Cell::new(None));

    let module = HostModule {
        name: "wasi_snapshot_preview1".into(),
        globals: vec![],
        functions: vec![
            wasi_binding!(wasi_ctx_two, args_get, "ii", "i", [i, i]),
            wasi_binding!(wasi_ctx_two, args_sizes_get, "ii", "i", [i, i]),
            wasi_binding!(wasi_ctx_two, environ_get, "ii", "i", [i, i]),
            wasi_binding!(wasi_ctx_two, environ_sizes_get, "ii", "i", [i, i]),
            wasi_binding!(wasi_ctx_two, clock_res_get, "ii", "i", [i, i]),
            wasi_binding!(wasi_ctx_two, clock_time_get, "iIi", "i", [i, I, i]),
            wasi_binding!(wasi_ctx_two, fd_advise, "iIIi", "i", [i, I, I, i]),
            wasi_binding!(wasi_ctx_two, fd_allocate, "iII", "i", [i, I, I]),
            wasi_binding!(wasi_ctx_two, fd_close, "i", "i", [i]),
            wasi_binding!(wasi_ctx_two, fd_datasync, "i", "i", [i]),
            wasi_binding!(wasi_ctx_two, fd_fdstat_get, "ii", "i", [i, i]),
            wasi_binding!(wasi_ctx_two, fd_fdstat_set_flags, "ii", "i", [i, i]),
            wasi_binding!(wasi_ctx_two, fd_fdstat_set_rights, "iII", "i", [i, I, I]),
            wasi_binding!(wasi_ctx_two, fd_filestat_get, "ii", "i", [i, i]),
            wasi_binding!(wasi_ctx_two, fd_filestat_set_size, "iI", "i", [i, I]),
            wasi_binding!(
                wasi_ctx_two,
                fd_filestat_set_times,
                "iIIi",
                "i",
                [i, I, I, i]
            ),
            wasi_binding!(wasi_ctx_two, fd_pread, "iiiIi", "i", [i, i, i, I, i]),
            wasi_binding!(wasi_ctx_two, fd_prestat_get, "ii", "i", [i, i]),
            wasi_binding!(wasi_ctx_two, fd_prestat_dir_name, "iii", "i", [i, i, i]),
            wasi_binding!(wasi_ctx_two, fd_pwrite, "iiiIi", "i", [i, i, i, I, i]),
            wasi_binding!(wasi_ctx_two, fd_read, "iiii", "i", [i, i, i, i]),
            wasi_binding!(wasi_ctx_two, fd_readdir, "iiiIi", "i", [i, i, i, I, i]),
            wasi_binding!(wasi_ctx_two, fd_seek, "iIii", "i", [i, I, i, i]),
            wasi_binding!(wasi_ctx_two, fd_sync, "i", "i", [i]),
            wasi_binding!(wasi_ctx_two, fd_tell, "ii", "i", [i, i]),
            wasi_binding!(wasi_ctx_two, fd_write, "iiii", "i", [i, i, i, i]),
            wasi_binding!(wasi_ctx_two, path_create_directory, "iii", "i", [i, i, i]),
            wasi_binding!(
                wasi_ctx_two,
                path_filestat_get,
                "iiiii",
                "i",
                [i, i, i, i, i]
            ),
            wasi_binding!(
                wasi_ctx_two,
                path_filestat_set_times,
                "iiiiIIi",
                "i",
                [i, i, i, i, I, I, i]
            ),
            wasi_binding!(
                wasi_ctx_two,
                path_link,
                "iiiiiii",
                "i",
                [i, i, i, i, i, i, i]
            ),
            wasi_binding!(
                wasi_ctx_two,
                path_open,
                "iiiiiIIii",
                "i",
                [i, i, i, i, i, I, I, i, i]
            ),
            wasi_binding!(
                wasi_ctx_two,
                path_readlink,
                "iiiiii",
                "i",
                [i, i, i, i, i, i]
            ),
            wasi_binding!(wasi_ctx_two, path_remove_directory, "iii", "i", [i, i, i]),
            wasi_binding!(wasi_ctx_two, path_rename, "iiiiii", "i", [i, i, i, i, i, i]),
            wasi_binding!(wasi_ctx_two, path_symlink, "iiiii", "i", [i, i, i, i, i]),
            wasi_binding!(wasi_ctx_two, path_unlink_file, "iii", "i", [i, i, i]),
            wasi_binding!(wasi_ctx_two, poll_oneoff, "iiii", "i", [i, i, i, i]),
            proc_exit_binding(&wasi_ctx_two, &exit_code),
            wasi_binding!(wasi_ctx_two, proc_raise, "i", "i", [i]),
            wasi_binding!(wasi_ctx_two, random_get, "ii", "i", [i, i]),
            wasi_binding!(wasi_ctx_two, sched_yield, "", "i", []),
            wasi_binding!(wasi_ctx_two, sock_accept, "iii", "i", [i, i, i]),
            wasi_binding!(wasi_ctx_two, sock_recv, "iiiiii", "i", [i, i, i, i, i, i]),
            wasi_binding!(wasi_ctx_two, sock_send, "iiiii", "i", [i, i, i, i, i]),
            wasi_binding!(wasi_ctx_two, sock_shutdown, "ii", "i", [i, i]),
        ],
        memory: spacewasm::Vec::zero(),
        table: spacewasm::Vec::zero(),
    };

    (module, exit_code)
}
