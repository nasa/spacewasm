//! FFI-safe status and run-outcome codes. Error mapping lives here so the
//! stable integer ABI is owned by the FFI layer, not the core crate.

use spacewasm::{
    AllocError, HostNameError, HostValListError, InterpreterResult, InvokeError, MemoryError,
    ParseError, TrapReason,
};

/// Operation status returned by most `spacewasm_*` functions.
/// [`spacewasm_status_t::SPACEWASM_OK`] (0) means success.
///
/// Variants are glob-re-exported below so they can be named unqualified within
/// this crate (e.g. `status::SPACEWASM_OK`).
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum spacewasm_status_t {
    SPACEWASM_OK = 0,

    // Generic errors
    SPACEWASM_ERR_NULL_ARG = 1,
    SPACEWASM_ERR_BAD_ARG = 2,
    SPACEWASM_ERR_BAD_UTF8 = 3,
    SPACEWASM_ERR_NAME_TOO_LONG = 4,
    SPACEWASM_ERR_BAD_SIGNATURE = 5,
    SPACEWASM_ERR_CAPACITY = 6,
    SPACEWASM_ERR_NOT_FOUND = 7,
    SPACEWASM_ERR_WRONG_STATE = 8,

    // Allocation errors
    SPACEWASM_ERR_ALLOC_FAILED = 16,
    SPACEWASM_ERR_OUT_OF_MEMORY = 17,
    SPACEWASM_ERR_PAGE_TOO_SMALL = 18,

    // Memory access errors
    SPACEWASM_ERR_MEM_OUT_OF_BOUNDS = 32,

    // Invoke errors
    SPACEWASM_ERR_PARAM_LEN_MISMATCH = 48,
    SPACEWASM_ERR_PARAM_TYPE_MISMATCH = 49,
    SPACEWASM_ERR_STACK_OVERFLOW = 50,

    // Parse / validation errors
    SPACEWASM_ERR_PARSE = 64,

    // Stream / input errors
    SPACEWASM_ERR_STREAM = 80,
}

pub use spacewasm_status_t::*;

/// Outcome of a call to `spacewasm_store_run`.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum spacewasm_run_status_t {
    SPACEWASM_RUN_FINISHED = 0,
    SPACEWASM_RUN_OUT_OF_FUEL = 1,
    SPACEWASM_RUN_PAUSE = 2,
    SPACEWASM_RUN_TRAP = 3,
    SPACEWASM_RUN_READER_ERROR = 4,
}

/// Reason accompanying a trap (`out_trap`). Mirrors [`spacewasm::TrapReason`],
/// with an extra [`SPACEWASM_TRAP_NONE`] (`-1`) written when no trap occurred.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum spacewasm_trap_t {
    /// No trap occurred (the run finished, paused, or ran out of fuel).
    SPACEWASM_TRAP_NONE = -1,
    /// Triggered by an `unreachable` instruction.
    SPACEWASM_TRAP_UNREACHABLE = 0,
    /// A host function noted an unrecoverable failure.
    SPACEWASM_TRAP_HOST = 1,
    /// Integer or floating-point division by zero.
    SPACEWASM_TRAP_DIVIDE_BY_ZERO = 2,
    /// An indirect call index was out of the table's range.
    SPACEWASM_TRAP_INVALID_TABLE_INDEX = 3,
    /// The function type in an indirect call did not match the pointer's type.
    SPACEWASM_TRAP_INVALID_TABLE_FUNCTION_TYPE = 4,
    /// An indirect call referenced an uninitialized table element.
    SPACEWASM_TRAP_UNINITIALIZED_TABLE_ELEMENT = 5,
    /// An imported global could not be read.
    SPACEWASM_TRAP_GLOBAL_GET_FAILED = 6,
    /// An imported global could not be set.
    SPACEWASM_TRAP_GLOBAL_SET_FAILED = 7,
    /// A memory allocation (e.g. `memory.grow`) ran out of memory.
    SPACEWASM_TRAP_OUT_OF_MEMORY = 8,
    /// `memory.grow` failed because a host function holds the memory.
    SPACEWASM_TRAP_MEMORY_REF_NOT_UNIQUE = 9,
    /// A memory operation was out of bounds.
    SPACEWASM_TRAP_MEMORY_OUT_OF_BOUNDS = 10,
    /// Ran out of stack space.
    SPACEWASM_TRAP_STACK_OVERFLOW = 11,
    /// The result of an operation was unrepresentable (e.g. converting Inf).
    SPACEWASM_TRAP_UNREPRESENTABLE_RESULT = 12,
    /// Signed division caused integer overflow.
    SPACEWASM_TRAP_INTEGER_OVERFLOW = 13,
    /// Attempted to convert NaN to an integer.
    SPACEWASM_TRAP_BAD_CONVERSION_TO_INTEGER = 14,
}

