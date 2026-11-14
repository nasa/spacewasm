//! The concrete `extern "C"` entry points (`spacewasm_*`) that make up the ABI.

use core::ffi::c_char;
use core::ffi::c_void;

use spacewasm::{
    CodeBuilder, CompilerOptions, ControlFrame, DecoderStack, Engine, ExportDesc, HostFunction,
    HostModule, Interpreter, InterpreterRunner, Module, ModuleRef, OperandType, RawValue, Ref,
    ValType, Value, Vec, WasmRef,
};

use crate::SpacewasmCaller;
use crate::alloc::CAllocator;
use crate::alloc::{self, spacewasm_alloc_fn_t, spacewasm_dealloc_fn_t, spacewasm_realloc_fn_t};
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

/// Compiler options for guest modules. Passed to [`spacewasm_new`].
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct spacewasm_compiler_options_t {
    /// Allow `memory.grow`; when `false`, modules using it are rejected.
    pub allow_memory_grow: bool,

    /// Bound on control-flow backpatch iterations; 0 for unlimited.
    pub max_backpatch_iterations: u32,

    /// Maximum compiled code pages across all modules on the engine.
    pub max_code_pages: usize,
}

impl From<spacewasm_compiler_options_t> for CompilerOptions {
    fn from(o: spacewasm_compiler_options_t) -> Self {
        CompilerOptions::new(
            o.allow_memory_grow,
            (o.max_backpatch_iterations != 0).then_some(o.max_backpatch_iterations),
            o.max_code_pages,
        )
    }
}

/// Opaque storage for one validator control frame.
#[repr(C)]
pub struct spacewasm_control_frame_t {
    _opaque: [u32; 3],
}

/// Opaque storage for one validator operand.
#[repr(C)]
pub struct spacewasm_operand_t {
    _opaque: [u8; 2],
}

// The opaque types stand in for the core crate's, so they must agree in layout.
const _: () = {
    use core::mem::{align_of, size_of};
    assert!(size_of::<spacewasm_control_frame_t>() == size_of::<ControlFrame>());
    assert!(align_of::<spacewasm_control_frame_t>() >= align_of::<ControlFrame>());
    assert!(size_of::<spacewasm_operand_t>() == size_of::<OperandType>());
    assert!(align_of::<spacewasm_operand_t>() >= align_of::<OperandType>());
};

/// Validator scratch storage, borrowed by [`spacewasm_load_module`] for the
/// call. Need not be initialized.
///
/// `control_frames_len` bounds block nesting and `operands_len` the operand
/// stack, per function; exceeding either fails with
/// [`spacewasm_status_t::SPACEWASM_ERR_OUT_OF_MEMORY`]. A pointer may be null
/// only if its length is 0.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct spacewasm_decoder_stack_t {
    pub control_frames: *mut spacewasm_control_frame_t,
    pub control_frames_len: usize,
    pub operands: *mut spacewasm_operand_t,
    pub operands_len: usize,
}

/// Initialize `len` slots at `ptr` to `value` and borrow them as a slice.
///
/// # Safety
/// `ptr` is null with `len == 0`, or valid for writes of `len` `T`s and
/// unaliased for `'a`.
unsafe fn init_slice<'a, T: Copy>(
    ptr: *mut T,
    len: usize,
    value: T,
) -> Result<&'a mut [T], spacewasm_status_t> {
    if len == 0 {
        return Ok(&mut []);
    }
    if ptr.is_null() {
        return Err(status::SPACEWASM_ERR_NULL_ARG);
    }
    // SAFETY: per the contract. Every slot is written before the slice is
    // formed, so C may hand over uninitialized memory.
    unsafe {
        for i in 0..len {
            ptr.add(i).write(value);
        }
        Ok(core::slice::from_raw_parts_mut(ptr, len))
    }
}

/// An interpreter engine and its compiled code.
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

/// Create a guest linear-memory allocator from C callbacks, each passed
/// `userdata`. Returns null if a callback is null or allocation fails.
#[unsafe(no_mangle)]
pub extern "C" fn spacewasm_allocator_new(
    alloc: spacewasm_alloc_fn_t,
    realloc: spacewasm_realloc_fn_t,
    dealloc: spacewasm_dealloc_fn_t,
    userdata: *mut c_void,
) -> *mut CAllocator {
    alloc::allocator_new(alloc, realloc, dealloc, userdata)
}

/// Destroy an allocator handle. No-op on null. Modules already loaded keep
/// their own reference.
///
/// # Safety
/// `allocator` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_allocator_destroy(allocator: *mut CAllocator) {
    unsafe { alloc::allocator_destroy(allocator) }
}

