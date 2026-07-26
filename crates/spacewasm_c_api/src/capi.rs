//! The concrete `extern "C"` entry points (`spacewasm_*`) that make up the ABI.

use core::ffi::c_char;
use core::ffi::c_void;

use spacewasm::{
    CodeBuilder, CompilerOptions, Engine, ExportDesc, HostFunction, HostModule, Interpreter,
    InterpreterRunner, Module, ModuleRef, Ref, StartInvocation, ValType, Value, Vec, WasmRef,
};

use crate::SpacewasmCaller;
use crate::alloc::{
    self, SpacewasmAllocator, spacewasm_alloc_fn_t, spacewasm_dealloc_fn_t, spacewasm_realloc_fn_t,
};
use crate::config::{MAX_CONTROL_FRAMES, MAX_STACK_DEPTH};
use crate::host;
use crate::host::CHostFunction;
use crate::host::spacewasm_host_fn_t;
use crate::status::{self, spacewasm_run_status_t, spacewasm_status_t, spacewasm_trap_t};
use crate::stream::{CallbackStream, spacewasm_read_fn_t};
use crate::value::{spacewasm_valtype_t, spacewasm_value_t};

macro_rules! check {
    ($val:expr) => {
        match $val {
            Ok(v) => v,
            Err(e) => return e,
        }
    };
}

/// FFI-safe mirror of [`spacewasm::CompilerOptions`], controlling how guest
/// modules loaded onto a store are compiled. Passed to [`spacewasm_new`].
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct spacewasm_compiler_options_t {
    /// Allow compiling `memory.grow` instructions. When `false`, a module using
    /// `memory.grow` is rejected at load time.
    pub allow_memory_grow: bool,

    /// Maximum number of iterations to resolve during a control-flow backpatch.
    /// Bounds compile time on pathological modules at the cost of rejecting some
    /// valid programs. Set to 0 for unlimited iterations.
    pub max_backpatch_iterations: u32,

    /// Maximum number of compiled code pages allowed across all modules loaded
    /// onto the store.
    pub max_code_pages: u32,
}

impl From<spacewasm_compiler_options_t> for CompilerOptions {
    fn from(o: spacewasm_compiler_options_t) -> Self {
        CompilerOptions {
            allow_memory_grow: o.allow_memory_grow,
            max_backpatch_iterations: o.max_backpatch_iterations,
            max_code_pages: o.max_code_pages,
        }
    }
}

/// C Engine wrapper combining the core Engine with its CodeBuilder.
pub struct CEngine {
    engine: Engine,
    code_builder: CodeBuilder,
}

/// Interpret a NUL-terminated C string as a Rust `&str`.
///
/// # Safety
/// `ptr` must be NUL-terminated and valid, or null.
pub(crate) unsafe fn cstr<'a>(ptr: *const c_char) -> Result<&'a str, spacewasm_status_t> {
    if ptr.is_null() {
        return Err(status::SPACEWASM_ERR_NULL_ARG);
    }
    // SAFETY: caller guarantees NUL-termination and validity.
    let c = unsafe { core::ffi::CStr::from_ptr(ptr) };
    c.to_str().map_err(|_| status::SPACEWASM_ERR_BAD_UTF8)
}

/// Create a guest linear-memory allocator from three C callbacks, returning an
/// opaque handle (or null if any callback is null or allocation fails). The
/// handle is passed to [`spacewasm_load_module`] and must be released with
/// [`spacewasm_allocator_destroy`]. `userdata` is passed to every callback.
#[unsafe(no_mangle)]
pub extern "C" fn spacewasm_allocator_new(
    alloc: spacewasm_alloc_fn_t,
    realloc: spacewasm_realloc_fn_t,
    dealloc: spacewasm_dealloc_fn_t,
    userdata: *mut c_void,
) -> *mut SpacewasmAllocator {
    alloc::allocator_new(alloc, realloc, dealloc, userdata)
}

/// Destroy an allocator handle. No-op on null. Any loaded module keeps its own
/// reference to the underlying allocator, so destroying the handle after loading
/// is safe.
///
/// # Safety
/// `allocator` must be a live handle from [`spacewasm_allocator_new`], not
/// already destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_allocator_destroy(allocator: *mut SpacewasmAllocator) {
    unsafe { alloc::allocator_destroy(allocator) }
}

