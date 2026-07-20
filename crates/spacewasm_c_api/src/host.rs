//! Bridge from spacewasm's Rust host-function closures to C callbacks.

use core::ffi::c_void;
use core::ops::ControlFlow;

use spacewasm::{Engine, HostFunctionBreak, HostFunctionResult, Value};

use crate::engine::{SpacewasmCaller, spacewasm_hostcall_result_t};
use crate::value::spacewasm_value_t;

/// Maximum number of parameters marshalled for a single host call; larger
/// arities trap.
const MAX_HOST_PARAMS: usize = 32;

/// A C function pointer plus its (embedder-owned) user data, stored inside the
/// boxed Rust closure that [`spacewasm::HostFunction`] expects.
pub(crate) struct CHostFunction {
    f: unsafe extern "C" fn(
        *mut SpacewasmCaller,
        *mut c_void,
        *const spacewasm_value_t,
        usize,
        *mut spacewasm_value_t,
    ) -> spacewasm_hostcall_result_t,
    userdata: *mut c_void,
}

impl CHostFunction {
    pub(crate) fn new(
        f: unsafe extern "C" fn(
            *mut SpacewasmCaller,
            *mut c_void,
            *const spacewasm_value_t,
            usize,
            *mut spacewasm_value_t,
        ) -> spacewasm_hostcall_result_t,
        userdata: *mut c_void,
    ) -> CHostFunction {
        CHostFunction { f, userdata }
    }

    /// Invoke the C callback for a guest→host call: marshal the arguments,
    /// expose the state as an opaque caller handle, and translate the result
    /// back into spacewasm's `ControlFlow` outcome.
    pub(crate) fn call(&self, state: &Engine, args: &[Value]) -> HostFunctionResult {
        if args.len() > MAX_HOST_PARAMS {
            return ControlFlow::Break(HostFunctionBreak::Trap);
        }

        // Marshal parameters into a fixed C-layout buffer.
        let mut params: [spacewasm_value_t; MAX_HOST_PARAMS] = [spacewasm_value_t {
            tag: crate::value::spacewasm_valtype_t::SPACEWASM_I32,
            u: crate::value::spacewasm_value_payload_t { i32_: 0 },
        }; MAX_HOST_PARAMS];
        for (i, v) in args.iter().enumerate() {
            params[i] = spacewasm_value_t::from_value(*v);
        }

        let mut out_result = spacewasm_value_t {
            tag: crate::value::spacewasm_valtype_t::SPACEWASM_I32,
            u: crate::value::spacewasm_value_payload_t { i32_: 0 },
        };

        // Expose the borrowed state as an opaque caller pointer. The callback
        // may only use it for the duration of this call.
        let caller = state as *const Engine as *mut SpacewasmCaller;

        // SAFETY: `f` is a valid C function pointer supplied at registration.
        let outcome = unsafe {
            (self.f)(
                caller,
                self.userdata,
                params.as_ptr(),
                args.len(),
                &mut out_result,
            )
        };

        match outcome {
            spacewasm_hostcall_result_t::SPACEWASM_CONTINUE => {
                ControlFlow::Continue(Some(out_result.to_value()))
            }
            spacewasm_hostcall_result_t::SPACEWASM_TRAP => {
                ControlFlow::Break(HostFunctionBreak::Trap)
            }
            spacewasm_hostcall_result_t::SPACEWASM_PAUSE => {
                ControlFlow::Break(HostFunctionBreak::Pause)
            }
        }
    }
}

/// Read guest linear memory into a destination buffer from within a host
/// callback. Returns a status code.
///
/// # Safety
/// `caller` must be a live caller handle; `dst` must be valid for `len` bytes.
pub unsafe fn mem_read(
    caller: *const SpacewasmCaller,
    addr: u32,
    dst: *mut u8,
    len: usize,
) -> crate::status::spacewasm_status_t {
    let Some(state) = (unsafe { SpacewasmCaller::state(caller) }) else {
        return crate::status::SPACEWASM_ERR_NULL_ARG;
    };
    if dst.is_null() && len != 0 {
        return crate::status::SPACEWASM_ERR_NULL_ARG;
    }
    match state.memory.load(addr as usize, len) {
        Ok(src) => {
            // SAFETY: caller guarantees dst is valid for len bytes; src is a
            // valid slice of len bytes from guest memory.
            unsafe { core::ptr::copy_nonoverlapping(src.as_ptr(), dst, len) };
            crate::status::SPACEWASM_OK
        }
        Err(e) => crate::status::memory_status(e),
    }
}

/// Write a source buffer into guest linear memory from within a host callback.
///
/// # Safety
/// `caller` must be a live caller handle; `src` must be valid for `len` bytes.
pub unsafe fn mem_write(
    caller: *const SpacewasmCaller,
    addr: u32,
    src: *const u8,
    len: usize,
) -> crate::status::spacewasm_status_t {
    let Some(state) = (unsafe { SpacewasmCaller::state(caller) }) else {
        return crate::status::SPACEWASM_ERR_NULL_ARG;
    };
    if src.is_null() && len != 0 {
        return crate::status::SPACEWASM_ERR_NULL_ARG;
    }
    // SAFETY: caller guarantees src is valid for len bytes.
    let data = unsafe { core::slice::from_raw_parts(src, len) };
    match state.memory.store(addr as usize, data) {
        Ok(()) => crate::status::SPACEWASM_OK,
        Err(e) => crate::status::memory_status(e),
    }
}

/// Report the size of guest linear memory in pages, from within a host callback.
///
/// # Safety
/// `caller` must be a live caller handle; `out_pages` must be a valid pointer.
pub unsafe fn mem_size(
    caller: *const SpacewasmCaller,
    out_pages: *mut u32,
) -> crate::status::spacewasm_status_t {
    let Some(state) = (unsafe { SpacewasmCaller::state(caller) }) else {
        return crate::status::SPACEWASM_ERR_NULL_ARG;
    };
    if out_pages.is_null() {
        return crate::status::SPACEWASM_ERR_NULL_ARG;
    }
    // SAFETY: out_pages is non-null and valid per the contract.
    unsafe { *out_pages = state.memory.size() };
    crate::status::SPACEWASM_OK
}
