//! The concrete `extern "C"` entry points (`spacewasm_*`) that make up the ABI.

use core::ffi::c_char;
use core::ffi::c_void;

use spacewasm::{
    CodeBuilder, CompilerOptions, Engine, ExportDesc, HostFunction, HostModule, Interpreter,
    InterpreterRunner, Module, ModuleRef, RawValue, Ref, ValType, Value, Vec, WasmRef,
};

use crate::SpacewasmCaller;
use crate::alloc::CAllocator;
use crate::alloc::{self, spacewasm_alloc_fn_t, spacewasm_dealloc_fn_t, spacewasm_realloc_fn_t};
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

/// FFI-safe mirror of [`CompilerOptions`], controlling how guest
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
            max_backpatch_iterations: (o.max_backpatch_iterations != 0)
                .then_some(o.max_backpatch_iterations),
            max_code_pages: o.max_code_pages,
        }
    }
}

/// Handle holding the SpaceWasm engine and compiled IR code.
/// This handle is used for holding and executing the SpaceWasm interpreter.
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
) -> *mut CAllocator {
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
pub unsafe extern "C" fn spacewasm_allocator_destroy(allocator: *mut CAllocator) {
    unsafe { alloc::allocator_destroy(allocator) }
}

#[repr(C)]
pub struct spacewasm_host_t {
    ptr: *mut HostModule,
    capacity: u32,
    len: u32,
}

impl From<Vec<HostModule>> for spacewasm_host_t {
    fn from(value: Vec<HostModule>) -> Self {
        unsafe { core::mem::transmute(value) }
    }
}

impl From<spacewasm_host_t> for Vec<HostModule> {
    fn from(value: spacewasm_host_t) -> Self {
        unsafe { core::mem::transmute(value) }
    }
}

impl From<&mut spacewasm_host_t> for &mut Vec<HostModule> {
    fn from(value: &mut spacewasm_host_t) -> Self {
        unsafe { core::mem::transmute(value) }
    }
}

const _: () = {
    assert!(
        core::mem::size_of::<spacewasm_host_t>() == core::mem::size_of::<Vec<HostModule>>(),
        "spacewasm_host_t must match Vec<HostModule> layout (size)"
    );
    assert!(
        core::mem::align_of::<spacewasm_host_t>() == core::mem::align_of::<Vec<HostModule>>(),
        "spacewasm_host_t must match Vec<HostModule> layout (align)"
    );
};

/// Create a new host module vector of `len` size
///
/// # Safety
/// `dest` must be null or a valid, live pointer to write the new host vector into.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_host_new(
    len: u32,
    dest: *mut spacewasm_host_t,
) -> spacewasm_status_t {
    if dest.is_null() {
        return status::SPACEWASM_ERR_NULL_ARG;
    }

    let v = check!(Vec::new(len).map_err(status::alloc_status));
    unsafe { *dest = v.into() };
    status::SPACEWASM_OK
}

/// Add a host module named `name` sized for `max_functions` functions and
/// `max_globals` globals, writing its index to `out_idx` (if non-null).
///
/// # Safety
/// `host` must be live; all C strings valid and NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_add_host_module(
    host: *mut spacewasm_host_t,
    name: *const c_char,
    max_functions: u32,
    max_globals: u32,
    out_idx: *mut u32,
) -> spacewasm_status_t {
    let functions = check!(Vec::new(max_functions).map_err(status::alloc_status));
    let globals = check!(Vec::new(max_globals).map_err(status::alloc_status));
    let name = check!(unsafe { cstr(name) });
    let name = check!(spacewasm::HostName::try_from_str(name).map_err(status::host_name_status));

    let module = HostModule {
        name,
        globals,
        functions,
        memory: Vec::zero(),
        table: Vec::zero(),
    };

    let host: &mut Vec<HostModule> =
        check!(unsafe { host.as_mut() }.ok_or(status::SPACEWASM_ERR_NULL_ARG)).into();
    check!(
        host.try_push(module)
            .ok()
            .ok_or(status::SPACEWASM_ERR_CAPACITY)
    );

    if let Some(out_idx) = unsafe { out_idx.as_mut() } {
        *out_idx = (host.len() - 1) as u32;
    }

    status::SPACEWASM_OK
}