#[repr(C)]
pub struct spacewasm_host_t {
    ptr: *mut spacewasm::HostModule,
    capacity: u32,
    len: u32,
}

impl From<spacewasm::Vec<spacewasm::HostModule>> for spacewasm_host_t {
    fn from(value: spacewasm::Vec<spacewasm::HostModule>) -> Self {
        unsafe { core::mem::transmute(value) }
    }
}

impl From<spacewasm_host_t> for spacewasm::Vec<spacewasm::HostModule> {
    fn from(value: spacewasm_host_t) -> Self {
        unsafe { core::mem::transmute(value) }
    }
}

impl From<&mut spacewasm_host_t> for &mut spacewasm::Vec<spacewasm::HostModule> {
    fn from(value: &mut spacewasm_host_t) -> Self {
        unsafe { core::mem::transmute(value) }
    }
}

/// Create a new host module vector of max_host_module size
///
/// # Safety
/// `host` must be live
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_host_new(
    len: u32,
    dest: *mut spacewasm_host_t,
) -> spacewasm_status_t {
    if dest.is_null() {
        return status::SPACEWASM_ERR_NULL_ARG;
    }

    let v = check!(spacewasm::Vec::new(len).map_err(status::alloc_status));
    unsafe { *dest = v.into() };
    status::SPACEWASM_OK
}

/// Append a host function to the host vector.
///
/// # Safety
/// `host` and `module_name` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_host_add_func(
    host: *mut spacewasm_host_t,
    module_name: *const c_char,
    export_name: *const c_char,
    params: *const spacewasm_valtype_t,
    n_params: usize,
    result: spacewasm_valtype_t,
    func: spacewasm_host_fn_t,
    userdata: *mut c_void,
) -> spacewasm_status_t {
    let Some(host) = (unsafe { host.as_mut() }) else {
        return status::SPACEWASM_ERR_NULL_ARG;
    };

    let module_name = check!(unsafe { cstr(module_name) });
    let export_name = check!(unsafe { cstr(export_name) });

    let params_slice = if params.is_null() || n_params == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(params, n_params) }
    };

    // If n_params is 0 and params is null, it might indicate no result type
    // For now, we'll always create Some result type from the given value
    let result_ty = Some(ValType::from(result));

    let h: &mut spacewasm::Vec<spacewasm::HostModule> = host.into();

    // Find or create the host module
    let hmod = h.iter_mut().find(|m| m.name == module_name);
    let hmod = match hmod {
        Some(m) => m,
        None => {
            h.push(HostModule {
                name: module_name.into(),
                globals: Vec::zero(),
                functions: Vec::zero(),
                memory: Vec::zero(),
                table: Vec::zero(),
            });
            h.last_mut().unwrap()
        }
    };

    // Create the host function
    let params_vec = check!(spacewasm::Vec::from_slice(params_slice).map_err(status::alloc_status));
    let hf = check!(
        HostFunction::new(
            export_name,
            params_vec,
            result_ty,
            CHostFunction::new(func.unwrap(), userdata)
        )
        .map_err(status::host_name_status)
    );

    hmod.functions.push(hf);
    status::SPACEWASM_OK
}

/// Load a guest module named `name` onto an existing engine by streaming its
/// bytes through the `read` callback. The callback owns the buffer backing each
/// chunk (see [`spacewasm_read_fn_t`]). This does not run the module's start
/// function; use [`spacewasm_invoke_start`] for that. `allocator` supplies the
/// guest linear memory (see [`spacewasm_allocator_new`]). Writes the new module's
/// index to `out_module_idx` (if non-null). May be called repeatedly to load
/// several modules onto the same engine.
///
/// # Safety
/// `engine` and `allocator` must be live handles; `read` a valid callback;
/// `out_module_idx` null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_load_module(
    engine: *mut CEngine,
    name: *const c_char,
    read: spacewasm_read_fn_t,
    read_userdata: *mut c_void,
    allocator: *mut SpacewasmAllocator,
    out_module_idx: *mut u32,
) -> spacewasm_status_t {
    // SAFETY: `allocator` is null or a live handle per the contract.
    let Some(alloc) = (unsafe { alloc::allocator_clone_rc(allocator) }) else {
        return status::SPACEWASM_ERR_NULL_ARG;
    };

    let Some(cengine) = (unsafe { engine.as_mut() }) else {
        return status::SPACEWASM_ERR_NULL_ARG;
    };

    if !cengine.engine.is_idle() {
        return status::SPACEWASM_ERR_WRONG_STATE;
    }

    let name = check!(unsafe { cstr(name) });

    let mut stream = check!(CallbackStream::new(read, read_userdata));

    let module = match Module::new::<MAX_CONTROL_FRAMES, MAX_STACK_DEPTH>(
        name,
        &mut stream,
        &mut cengine.engine.store,
        &mut cengine.code_builder,
        alloc,
    ) {
        Ok(m) => m,
        // If the callback reported an I/O error, surface that rather than a
        // generic parse failure.
        Err(e) if stream.errored() => {
            let _ = e;
            return status::SPACEWASM_ERR_STREAM;
        }
        Err(e) => return status::parse_status(&e),
    };

    let module_ref = cengine.engine.push_module(module);
    let idx = module_ref.0 as u32;

    if !out_module_idx.is_null() {
        unsafe { *out_module_idx = idx };
    }
    status::SPACEWASM_OK
}

