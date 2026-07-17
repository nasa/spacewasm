/// WASI bindings for spacewasi using the wasi-common interfaces
///
/// Copyright 2026 California Institute of Technology
///
/// Licensed under the Apache License, Version 2.0 (the "License");
/// you may not use this file except in compliance with the License.
/// You may obtain a copy of the License at
///
/// <http://www.apache.org/licenses/LICENSE-2.0>
///
/// ---
/// Portions of this file are derived from <https://github.com/bytecodealliance/wasmtime>
/// and the wasi-common crate developed by the wasmtime community.
use futures::executor::block_on;
use spacewasm::{HostFunction, HostModule, Value, vec};
use std::cell::RefCell;
use std::ops::ControlFlow;
use std::rc::Rc;
use wasi_common::snapshots::preview_1::wasi_snapshot_preview1;
use wiggle::GuestMemory;

pub fn make_wasi_preview1_module(wasi_ctx: wasi_common::WasiCtx) -> HostModule {
    let wasi_ctx_two = Rc::new(RefCell::new(wasi_ctx));

    HostModule {
        name: "wasi_snapshot_preview1",
        globals: vec![],
        functions: vec![
            HostFunction::new("args_get", "ii".into(), "i".into(), {
                let wasi_ctx_args_get = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a1)) = args.get(1) else {
                        panic!("expected i32");
                    };

                    // eprintln!("args_get({a0}, {a1})");

                    let code = block_on(wasi_snapshot_preview1::args_get(
                        &mut *wasi_ctx_args_get.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                    ))
                    .unwrap();

                    // eprintln!("\t-->{code}");

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("args_sizes_get", "ii".into(), "i".into(), {
                let wasi_ctx_args_sizes_get = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a1)) = args.get(1) else {
                        panic!("expected i32");
                    };

                    // eprintln!("args_sizes_get({a0}, {a1})");

                    let code = block_on(wasi_snapshot_preview1::args_sizes_get(
                        &mut *wasi_ctx_args_sizes_get.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("environ_get", "ii".into(), "i".into(), {
                let wasi_ctx_environ_get = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a1)) = args.get(1) else {
                        panic!("expected i32");
                    };

                    // eprintln!("environ_get({a0}, {a1})");

                    let code = block_on(wasi_snapshot_preview1::environ_get(
                        &mut *wasi_ctx_environ_get.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("environ_sizes_get", "ii".into(), "i".into(), {
                let wasi_ctx_environ_sizes_get = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a1)) = args.get(1) else {
                        panic!("expected i32");
                    };

                    // eprintln!("environ_sizes_get({a0}, {a1})");

                    let code = block_on(wasi_snapshot_preview1::environ_sizes_get(
                        &mut *wasi_ctx_environ_sizes_get.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("clock_res_get", "ii".into(), "i".into(), {
                let wasi_ctx_clock_res_get = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a1)) = args.get(1) else {
                        panic!("expected i32");
                    };

                    // eprintln!("clock_res_get({a0}, {a1})");

                    let code = block_on(wasi_snapshot_preview1::clock_res_get(
                        &mut *wasi_ctx_clock_res_get.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("clock_time_get", "iIi".into(), "i".into(), {
                let wasi_ctx_clock_time_get = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I64(a1)) = args.get(1) else {
                        panic!("expected i64");
                    };
                    let Some(Value::I32(a2)) = args.get(2) else {
                        panic!("expected i32");
                    };

                    // eprintln!("clock_time_get({a0}, {a1}, {a2})");

                    let code = block_on(wasi_snapshot_preview1::clock_time_get(
                        &mut *wasi_ctx_clock_time_get.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                        *a2,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("fd_advise", "iIIi".into(), "i".into(), {
                let wasi_ctx_fd_advise = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I64(a1)) = args.get(1) else {
                        panic!("expected i64");
                    };
                    let Some(Value::I64(a2)) = args.get(2) else {
                        panic!("expected i64");
                    };
                    let Some(Value::I32(a3)) = args.get(3) else {
                        panic!("expected i32");
                    };

                    // eprintln!("fd_advise({a0}, {a1}, {a2}, {a3})");

                    let code = block_on(wasi_snapshot_preview1::fd_advise(
                        &mut *wasi_ctx_fd_advise.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                        *a2,
                        *a3,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("fd_allocate", "iII".into(), "i".into(), {
                let wasi_ctx_fd_allocate = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I64(a1)) = args.get(1) else {
                        panic!("expected i64");
                    };
                    let Some(Value::I64(a2)) = args.get(2) else {
                        panic!("expected i64");
                    };

                    // eprintln!("fd_allocate({a0}, {a1}, {a2})");

                    let code = block_on(wasi_snapshot_preview1::fd_allocate(
                        &mut *wasi_ctx_fd_allocate.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                        *a2,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("fd_close", "i".into(), "i".into(), {
                let wasi_ctx_fd_close = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };

                    // eprintln!("fd_close({a0})");

                    let code = block_on(wasi_snapshot_preview1::fd_close(
                        &mut *wasi_ctx_fd_close.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("fd_datasync", "i".into(), "i".into(), {
                let wasi_ctx_fd_datasync = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };

                    // eprintln!("fd_datasync({a0})");

                    let code = block_on(wasi_snapshot_preview1::fd_datasync(
                        &mut *wasi_ctx_fd_datasync.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("fd_fdstat_get", "ii".into(), "i".into(), {
                let wasi_ctx_fd_fdstat_get = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a1)) = args.get(1) else {
                        panic!("expected i32");
                    };

                    // eprintln!("fd_fdstat_get({a0}, {a1})");

                    let code = block_on(wasi_snapshot_preview1::fd_fdstat_get(
                        &mut *wasi_ctx_fd_fdstat_get.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("fd_fdstat_set_flags", "ii".into(), "i".into(), {
                let wasi_ctx_fd_fdstat_set_flags = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a1)) = args.get(1) else {
                        panic!("expected i32");
                    };

                    // eprintln!("fd_fdstat_set_flags({a0}, {a1})");

                    let code = block_on(wasi_snapshot_preview1::fd_fdstat_set_flags(
                        &mut *wasi_ctx_fd_fdstat_set_flags.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("fd_fdstat_set_rights", "iII".into(), "i".into(), {
                let wasi_ctx_fd_fdstat_set_rights = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I64(a1)) = args.get(1) else {
                        panic!("expected i64");
                    };
                    let Some(Value::I64(a2)) = args.get(2) else {
                        panic!("expected i64");
                    };

                    eprintln!("fd_fdstat_set_rights({a0}, {a1}, {a2})");

                    let code = block_on(wasi_snapshot_preview1::fd_fdstat_set_rights(
                        &mut *wasi_ctx_fd_fdstat_set_rights.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                        *a2,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("fd_filestat_get", "ii".into(), "i".into(), {
                let wasi_ctx_fd_filestat_get = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a1)) = args.get(1) else {
                        panic!("expected i32");
                    };

                    // eprintln!("fd_filestat_get({a0}, {a1})");

                    let code = block_on(wasi_snapshot_preview1::fd_filestat_get(
                        &mut *wasi_ctx_fd_filestat_get.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("fd_filestat_set_size", "iI".into(), "i".into(), {
                let wasi_ctx_fd_filestat_set_size = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I64(a1)) = args.get(1) else {
                        panic!("expected i64");
                    };

                    // eprintln!("fd_filestat_set_size({a0}, {a1})");

                    let code = block_on(wasi_snapshot_preview1::fd_filestat_set_size(
                        &mut *wasi_ctx_fd_filestat_set_size.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("fd_filestat_set_times", "iIIi".into(), "i".into(), {
                let wasi_ctx_fd_filestat_set_times = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I64(a1)) = args.get(1) else {
                        panic!("expected i64");
                    };
                    let Some(Value::I64(a2)) = args.get(2) else {
                        panic!("expected i64");
                    };
                    let Some(Value::I32(a3)) = args.get(3) else {
                        panic!("expected i32");
                    };

                    // eprintln!("fd_filestat_set_times({a0}, {a1}, {a2}, {a3})");

                    let code = block_on(wasi_snapshot_preview1::fd_filestat_set_times(
                        &mut *wasi_ctx_fd_filestat_set_times.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                        *a2,
                        *a3,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("fd_pread", "iiiIi".into(), "i".into(), {
                let wasi_ctx_fd_pread = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a1)) = args.get(1) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a2)) = args.get(2) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I64(a3)) = args.get(3) else {
                        panic!("expected i64");
                    };
                    let Some(Value::I32(a4)) = args.get(4) else {
                        panic!("expected i32");
                    };

                    // eprintln!("fd_pread({a0}, {a1}, {a2}, {a3}, {a4})");

                    let code = block_on(wasi_snapshot_preview1::fd_pread(
                        &mut *wasi_ctx_fd_pread.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                        *a2,
                        *a3,
                        *a4,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("fd_prestat_get", "ii".into(), "i".into(), {
                let wasi_ctx_fd_prestat_get = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a1)) = args.get(1) else {
                        panic!("expected i32");
                    };

                    // eprintln!("fd_prestat_get({a0}, {a1})");

                    let code = block_on(wasi_snapshot_preview1::fd_prestat_get(
                        &mut *wasi_ctx_fd_prestat_get.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                    ))
                    .unwrap();

                    // eprintln!("\t-->{code}");
                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("fd_prestat_dir_name", "iii".into(), "i".into(), {
                let wasi_ctx_fd_prestat_dir_name = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a1)) = args.get(1) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a2)) = args.get(2) else {
                        panic!("expected i32");
                    };

                    // eprintln!("fd_prestat_dir_name({a0}, {a1}, {a2})");

                    let code = block_on(wasi_snapshot_preview1::fd_prestat_dir_name(
                        &mut *wasi_ctx_fd_prestat_dir_name.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                        *a2,
                    ))
                    .unwrap();

                    // eprintln!("\t-->{code}");

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("fd_pwrite", "iiiIi".into(), "i".into(), {
                let wasi_ctx_fd_pwrite = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a1)) = args.get(1) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a2)) = args.get(2) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I64(a3)) = args.get(3) else {
                        panic!("expected i64");
                    };
                    let Some(Value::I32(a4)) = args.get(4) else {
                        panic!("expected i32");
                    };

                    // eprintln!("fd_pwrite({a0}, {a1}, {a2}, {a3}, {a4})");

                    let code = block_on(wasi_snapshot_preview1::fd_pwrite(
                        &mut *wasi_ctx_fd_pwrite.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                        *a2,
                        *a3,
                        *a4,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("fd_read", "iiii".into(), "i".into(), {
                let wasi_ctx_fd_read = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a1)) = args.get(1) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a2)) = args.get(2) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a3)) = args.get(3) else {
                        panic!("expected i32");
                    };

                    // eprintln!("fd_read({a0}, {a1}, {a2}, {a3})");

                    let code = block_on(wasi_snapshot_preview1::fd_read(
                        &mut *wasi_ctx_fd_read.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                        *a2,
                        *a3,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("fd_readdir", "iiiIi".into(), "i".into(), {
                let wasi_ctx_fd_readdir = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a1)) = args.get(1) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a2)) = args.get(2) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I64(a3)) = args.get(3) else {
                        panic!("expected i64");
                    };
                    let Some(Value::I32(a4)) = args.get(4) else {
                        panic!("expected i32");
                    };

                    // eprintln!("fd_readdir({a0}, {a1}, {a2}, {a3}, {a4})");

                    let code = block_on(wasi_snapshot_preview1::fd_readdir(
                        &mut *wasi_ctx_fd_readdir.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                        *a2,
                        *a3,
                        *a4,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("fd_seek", "iIii".into(), "i".into(), {
                let wasi_ctx_fd_seek = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I64(a1)) = args.get(1) else {
                        panic!("expected i64");
                    };
                    let Some(Value::I32(a2)) = args.get(2) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a3)) = args.get(3) else {
                        panic!("expected i32");
                    };

                    // eprintln!("fd_seek({a0}, {a1}, {a2}, {a3})");

                    let code = block_on(wasi_snapshot_preview1::fd_seek(
                        &mut *wasi_ctx_fd_seek.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                        *a2,
                        *a3,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("fd_sync", "i".into(), "i".into(), {
                let wasi_ctx_fd_sync = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };

                    // eprintln!("fd_sync({a0})");

                    let code = block_on(wasi_snapshot_preview1::fd_sync(
                        &mut *wasi_ctx_fd_sync.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("fd_tell", "ii".into(), "i".into(), {
                let wasi_ctx_fd_tell = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a1)) = args.get(1) else {
                        panic!("expected i32");
                    };

                    // eprintln!("fd_tell({a0}, {a1})");

                    let code = block_on(wasi_snapshot_preview1::fd_tell(
                        &mut *wasi_ctx_fd_tell.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("fd_write", "iiii".into(), "i".into(), {
                let wasi_ctx_fd_write = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a1)) = args.get(1) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a2)) = args.get(2) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a3)) = args.get(3) else {
                        panic!("expected i32");
                    };

                    // eprintln!("fd_write({a0}, {a1}, {a2}, {a3})");

                    let code = block_on(wasi_snapshot_preview1::fd_write(
                        &mut *wasi_ctx_fd_write.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                        *a2,
                        *a3,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("path_create_directory", "iii".into(), "i".into(), {
                let wasi_ctx_path_create_directory = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a1)) = args.get(1) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a2)) = args.get(2) else {
                        panic!("expected i32");
                    };

                    // eprintln!("path_create_directory({a0}, {a1}, {a2})");

                    let code = block_on(wasi_snapshot_preview1::path_create_directory(
                        &mut *wasi_ctx_path_create_directory.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                        *a2,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("path_filestat_get", "iiiii".into(), "i".into(), {
                let wasi_ctx_path_filestat_get = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a1)) = args.get(1) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a2)) = args.get(2) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a3)) = args.get(3) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a4)) = args.get(4) else {
                        panic!("expected i32");
                    };

                    // eprintln!("path_filestat_get({a0}, {a1}, {a2}, {a3}, {a4})");

                    let code = block_on(wasi_snapshot_preview1::path_filestat_get(
                        &mut *wasi_ctx_path_filestat_get.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                        *a2,
                        *a3,
                        *a4,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("path_filestat_set_times", "iiiiIIi".into(), "i".into(), {
                let wasi_ctx_path_filestat_set_times = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a1)) = args.get(1) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a2)) = args.get(2) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a3)) = args.get(3) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I64(a4)) = args.get(4) else {
                        panic!("expected i64");
                    };
                    let Some(Value::I64(a5)) = args.get(5) else {
                        panic!("expected i64");
                    };
                    let Some(Value::I32(a6)) = args.get(6) else {
                        panic!("expected i32");
                    };

                    // eprintln!("path_filestat_set_times({a0}, {a1}, {a2}, {a3}, {a4}, {a5}, {a6})");

                    let code = block_on(wasi_snapshot_preview1::path_filestat_set_times(
                        &mut *wasi_ctx_path_filestat_set_times.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                        *a2,
                        *a3,
                        *a4,
                        *a5,
                        *a6,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("path_link", "iiiiiii".into(), "i".into(), {
                let wasi_ctx_path_link = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a1)) = args.get(1) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a2)) = args.get(2) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a3)) = args.get(3) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a4)) = args.get(4) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a5)) = args.get(5) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a6)) = args.get(6) else {
                        panic!("expected i32");
                    };

                    // eprintln!("path_link({a0}, {a1}, {a2}, {a3}, {a4}, {a5}, {a6})");

                    let code = block_on(wasi_snapshot_preview1::path_link(
                        &mut *wasi_ctx_path_link.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                        *a2,
                        *a3,
                        *a4,
                        *a5,
                        *a6,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("path_open", "iiiiiIIii".into(), "i".into(), {
                let wasi_ctx_path_open = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a1)) = args.get(1) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a2)) = args.get(2) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a3)) = args.get(3) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a4)) = args.get(4) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I64(a5)) = args.get(5) else {
                        panic!("expected i64");
                    };
                    let Some(Value::I64(a6)) = args.get(6) else {
                        panic!("expected i64");
                    };
                    let Some(Value::I32(a7)) = args.get(7) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a8)) = args.get(8) else {
                        panic!("expected i32");
                    };

                    // eprintln!("path_open({a0}, {a1}, {a2}, {a3}, {a4}, {a5}, {a6}, {a7}, {a8})");

                    let code = block_on(wasi_snapshot_preview1::path_open(
                        &mut *wasi_ctx_path_open.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                        *a2,
                        *a3,
                        *a4,
                        *a5,
                        *a6,
                        *a7,
                        *a8,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("path_readlink", "iiiiii".into(), "i".into(), {
                let wasi_ctx_path_readlink = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a1)) = args.get(1) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a2)) = args.get(2) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a3)) = args.get(3) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a4)) = args.get(4) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a5)) = args.get(5) else {
                        panic!("expected i32");
                    };

                    // eprintln!("path_readlink({a0}, {a1}, {a2}, {a3}, {a4}, {a5})");

                    let code = block_on(wasi_snapshot_preview1::path_readlink(
                        &mut *wasi_ctx_path_readlink.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                        *a2,
                        *a3,
                        *a4,
                        *a5,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("path_remove_directory", "iii".into(), "i".into(), {
                let wasi_ctx_path_remove_directory = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a1)) = args.get(1) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a2)) = args.get(2) else {
                        panic!("expected i32");
                    };

                    // eprintln!("path_remove_directory({a0}, {a1}, {a2})");

                    let code = block_on(wasi_snapshot_preview1::path_remove_directory(
                        &mut *wasi_ctx_path_remove_directory.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                        *a2,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("path_rename", "iiiiii".into(), "i".into(), {
                let wasi_ctx_path_rename = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a1)) = args.get(1) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a2)) = args.get(2) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a3)) = args.get(3) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a4)) = args.get(4) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a5)) = args.get(5) else {
                        panic!("expected i32");
                    };

                    // eprintln!("path_rename({a0}, {a1}, {a2}, {a3}, {a4}, {a5})");

                    let code = block_on(wasi_snapshot_preview1::path_rename(
                        &mut *wasi_ctx_path_rename.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                        *a2,
                        *a3,
                        *a4,
                        *a5,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("path_symlink", "iiiii".into(), "i".into(), {
                let wasi_ctx_path_symlink = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a1)) = args.get(1) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a2)) = args.get(2) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a3)) = args.get(3) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a4)) = args.get(4) else {
                        panic!("expected i32");
                    };

                    // eprintln!("path_symlink({a0}, {a1}, {a2}, {a3}, {a4})");

                    let code = block_on(wasi_snapshot_preview1::path_symlink(
                        &mut *wasi_ctx_path_symlink.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                        *a2,
                        *a3,
                        *a4,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("path_unlink_file", "iii".into(), "i".into(), {
                let wasi_ctx_path_unlink_file = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a1)) = args.get(1) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a2)) = args.get(2) else {
                        panic!("expected i32");
                    };

                    // eprintln!("path_unlink_file({a0}, {a1}, {a2})");

                    let code = block_on(wasi_snapshot_preview1::path_unlink_file(
                        &mut *wasi_ctx_path_unlink_file.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                        *a2,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("poll_oneoff", "iiii".into(), "i".into(), {
                let wasi_ctx_poll_oneoff = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a1)) = args.get(1) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a2)) = args.get(2) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a3)) = args.get(3) else {
                        panic!("expected i32");
                    };

                    // eprintln!("poll_oneoff({a0}, {a1}, {a2}, {a3})");

                    let code = block_on(wasi_snapshot_preview1::poll_oneoff(
                        &mut *wasi_ctx_poll_oneoff.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                        *a2,
                        *a3,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("proc_exit", "i".into(), "".into(), {
                let wasi_ctx_proc_exit = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };

                    let _ = block_on(wasi_snapshot_preview1::proc_exit(
                        &mut *wasi_ctx_proc_exit.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                    ));

                    std::process::exit(*a0);
                }
            }),
            HostFunction::new("proc_raise", "i".into(), "i".into(), {
                let wasi_ctx_proc_raise = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };

                    // eprintln!("proc_raise({a0}))");

                    let code = block_on(wasi_snapshot_preview1::proc_raise(
                        &mut *wasi_ctx_proc_raise.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("random_get", "ii".into(), "i".into(), {
                let wasi_ctx_random_get = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a1)) = args.get(1) else {
                        panic!("expected i32");
                    };

                    // eprintln!("random_get({a0}, {a1})");

                    let code = block_on(wasi_snapshot_preview1::random_get(
                        &mut *wasi_ctx_random_get.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                    ))
                    .unwrap();

                    // eprintln!("\t\t{:?}", state.memory.get_slice().get((*a0 as usize)..((*a0 as usize) + (*a1 as usize))));
                    // eprintln!("\t-->{code}");

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("sched_yield", "".into(), "i".into(), {
                let wasi_ctx_sched_yield = Rc::clone(&wasi_ctx_two);
                move |state, _| {
                    // eprintln!("sched_yield())");

                    let code = block_on(wasi_snapshot_preview1::sched_yield(
                        &mut *wasi_ctx_sched_yield.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("sock_accept", "iii".into(), "i".into(), {
                let wasi_ctx_sock_accept = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a1)) = args.get(1) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a2)) = args.get(2) else {
                        panic!("expected i32");
                    };

                    // eprintln!("sock_accept({a0}, {a1}, {a2})");

                    let code = block_on(wasi_snapshot_preview1::sock_accept(
                        &mut *wasi_ctx_sock_accept.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                        *a2,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("sock_recv", "iiiiii".into(), "i".into(), {
                let wasi_ctx_sock_recv = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a1)) = args.get(1) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a2)) = args.get(2) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a3)) = args.get(3) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a4)) = args.get(4) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a5)) = args.get(5) else {
                        panic!("expected i32");
                    };

                    // eprintln!("sock_recv({a0}, {a1}, {a2}, {a3}, {a4}, {a5})");

                    let code = block_on(wasi_snapshot_preview1::sock_recv(
                        &mut *wasi_ctx_sock_recv.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                        *a2,
                        *a3,
                        *a4,
                        *a5,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("sock_send", "iiiii".into(), "i".into(), {
                let wasi_ctx_sock_send = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a1)) = args.get(1) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a2)) = args.get(2) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a3)) = args.get(3) else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a4)) = args.get(4) else {
                        panic!("expected i32");
                    };

                    // eprintln!("sock_send({a0}, {a1}, {a2}, {a3}, {a4})");

                    let code = block_on(wasi_snapshot_preview1::sock_send(
                        &mut *wasi_ctx_sock_send.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                        *a2,
                        *a3,
                        *a4,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
            HostFunction::new("sock_shutdown", "ii".into(), "i".into(), {
                let wasi_ctx_sock_shutdown = Rc::clone(&wasi_ctx_two);
                move |state, args| {
                    let Some(Value::I32(a0)) = args.first() else {
                        panic!("expected i32");
                    };
                    let Some(Value::I32(a1)) = args.get(1) else {
                        panic!("expected i32");
                    };

                    // eprintln!("sock_shutdown({a0}, {a1})");

                    let code = block_on(wasi_snapshot_preview1::sock_shutdown(
                        &mut *wasi_ctx_sock_shutdown.borrow_mut(),
                        &mut GuestMemory::Shared(unsafe {
                            core::mem::transmute::<&[u8], &[std::cell::UnsafeCell<u8>]>(
                                state.memory.get_slice(),
                            )
                        }),
                        *a0,
                        *a1,
                    ))
                    .unwrap();

                    ControlFlow::Continue(Some(Value::I32(code as i32)))
                }
            }),
        ],
        memory: spacewasm::Vec::zero(),
        table: spacewasm::Vec::zero(),
    }
}