/// Register a host function `name` in host module `module_idx`, with parameter
/// and return signatures given by `params_sig`/`returns_sig` and implemented by
/// callback `f` (passed `userdata` on each call).
///
/// # Safety
/// `host` must be live; all C strings valid and NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_add_host_function(
    host: *mut spacewasm_host_t,
    module_idx: u32,
    name: *const c_char,
    params_sig: *const c_char,
    returns_sig: *const c_char,
    f: spacewasm_host_fn_t,
    userdata: *mut c_void,
) -> spacewasm_status_t {
    let name = check!(unsafe { cstr(name) });
    let params_sig = check!(unsafe { cstr(params_sig) });
    let returns_sig = check!(unsafe { cstr(returns_sig) });

    let name = check!(spacewasm::HostName::try_from_str(name).map_err(status::host_name_status));
    let params =
        check!(spacewasm::HostValList::try_new(params_sig).map_err(status::host_val_list_status));
    let returns =
        check!(spacewasm::HostValList::try_new(returns_sig).map_err(status::host_val_list_status));

    let host: &mut Vec<HostModule> =
        check!(unsafe { host.as_mut() }.ok_or(status::SPACEWASM_ERR_NULL_ARG)).into();

    let f = check!(f.ok_or(status::SPACEWASM_ERR_NULL_ARG));

    let trampoline = CHostFunction::new(f, userdata);
    let host_fn = check!(
        HostFunction::try_new(name, params, returns, move |state, args| {
            trampoline.call(state, args)
        })
        .map_err(status::host_val_list_status)
    );

    match host.get_mut(module_idx as usize) {
        Some(m) => match m.functions.try_push(host_fn) {
            Ok(()) => status::SPACEWASM_OK,
            Err(_) => status::SPACEWASM_ERR_CAPACITY,
        },
        None => status::SPACEWASM_ERR_NOT_FOUND,
    }
}

/// Load a guest module named `name` onto an existing engine by streaming its
/// bytes through the `read` callback. The callback owns the buffer backing each
/// chunk (see [`spacewasm_read_fn_t`]). This does not run the module's start
/// function; resolve it with [`spacewasm_module_start`] and invoke it with
/// [`spacewasm_invoke`] for that. `allocator` supplies the
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
    allocator: *mut CAllocator,
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
            return status::SPACEWASM_ERR_READER_ERROR;
        }
        Err(e) => return status::parse_status(&e),
    };

    let module_ref = match cengine.engine.push_module(module) {
        Ok(m) => m,
        Err(_) => return status::SPACEWASM_ERR_CAPACITY,
    };

    let idx = module_ref.0 as u32;

    if !out_module_idx.is_null() {
        unsafe { *out_module_idx = idx };
    }
    status::SPACEWASM_OK
}

/// Consume the host module vector `host` and finish it into an engine handle,
/// written to `out_engine`. The engine is sized with a `stack_size`-word guest
/// stack (each word is 4 bytes), room for `max_modules` guest modules (<= 256), and compiles guest
/// modules according to `options` (code-page budget, `memory.grow` support,
/// backpatch bound). No guest module is loaded yet; use
/// [`spacewasm_load_module`] to load one or more.
///
/// `host` may be null to create an engine with no host modules.
///
/// # Ownership of `host`
///
/// The host vector is consumed on every path *except* the two argument-validation
/// failures that are rejected before `host` is read:
///
/// * [`spacewasm_status_t::SPACEWASM_ERR_NULL_ARG`] — `out_engine` is null.
/// * [`spacewasm_status_t::SPACEWASM_ERR_VEC_TOO_LONG`] — `max_modules` exceeds 256.
///
/// On those two the caller still owns `host` and must
/// [`spacewasm_host_destroy`] it. Every other outcome, success or failure,
/// consumes it. Concretely: check for these two codes before deciding whether to
/// destroy the vector.
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
        return status::SPACEWASM_ERR_VEC_TOO_LONG;
    }

    // Take ownership of the host modules (consuming the handle), or start from
    // an empty set when none were supplied.
    let host_modules: Vec<HostModule> = if host.is_null() {
        Vec::zero()
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
    let modules: Vec<HostModule> = unsafe { host.read() }.into();
    drop(modules);
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

/// Look up the start function of module `module_idx` and write its location to
/// `out_module_idx` and `out_func_index`.
///
/// Start functions are never host functions (that is rejected at load time), so
/// the result is always a directly-invokable Wasm function. The written indices
/// can be passed straight to [`spacewasm_invoke`] followed by [`spacewasm_run`],
/// exactly as you would an exported function. Note that `out_module_idx` may
/// differ from `module_idx` when the start is an imported (cross-module)
/// function.
///
/// Returns [`spacewasm_status_t::SPACEWASM_OK`] and populates the outputs when
/// the module declares a start function. Returns
/// [`spacewasm_status_t::SPACEWASM_ERR_NOT_FOUND`] when `module_idx` is out of
/// range or the module has no start function, in which case there is nothing to
/// invoke.
///
/// # Safety
/// `engine` must be live; `out_module_idx` and `out_func_index` valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_module_start(
    engine: *mut CEngine,
    module_idx: u32,
    out_module_idx: *mut u32,
    out_func_index: *mut u32,
) -> spacewasm_status_t {
    let Some(cengine) = (unsafe { engine.as_ref() }) else {
        return status::SPACEWASM_ERR_NULL_ARG;
    };
    if out_module_idx.is_null() || out_func_index.is_null() {
        return status::SPACEWASM_ERR_NULL_ARG;
    }

    if module_idx as usize >= cengine.engine.store.modules().len() {
        return status::SPACEWASM_ERR_NOT_FOUND;
    }

    match cengine.engine.module_start(ModuleRef(module_idx as u8)) {
        Some(start) => {
            unsafe {
                *out_module_idx = start.module.0 as u32;
                *out_func_index = start.index as u32;
            }
            status::SPACEWASM_OK
        }
        None => status::SPACEWASM_ERR_NOT_FOUND,
    }
}