/// Consume the host module vector `host` and finish it into an engine handle,
/// written to `out_engine`. The engine is sized with a `stack_size`-byte guest
/// stack, room for `max_modules` guest modules (≤ 256), and compiles guest
/// modules according to `options` (code-page budget, `memory.grow` support,
/// backpatch bound). No guest module is loaded yet; use
/// [`spacewasm_load_module`] to load one or more.
///
/// `host` may be null to create an engine with no host modules. The host vector
/// is always consumed (its handle must not be used or destroyed afterwards),
/// whether or not the engine is created successfully.
///
/// # Safety
/// `host` must be null or a live handle from [`spacewasm_host_new`], not already
/// consumed/destroyed; `out_engine` must be a valid pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_new(
    host: *mut spacewasm_host_t,
    stack_size: usize,
    max_modules: u32,
    options: spacewasm_compiler_options_t,
    out_engine: *mut *mut CEngine,
) -> spacewasm_status_t {
    if out_engine.is_null() {
        return status::SPACEWASM_ERR_NULL_ARG;
    }

    if max_modules > 256 {
        return status::SPACEWASM_ERR_BAD_ARG;
    }

    // Take ownership of the host modules (consuming the handle), or start from
    // an empty set when none were supplied.
    let host_modules: spacewasm::Vec<spacewasm::HostModule> = if host.is_null() {
        check!(spacewasm::Vec::new(0).map_err(status::alloc_status))
    } else {
        unsafe { host.read() }.into()
    };

    let engine = check!(
        Engine::new(stack_size, max_modules as usize, host_modules).map_err(status::memory_status)
    );

    let code_builder = check!(CodeBuilder::new(options.into()).map_err(status::alloc_status));

    let cengine = CEngine {
        engine,
        code_builder,
    };

    let boxed = check!(spacewasm::Box::new(cengine).map_err(status::alloc_status));
    unsafe { *out_engine = spacewasm::Box::leak(boxed) as *mut CEngine };
    status::SPACEWASM_OK
}

/// Destroy a host vector that was never consumed into an engine. No-op on null.
///
/// # Safety
/// `host` must be null or a live unconsumed handle from [`spacewasm_host_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_host_destroy(host: *mut spacewasm_host_t) {
    if host.is_null() {
        return;
    }
    // Convert the handle back to the owning `Vec` so its allocation (and each
    // `HostModule`) is freed.
    let modules: spacewasm::Vec<spacewasm::HostModule> = unsafe { host.read() }.into();
    core::mem::drop(modules);
}

/// Find a module with a given name in the engine.
///
/// # Safety
/// `engine` must be live; `name` valid; `out_index` valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_find_module(
    engine: *mut CEngine,
    name: *const c_char,
    out_index: *mut u32,
) -> spacewasm_status_t {
    let Some(cengine) = (unsafe { engine.as_ref() }) else {
        return status::SPACEWASM_ERR_NULL_ARG;
    };
    if out_index.is_null() {
        return status::SPACEWASM_ERR_NULL_ARG;
    }
    let name = check!(unsafe { cstr(name) });

    let idx = cengine
        .engine
        .store
        .modules()
        .iter()
        .enumerate()
        .find_map(|(i, m)| if m.name == name { Some(i as u32) } else { None })
        .ok_or(status::SPACEWASM_ERR_NOT_FOUND);

    match idx {
        Ok(i) => {
            unsafe { *out_index = i };
            status::SPACEWASM_OK
        }
        Err(e) => e,
    }
}