pub use spacewasm_trap_t::*;

pub fn trap_reason_code(t: TrapReason) -> spacewasm_trap_t {
    match t {
        TrapReason::Unreachable => SPACEWASM_TRAP_UNREACHABLE,
        TrapReason::Host => SPACEWASM_TRAP_HOST,
        TrapReason::DivideByZero => SPACEWASM_TRAP_DIVIDE_BY_ZERO,
        TrapReason::InvalidTableIndex => SPACEWASM_TRAP_INVALID_TABLE_INDEX,
        TrapReason::InvalidTableFunctionType => SPACEWASM_TRAP_INVALID_TABLE_FUNCTION_TYPE,
        TrapReason::UninitializedTableElement => SPACEWASM_TRAP_UNINITIALIZED_TABLE_ELEMENT,
        TrapReason::GlobalGetFailed => SPACEWASM_TRAP_GLOBAL_GET_FAILED,
        TrapReason::GlobalSetFailed => SPACEWASM_TRAP_GLOBAL_SET_FAILED,
        TrapReason::OutOfMemory => SPACEWASM_TRAP_OUT_OF_MEMORY,
        TrapReason::MemoryRefNotUnique => SPACEWASM_TRAP_MEMORY_REF_NOT_UNIQUE,
        TrapReason::MemoryOutOfBounds => SPACEWASM_TRAP_MEMORY_OUT_OF_BOUNDS,
        TrapReason::StackOverflow => SPACEWASM_TRAP_STACK_OVERFLOW,
        TrapReason::UnrepresentableResult => SPACEWASM_TRAP_UNREPRESENTABLE_RESULT,
        TrapReason::IntegerOverflow => SPACEWASM_TRAP_INTEGER_OVERFLOW,
        TrapReason::BadConversionToInteger => SPACEWASM_TRAP_BAD_CONVERSION_TO_INTEGER,
    }
}

pub fn alloc_status(e: AllocError) -> spacewasm_status_t {
    match e {
        AllocError::AllocationFailed => SPACEWASM_ERR_ALLOC_FAILED,
        AllocError::OutOfMemory => SPACEWASM_ERR_OUT_OF_MEMORY,
        AllocError::PageTooSmall => SPACEWASM_ERR_PAGE_TOO_SMALL,
    }
}

pub fn memory_status(e: MemoryError) -> spacewasm_status_t {
    match e {
        MemoryError::OutOfBounds => SPACEWASM_ERR_MEM_OUT_OF_BOUNDS,
        MemoryError::OutOfMemory => SPACEWASM_ERR_OUT_OF_MEMORY,
        MemoryError::AllocationFailed => SPACEWASM_ERR_ALLOC_FAILED,
        MemoryError::PageTooSmall => SPACEWASM_ERR_PAGE_TOO_SMALL,
    }
}

pub fn invoke_status(e: InvokeError) -> spacewasm_status_t {
    match e {
        InvokeError::ParamLenMismatch => SPACEWASM_ERR_PARAM_LEN_MISMATCH,
        InvokeError::ParamTypeMismatch => SPACEWASM_ERR_PARAM_TYPE_MISMATCH,
        InvokeError::StackOverflow => SPACEWASM_ERR_STACK_OVERFLOW,
    }
}

pub fn parse_status(_e: &ParseError) -> spacewasm_status_t {
    SPACEWASM_ERR_PARSE
}

pub fn host_name_status(_e: HostNameError) -> spacewasm_status_t {
    SPACEWASM_ERR_NAME_TOO_LONG
}

pub fn host_val_list_status(_e: HostValListError) -> spacewasm_status_t {
    SPACEWASM_ERR_BAD_SIGNATURE
}

/// Translate an [`InterpreterResult`] into a run status + trap code.
pub fn run_status(r: &InterpreterResult) -> (spacewasm_run_status_t, spacewasm_trap_t) {
    match r {
        InterpreterResult::Finished => (
            spacewasm_run_status_t::SPACEWASM_RUN_FINISHED,
            SPACEWASM_TRAP_NONE,
        ),
        InterpreterResult::OutOfFuel => (
            spacewasm_run_status_t::SPACEWASM_RUN_OUT_OF_FUEL,
            SPACEWASM_TRAP_NONE,
        ),
        InterpreterResult::Pause => (
            spacewasm_run_status_t::SPACEWASM_RUN_PAUSE,
            SPACEWASM_TRAP_NONE,
        ),
        InterpreterResult::Trap(t) => (
            spacewasm_run_status_t::SPACEWASM_RUN_TRAP,
            trap_reason_code(*t),
        ),
        InterpreterResult::ReaderError(_) => (
            spacewasm_run_status_t::SPACEWASM_RUN_READER_ERROR,
            SPACEWASM_TRAP_NONE,
        ),
    }
}