/// Check that function `func_index` of module `module_idx` has the signature
/// described by `params_sig` and `returns_sig`.
///
/// Signatures use the same alphabet as [`spacewasm_add_host_function`]:
/// `i` (i32), `I` (i64), `f` (f32), `d` (f64). For example, a function
/// `(i32, i32) -> i32` matches `params_sig = "ii"`, `returns_sig = "i"`.
///
/// Returns [`spacewasm_status_t::SPACEWASM_OK`] when the signature matches.
/// Returns [`spacewasm_status_t::SPACEWASM_ERR_PARAM_LEN_MISMATCH`] when the
/// parameter or return count differs, and
/// [`spacewasm_status_t::SPACEWASM_ERR_PARAM_TYPE_MISMATCH`] when a type at some
/// position differs. Returns [`spacewasm_status_t::SPACEWASM_ERR_NOT_FOUND`]
/// when `module_idx` or `func_index` is out of range. When a signature string
/// contains a character other than `iIfd` it returns
/// [`spacewasm_status_t::SPACEWASM_ERR_BAD_ARG`], and when it declares more than
/// `MAX_HOST_FUNCTION_PARAMS` entries it returns
/// [`spacewasm_status_t::SPACEWASM_ERR_FUNCTION_PARAMETERS_TOO_LARGE`].
///
/// # Safety
/// `engine` must be live; all C strings valid and NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_check_func_signature(
    engine: *mut CEngine,
    module_idx: u32,
    func_index: u32,
    params_sig: *const c_char,
    returns_sig: *const c_char,
) -> spacewasm_status_t {
    let Some(cengine) = (unsafe { engine.as_ref() }) else {
        return status::SPACEWASM_ERR_NULL_ARG;
    };

    let params_sig = check!(unsafe { cstr(params_sig) });
    let returns_sig = check!(unsafe { cstr(returns_sig) });

    let params =
        check!(spacewasm::HostValList::try_new(params_sig).map_err(status::host_val_list_status));
    let returns =
        check!(spacewasm::HostValList::try_new(returns_sig).map_err(status::host_val_list_status));

    let module = match cengine.engine.store.modules().get(module_idx as usize) {
        Some(m) => m,
        None => return status::SPACEWASM_ERR_NOT_FOUND,
    };

    let func = match module.functions.get(func_index as usize) {
        Some(f) => f,
        None => return status::SPACEWASM_ERR_NOT_FOUND,
    };

    let ty = match module.types.get(func.ty.0 as usize) {
        Some(t) => t,
        None => return status::SPACEWASM_ERR_NOT_FOUND,
    };

    if params.len() != ty.params.len() || returns.len() != ty.returns.len() {
        return status::SPACEWASM_ERR_PARAM_LEN_MISMATCH;
    }

    if params.as_slice() != &ty.params[..] || returns.as_slice() != &ty.returns[..] {
        return status::SPACEWASM_ERR_PARAM_TYPE_MISMATCH;
    }

    status::SPACEWASM_OK
}