/// Look up the exported function named `name` in module `module_idx` and write
/// its index to `out_index`.
///
/// # Safety
/// `engine` must be live; `name` valid; `out_index` valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_find_export_func(
    engine: *mut CEngine,
    module_idx: u32,
    name: *const c_char,
    out_index: *mut u32,
) -> spacewasm_status_t {
    let Some(cengine) = (unsafe { engine.as_ref() }) else {
        return status::SPACEWASM_ERR_NULL_ARG;
    };
    if out_index.is_null() {
        return status::SPACEWASM_ERR_NULL_ARG;
    }
    let name = check!(unsafe { cstr(name) });

    let module = match cengine.engine.store.modules().get(module_idx as usize) {
        Some(m) => m,
        None => return status::SPACEWASM_ERR_NOT_FOUND,
    };

    for e in &module.exports {
        if e.name == name {
            if let ExportDesc::Func(fi) = e.desc {
                return match module.get_func_ref(fi) {
                    Some(Ref::Module(idx)) => {
                        unsafe { *out_index = idx as u32 };
                        status::SPACEWASM_OK
                    }
                    _ => status::SPACEWASM_ERR_NOT_FOUND,
                };
            }
        }
    }
    status::SPACEWASM_ERR_NOT_FOUND
}

/// Invoke the start function of a module.
///
/// If there is no start function, return [`spacewasm_run_status_t::SPACEWASM_RUN_FINISHED`]
/// If there is a start function, return [`spacewasm_run_status_t::SPACEWASM_RUN_OUT_OF_FUEL`]
///
/// If there are any bad arguments or the start function is a host function that traps,
/// return [`spacewasm_run_status_t::SPACEWASM_RUN_TRAP`]
///
/// # Safety
/// `engine` must be live
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_invoke_start(
    engine: *mut CEngine,
    module_idx: u32,
) -> spacewasm_run_status_t {
    let Some(cengine) = (unsafe { engine.as_mut() }) else {
        return spacewasm_run_status_t::SPACEWASM_RUN_TRAP;
    };

    if !cengine.engine.is_idle() {
        return spacewasm_run_status_t::SPACEWASM_RUN_TRAP;
    }

    if module_idx as usize >= cengine.engine.store.modules().len() {
        return spacewasm_run_status_t::SPACEWASM_RUN_TRAP;
    }

    match cengine.engine.invoke_start(ModuleRef(module_idx as u8)) {
        StartInvocation::Finished => spacewasm_run_status_t::SPACEWASM_RUN_FINISHED,
        StartInvocation::Trap(_) => spacewasm_run_status_t::SPACEWASM_RUN_TRAP,
        StartInvocation::Pause => spacewasm_run_status_t::SPACEWASM_RUN_PAUSE,
        StartInvocation::Running => spacewasm_run_status_t::SPACEWASM_RUN_OUT_OF_FUEL,
    }
}

/// Set up a call to exported function `func_index` of module `module_idx` with
/// the `n` arguments in `params`. Does not run the function; drive execution
/// with [`spacewasm_run`].
///
/// # Safety
/// `engine` must be live; `params` valid for `n` entries.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_invoke(
    engine: *mut CEngine,
    module_idx: u32,
    func_index: u32,
    params: *const spacewasm_value_t,
    n: usize,
) -> spacewasm_status_t {
    let Some(cengine) = (unsafe { engine.as_mut() }) else {
        return status::SPACEWASM_ERR_NULL_ARG;
    };
    if params.is_null() && n != 0 {
        return status::SPACEWASM_ERR_NULL_ARG;
    }
    if func_index > u16::MAX as u32 {
        return status::SPACEWASM_ERR_BAD_ARG;
    }

    if !cengine.engine.is_idle() {
        return status::SPACEWASM_ERR_WRONG_STATE;
    }

    if module_idx as usize >= cengine.engine.store.modules().len() {
        return status::SPACEWASM_ERR_NOT_FOUND;
    }

    let mut buf: [Value; 64] = [Value::I32(0); 64];
    if n > buf.len() {
        return status::SPACEWASM_ERR_CAPACITY;
    }

    // Marshal parameters. `from_raw_parts` requires a non-null pointer even for
    // a zero-length slice, so only build the slice when there are entries (the
    // contract permits a null `params` when `n == 0`).
    if n != 0 {
        let slice = unsafe { core::slice::from_raw_parts(params, n) };
        for (i, v) in slice.iter().enumerate() {
            buf[i] = v.to_value();
        }
    }

    let f_ref = WasmRef {
        module: ModuleRef(module_idx as u8),
        index: func_index as u16,
    };

    match cengine.engine.invoke(f_ref, &buf[..n]) {
        Ok(()) => status::SPACEWASM_OK,
        Err(e) => status::invoke_status(e),
    }
}