#[repr(C)]
pub struct spacewasm_host_t {
    ptr: *mut HostModule,
    capacity: usize,
    len: usize,
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

/// Create a host module vector with room for `len` modules.
///
/// # Safety
/// `dest` must be null or valid for writes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_host_new(
    len: usize,
    dest: *mut spacewasm_host_t,
) -> spacewasm_status_t {
    if dest.is_null() {
        return status::SPACEWASM_ERR_NULL_ARG;
    }

    let v = check!(Vec::new(len).map_err(status::alloc_status));
    unsafe { *dest = v.into() };
    status::SPACEWASM_OK
}

/// Add a host module named `name` with room for `max_functions` functions and
/// `max_globals` globals. Writes its index to `out_idx` if non-null.
///
/// # Safety
/// `host` must be live; strings valid and NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_add_host_module(
    host: *mut spacewasm_host_t,
    name: *const c_char,
    max_functions: usize,
    max_globals: usize,
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

/// Add host function `name` to host module `module_idx`, implemented by `f`
/// (passed `userdata`). Signatures use `i` (i32), `I` (i64), `f` (f32),
/// `d` (f64).
///
/// # Safety
/// `host` must be live; strings valid and NUL-terminated.
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

/// Load a guest module named `name`, streamed through `read`. `allocator`
/// backs its linear memory and `decoder_stack` is the validator's scratch
/// storage. Writes the module index to `out_module_idx` if non-null. Does not
/// run the start function (see [`spacewasm_module_start`]).
///
/// Any failure leaves the engine unusable: destroy it and create a new one.
///
/// # Safety
/// `engine` and `allocator` must be live; `decoder_stack` must describe
/// writable, unaliased arrays; `out_module_idx` null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_load_module(
    engine: *mut CEngine,
    name: *const c_char,
    read: spacewasm_read_fn_t,
    read_userdata: *mut c_void,
    allocator: *mut CAllocator,
    decoder_stack: spacewasm_decoder_stack_t,
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

    // SAFETY: the opaque element types match the core ones in size and
    // alignment (checked above), and the arrays are valid per the contract.
    let control_frames = check!(unsafe {
        init_slice(
            decoder_stack.control_frames.cast::<ControlFrame>(),
            decoder_stack.control_frames_len,
            ControlFrame::EMPTY,
        )
    });
    let operands = check!(unsafe {
        init_slice(
            decoder_stack.operands.cast::<OperandType>(),
            decoder_stack.operands_len,
            OperandType::EMPTY,
        )
    });
    let mut decoder_stack = DecoderStack::new(control_frames, operands);
    let module = match Module::new(
        name,
        &mut stream,
        &mut cengine.engine.store,
        &mut cengine.code_builder,
        &mut decoder_stack,
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

/// Create an engine from `host` (may be null), with a `stack_size`-word stack
/// and room for `max_modules` (<= 256) guest modules.
///
/// `host` is consumed, except on [`spacewasm_status_t::SPACEWASM_ERR_NULL_ARG`]
/// and [`spacewasm_status_t::SPACEWASM_ERR_VEC_TOO_LONG`], when the caller still
/// owns it.
///
/// # Safety
/// `host` null or a live, unconsumed handle; `out_engine` valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_new(
    host: *mut spacewasm_host_t,
    stack_size: usize,
    max_modules: usize,
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

    let engine =
        check!(Engine::new(stack_size, max_modules, host_modules).map_err(status::memory_status));

    let code_builder = check!(CodeBuilder::new(options.into()).map_err(status::alloc_status));

    let cengine = CEngine {
        engine,
        code_builder,
    };

    let boxed = check!(spacewasm::Box::new(cengine).map_err(status::alloc_status));
    unsafe { *out_engine = spacewasm::Box::leak(boxed) as *mut CEngine };
    status::SPACEWASM_OK
}

/// Destroy a host vector that was not consumed by [`spacewasm_new`]. No-op on
/// null.
///
/// # Safety
/// `host` must be null or a live, unconsumed handle.
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

/// Find the module named `name`.
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

/// Find the function exported as `name` from module `module_idx`.
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

/// Find the start function of module `module_idx`, ready for
/// [`spacewasm_invoke`]. `out_module_idx` may differ from `module_idx` if the
/// start function is imported. Returns
/// [`spacewasm_status_t::SPACEWASM_ERR_NOT_FOUND`] if there is none.
///
/// # Safety
/// `engine` must be live; outputs valid.
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
/// `params_sig` -> `returns_sig` (same alphabet as
/// [`spacewasm_add_host_function`]).
///
/// # Safety
/// `engine` must be live; strings valid and NUL-terminated.
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

/// Find the global exported as `name` from module `module_idx`. Only globals
/// the module defines itself are found, not re-exported imports.
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

/// Read global `global_index` of module `module_idx` into `out`.
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