/// Look up the global exported as `name` in module `module_idx` and write its
/// index to `out_index`. The written index addresses the module's own globals
/// and can be passed straight to [`spacewasm_get_global`] and
/// [`spacewasm_set_global`].
///
/// Only globals defined by module `module_idx` itself are resolvable this way:
/// if the export re-exports a global imported from another (guest or host)
/// module, this returns [`spacewasm_status_t::SPACEWASM_ERR_NOT_FOUND`], exactly
/// as [`spacewasm_find_export_func`] does for imported functions. Reach such a
/// global through the module that defines it.
///
/// Returns [`spacewasm_status_t::SPACEWASM_ERR_NOT_FOUND`] when `module_idx` is
/// out of range or the module exports no matching, locally-defined global.
///
/// # Safety
/// `engine` must be live; `name` valid; `out_index` valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_find_global(
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
            if let ExportDesc::Global(gi) = e.desc {
                return match module.get_global_ref(gi) {
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

/// Read global `global_index` of module `module_idx` into `out`, tagged with the
/// global's declared value type. `global_index` addresses the module's own
/// globals, as returned by [`spacewasm_find_global`].
///
/// Returns [`spacewasm_status_t::SPACEWASM_ERR_NOT_FOUND`] when `module_idx` or
/// `global_index` is out of range.
///
/// # Safety
/// `engine` must be live; `out` valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_get_global(
    engine: *mut CEngine,
    module_idx: u32,
    global_index: u32,
    out: *mut spacewasm_value_t,
) -> spacewasm_status_t {
    let Some(cengine) = (unsafe { engine.as_ref() }) else {
        return status::SPACEWASM_ERR_NULL_ARG;
    };
    if out.is_null() {
        return status::SPACEWASM_ERR_NULL_ARG;
    }

    let module = match cengine.engine.store.modules().get(module_idx as usize) {
        Some(m) => m,
        None => return status::SPACEWASM_ERR_NOT_FOUND,
    };

    let global = match module.globals.get(global_index as usize) {
        Some(g) => g,
        None => return status::SPACEWASM_ERR_NOT_FOUND,
    };

    unsafe { *out = global.value().into() };
    status::SPACEWASM_OK
}

/// Write `value` into global `global_index` of module `module_idx`.
/// `global_index` addresses the module's own globals, as returned by
/// [`spacewasm_find_global`].
///
/// The tag of `value` must match the global's declared value type, and the
/// global must be mutable.
///
/// Returns [`spacewasm_status_t::SPACEWASM_ERR_NOT_FOUND`] when `module_idx` or
/// `global_index` is out of range,
/// [`spacewasm_status_t::SPACEWASM_ERR_GLOBAL_TYPE_MISMATCH`] when the value type
/// does not match the global, and
/// [`spacewasm_status_t::SPACEWASM_ERR_GLOBAL_NOT_MUTABLE`] when the global is
/// declared `const`.
///
/// # Safety
/// `engine` must be a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_set_global(
    engine: *mut CEngine,
    module_idx: u32,
    global_index: u32,
    value: spacewasm_value_t,
) -> spacewasm_status_t {
    let Some(cengine) = (unsafe { engine.as_mut() }) else {
        return status::SPACEWASM_ERR_NULL_ARG;
    };

    let module = match cengine
        .engine
        .store
        .modules_mut()
        .get_mut(module_idx as usize)
    {
        Some(m) => m,
        None => return status::SPACEWASM_ERR_NOT_FOUND,
    };

    let global = match module.globals.get_mut(global_index as usize) {
        Some(g) => g,
        None => return status::SPACEWASM_ERR_NOT_FOUND,
    };

    let Some(value_val) = value.try_to_value() else {
        return status::SPACEWASM_ERR_BAD_ARG;
    };

    let (ty, raw) = match value_val {
        Value::I32(i) => (ValType::I32, RawValue::from_i32(i)),
        Value::I64(i) => (ValType::I64, RawValue::from_i64(i)),
        Value::F32(f) => (ValType::F32, RawValue::from_f32(f)),
        Value::F64(f) => (ValType::F64, RawValue::from_f64(f)),
    };

    if ty != global.type_.ty {
        return status::SPACEWASM_ERR_GLOBAL_TYPE_MISMATCH;
    }

    if !global.type_.mutable {
        return status::SPACEWASM_ERR_GLOBAL_NOT_MUTABLE;
    }

    global.value = raw;
    status::SPACEWASM_OK
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

    let modules = cengine.engine.store.modules();
    let Some(module) = modules.get(module_idx as usize) else {
        return status::SPACEWASM_ERR_NOT_FOUND;
    };
    if func_index as usize >= module.functions.len() {
        return status::SPACEWASM_ERR_NOT_FOUND;
    }

    let mut buf: [Value; 64] = [Value::I32(0); 64];
    if n > buf.len() {
        return status::SPACEWASM_ERR_CAPACITY;
    }

    if n != 0 {
        let slice = unsafe { core::slice::from_raw_parts(params, n) };
        for (i, v) in slice.iter().enumerate() {
            let Some(val) = v.try_to_value() else {
                return status::SPACEWASM_ERR_BAD_ARG;
            };
            buf[i] = val;
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
    // Clear out the destination before we do any early returns
    if !out_trap.is_null() {
        unsafe { *out_trap = spacewasm_trap_t::SPACEWASM_TRAP_NONE };
    }

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

/// Resume the interpreter from a paused state (no return value).
///
/// Returns [`spacewasm_status_t::SPACEWASM_ERR_WRONG_STATE`] if the engine is not
/// paused, and [`spacewasm_status_t::SPACEWASM_ERR_PARAM_TYPE_MISMATCH`] if the
/// paused host function declared a result — use [`spacewasm_resume_value`]
/// instead. On the latter the pause is preserved, so the call can be retried.
///
/// # Safety
/// `engine` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_resume(engine: *mut CEngine) -> spacewasm_status_t {
    let Some(cengine) = (unsafe { engine.as_mut() }) else {
        return status::SPACEWASM_ERR_NULL_ARG;
    };

    match cengine.engine.resume(None) {
        Ok(()) => status::SPACEWASM_OK,
        Err(e) => status::resume_status(e),
    }
}

/// Resume the interpreter from a paused state.
/// This function will also push a value to the interpreter stack
/// as the return value of the host function that requested a pause.
///
/// Returns [`spacewasm_status_t::SPACEWASM_ERR_BAD_ARG`] if `resume_value.tag` is
/// not a valid [`spacewasm_valtype_t`],
/// [`spacewasm_status_t::SPACEWASM_ERR_WRONG_STATE`] if the engine is not paused,
/// and [`spacewasm_status_t::SPACEWASM_ERR_PARAM_TYPE_MISMATCH`] if the value's
/// type does not match the paused host function's declared result. In the
/// mismatch case the pause is preserved and no stack state changes, so the call
/// can be retried with a correctly typed value.
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

    let Some(value) = resume_value.try_to_value() else {
        return status::SPACEWASM_ERR_BAD_ARG;
    };

    match cengine.engine.resume(Some(value)) {
        Ok(()) => status::SPACEWASM_OK,
        Err(e) => status::resume_status(e),
    }
}

/// Reset the engine back to an idle state, discarding any in-progress or
/// completed call: the program counter, stack pointers, and pending result are
/// cleared, and guest linear memory and the table are reset to their zero
/// state. Loaded modules remain loaded. Use this to abandon a paused or
/// out-of-fuel call, or to run a fresh invocation from a clean slate.
///
/// # Safety
/// `engine` must be a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_reset(engine: *mut CEngine) -> spacewasm_status_t {
    let Some(cengine) = (unsafe { engine.as_mut() }) else {
        return status::SPACEWASM_ERR_NULL_ARG;
    };

    cengine.engine.reset();
    status::SPACEWASM_OK
}

/// Fetch the result of the last completed call, coerced to `expected`, into
/// `out`.
///
/// # Silent type coercion
///
/// The core engine stores a completed call's result as an untagged
/// [`RawValue`]. The function signature must be checked before invoking and the return
/// value must be extracted (in this function) using the proper `expected` type.
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
    let Ok(expected_ty) = ValType::try_from(&expected) else {
        return status::SPACEWASM_ERR_BAD_ARG;
    };
    match cengine
        .engine
        .result
        .map(|raw| spacewasm_value_t::from_raw(raw, expected_ty))
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

/// Report the size of guest linear memory in pages. Intended for use from
/// within a host function.
///
/// # Safety
/// `caller` must be a live caller handle; `out_pages` must be a valid pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_mem_size(
    caller: *mut SpacewasmCaller,
    out_pages: *mut u32,
) -> spacewasm_status_t {
    unsafe { host::mem_size(caller, out_pages) }
}