/// Run the pending invocation for up to `fuel` units of work, writing any trap
/// to `out_trap`. Returns whether the call finished, trapped, or ran out of fuel.
///
/// # Safety
/// `engine` must be live; `out_trap` null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_run(
    engine: *mut CEngine,
    fuel: usize,
    out_trap: *mut spacewasm_trap_t,
) -> spacewasm_run_status_t {
    let Some(cengine) = (unsafe { engine.as_mut() }) else {
        return spacewasm_run_status_t::SPACEWASM_RUN_TRAP;
    };

    if cengine.engine.is_idle() {
        return spacewasm_run_status_t::SPACEWASM_RUN_TRAP;
    }

    let interpreter = Interpreter;
    let code_pages = cengine.code_builder.pages();
    let result = interpreter.run(code_pages, &mut cengine.engine, fuel);

    let (rs, trap) = status::run_status(&result);
    if !out_trap.is_null() {
        unsafe { *out_trap = trap };
    }
    rs
}

/// Resume the interpreter from a paused state.
///
/// # Safety
/// `engine` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_resume(engine: *mut CEngine) -> spacewasm_status_t {
    let Some(cengine) = (unsafe { engine.as_mut() }) else {
        return status::SPACEWASM_ERR_NULL_ARG;
    };

    cengine.engine.resume(None);
    status::SPACEWASM_OK
}

/// Resume the interpreter from a paused state.
/// This function will also push a value to the interpreter stack
/// as the return value of the host function that requested a pause.
///
/// # Safety
/// `engine` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_resume_value(
    engine: *mut CEngine,
    resume_value: spacewasm_value_t,
) -> spacewasm_status_t {
    let Some(cengine) = (unsafe { engine.as_mut() }) else {
        return status::SPACEWASM_ERR_NULL_ARG;
    };

    cengine.engine.resume(Some(resume_value.into()));
    status::SPACEWASM_OK
}

/// Fetch the result of the last completed call, coerced to `expected`, into
/// `out`.
///
/// # Safety
/// `engine` must be live; `out` valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_get_result(
    engine: *mut CEngine,
    expected: spacewasm_valtype_t,
    out: *mut spacewasm_value_t,
) -> spacewasm_status_t {
    let Some(cengine) = (unsafe { engine.as_ref() }) else {
        return status::SPACEWASM_ERR_NULL_ARG;
    };
    if out.is_null() {
        return status::SPACEWASM_ERR_NULL_ARG;
    }
    match cengine
        .engine
        .result
        .map(|raw| spacewasm_value_t::from_raw(raw, ValType::from(expected)))
    {
        Some(v) => {
            unsafe { *out = v };
            status::SPACEWASM_OK
        }
        None => status::SPACEWASM_ERR_NOT_FOUND,
    }
}

/// Destroy an engine and free its resources. No-op on null.
///
/// # Safety
/// `engine` must be a live handle, not already destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_destroy(engine: *mut CEngine) {
    if engine.is_null() {
        return;
    }
    let _ = unsafe { spacewasm::Box::from_raw(spacewasm::GlobalAllocator, engine) };
}

/// Read `len` bytes of guest linear memory starting at `addr` into `dst`.
/// Intended for use from within a host function.
///
/// # Safety
/// `caller` must be a live caller handle; `dst` valid for `len`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_mem_read(
    caller: *mut SpacewasmCaller,
    addr: u32,
    dst: *mut u8,
    len: usize,
) -> spacewasm_status_t {
    unsafe { host::mem_read(caller, addr, dst, len) }
}

/// Write `len` bytes from `src` to guest linear memory starting at `addr`.
/// Intended for use from within a host function.
///
/// # Safety
/// `caller` must be a live caller handle; `src` valid for `len`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_mem_write(
    caller: *mut SpacewasmCaller,
    addr: u32,
    src: *const u8,
    len: usize,
) -> spacewasm_status_t {
    unsafe { host::mem_write(caller, addr, src, len) }
}