/// Write `value` to mutable global `global_index` of module `module_idx`. The
/// value's type must match the global's.
///
/// # Safety
/// `engine` must be live.
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

/// Prepare a call to function `func_index` of module `module_idx` with the `n`
/// arguments in `params`. Run it with [`spacewasm_run`].
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

/// Run the pending call for up to `fuel` instructions, writing any trap to
/// `out_trap`. A paused engine stays paused until resumed.
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

/// Resume after a pause from a host function with no result.
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

/// Resume after a pause, supplying the host function's result. On a type
/// mismatch the pause is kept, so the call can be retried.
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

/// Abandon any call and reset execution state, linear memory and the table.
/// Loaded modules stay loaded.
///
/// # Safety
/// `engine` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_reset(engine: *mut CEngine) -> spacewasm_status_t {
    let Some(cengine) = (unsafe { engine.as_mut() }) else {
        return status::SPACEWASM_ERR_NULL_ARG;
    };

    cengine.engine.reset();
    status::SPACEWASM_OK
}

/// Read the last completed call's result as type `expected`. The result is
/// untagged, so `expected` must match the function's return type.
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

/// Copy `backtrace` into the C output buffer as described for
/// [`spacewasm_backtrace`]. `out_len` must already be checked non-null.
///
/// # Safety
/// `out_pcs` must be valid for `capacity` writes (or null with `capacity == 0`);
/// `out_len` must be valid.
pub(crate) unsafe fn write_backtrace(
    backtrace: spacewasm::Backtrace<'_>,
    out_pcs: *mut u32,
    capacity: usize,
    out_len: *mut usize,
) -> spacewasm_status_t {
    let mut depth = 0usize;
    for pc in backtrace {
        if depth < capacity {
            // SAFETY: `out_pcs` is valid for `capacity` writes per the contract.
            unsafe { *out_pcs.add(depth) = pc.0 };
        }
        depth += 1;
    }

    unsafe { *out_len = depth };
    if depth > capacity {
        status::SPACEWASM_ERR_CAPACITY
    } else {
        status::SPACEWASM_OK
    }
}

/// Write the call stack's IR PCs to `out_pcs`, innermost first, and its depth to
/// `out_len`. Entry 0 is the current instruction (the faulting one after a trap,
/// the host `call` while paused); each later entry is a `call` instruction, not
/// a return address.
///
/// Writes at most `capacity` entries, returning
/// [`spacewasm_status_t::SPACEWASM_ERR_CAPACITY`] if truncated. Returns
/// [`spacewasm_status_t::SPACEWASM_ERR_WRONG_STATE`] if there is no call stack.
///
/// # Safety
/// `engine` must be live; `out_pcs` valid for `capacity` entries (or null if
/// 0); `out_len` valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_backtrace(
    engine: *const CEngine,
    out_pcs: *mut u32,
    capacity: usize,
    out_len: *mut usize,
) -> spacewasm_status_t {
    let Some(cengine) = (unsafe { engine.as_ref() }) else {
        return status::SPACEWASM_ERR_NULL_ARG;
    };
    if out_len.is_null() || (out_pcs.is_null() && capacity != 0) {
        return status::SPACEWASM_ERR_NULL_ARG;
    }

    match cengine.engine.backtrace() {
        Some(bt) => unsafe { write_backtrace(bt, out_pcs, capacity, out_len) },
        None => status::SPACEWASM_ERR_WRONG_STATE,
    }
}

/// Destroy an engine. No-op on null.
///
/// # Safety
/// `engine` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_destroy(engine: *mut CEngine) {
    if engine.is_null() {
        return;
    }
    let _ = unsafe { spacewasm::Box::from_raw(spacewasm::GlobalAllocator, engine) };
}

/// Read `len` bytes of guest memory at `addr` into `dst`, from a host function.
///
/// # Safety
/// `caller` must be live; `dst` valid for `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_mem_read(
    caller: *mut SpacewasmCaller,
    addr: u32,
    dst: *mut u8,
    len: usize,
) -> spacewasm_status_t {
    unsafe { host::mem_read(caller, addr, dst, len) }
}

/// Write `len` bytes from `src` to guest memory at `addr`, from a host function.
///
/// # Safety
/// `caller` must be live; `src` valid for `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_mem_write(
    caller: *mut SpacewasmCaller,
    addr: u32,
    src: *const u8,
    len: usize,
) -> spacewasm_status_t {
    unsafe { host::mem_write(caller, addr, src, len) }
}

/// Write the guest memory size in pages to `out_pages`, from a host function.
///
/// # Safety
/// `caller` must be live; `out_pages` valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn spacewasm_mem_size(
    caller: *mut SpacewasmCaller,
    out_pages: *mut u32,
) -> spacewasm_status_t {
    unsafe { host::mem_size(caller, out_pages) }
}
