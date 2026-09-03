//! FFI-safe status and run-outcome codes. Error mapping lives here so the
//! stable integer ABI is owned by the FFI layer, not the core crate.

use spacewasm::{
    AllocError, ConstantExprError, HostFunctionError, HostNameError, InterpreterResult,
    InvokeError, MemoryError, ParseError, TrapReason, ValidationError,
};

/// Operation status returned by most `spacewasm_*` functions.
/// [`spacewasm_status_t::SPACEWASM_OK`] (0) means success.
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
    SPACEWASM_ERR_GUEST_MEMORY_ALLOC_FAILED = 15,
    SPACEWASM_ERR_ALLOC_FAILED = 16,
    SPACEWASM_ERR_OUT_OF_MEMORY = 17,
    SPACEWASM_ERR_PAGE_TOO_SMALL = 18,

    // Memory access errors
    SPACEWASM_ERR_MEM_OUT_OF_BOUNDS = 32,

    // Invoke errors
    SPACEWASM_ERR_PARAM_LEN_MISMATCH = 48,
    SPACEWASM_ERR_PARAM_TYPE_MISMATCH = 49,
    SPACEWASM_ERR_STACK_OVERFLOW = 50,

    // Parse / validation errors - Basic parsing
    SPACEWASM_ERR_EOF = 64,
    SPACEWASM_ERR_MALFORMED_INTEGER = 65,
    SPACEWASM_ERR_I33_IS_NEGATIVE = 66,
    SPACEWASM_ERR_MALFORMED_MAGIC = 67,
    SPACEWASM_ERR_MALFORMED_VERSION = 68,
    SPACEWASM_ERR_MALFORMED_UTF8 = 69,
    SPACEWASM_ERR_DUPLICATE_MODULE_NAME = 70,
    SPACEWASM_ERR_DUPLICATE_EXPORT_NAME = 71,
    SPACEWASM_ERR_MALFORMED_SECTION_ID = 72,
    SPACEWASM_ERR_MALFORMED_VALUE_TYPE = 73,
    SPACEWASM_ERR_MALFORMED_FUNCTION = 74,
    SPACEWASM_ERR_MALFORMED_LIMIT = 75,
    SPACEWASM_ERR_MALFORMED_ELEM_TYPE = 76,
    SPACEWASM_ERR_MALFORMED_SECTION_SIZE = 77,
    SPACEWASM_ERR_EXPECTED_CONST_OR_VAR = 78,
    SPACEWASM_ERR_MALFORMED_IMPORT_EXPORT_DESC = 79,
    SPACEWASM_ERR_MALFORMED_MEM_TYPE = 80,
    SPACEWASM_ERR_INVALID_PAGE_SIZE = 81,
    SPACEWASM_ERR_INVALID_SECTION_ORDERING = 82,
    SPACEWASM_ERR_DUPLICATE_SECTION = 83,
    SPACEWASM_ERR_INVALID_MAX_LIMIT = 84,
    SPACEWASM_ERR_EXPECTED_TERMINAL = 85,
    SPACEWASM_ERR_INVALID_OPCODE = 86,
    SPACEWASM_ERR_MALFORMED_CODE_SIZE = 87,
    SPACEWASM_ERR_INVALID_CODE_SECTION_FUNCTION_COUNT = 88,
    SPACEWASM_ERR_VEC_TOO_LONG = 89,
    SPACEWASM_ERR_IDX_TOO_LARGE = 90,
    SPACEWASM_ERR_MODULE_IDX_TOO_LARGE = 91,
    SPACEWASM_ERR_MEMORY_TOO_LARGE = 92,
    SPACEWASM_ERR_MEMORY_IMPORT_TOO_LARGE = 93,
    SPACEWASM_ERR_MEM_ALIGN_TOO_LARGE = 94,
    SPACEWASM_ERR_TABLE_TOO_LARGE = 95,

    // Parse / validation errors - Control flow validation
    SPACEWASM_ERR_CONTROL_FLOW_TOO_DEEP = 96,
    SPACEWASM_ERR_STACK_UNDERFLOW = 97,
    SPACEWASM_ERR_STACK_TOO_LARGE = 98,
    SPACEWASM_ERR_LABEL_STACK_JUMP_TOO_DEEP = 99,
    SPACEWASM_ERR_LABEL_JUMP_TOO_LARGE = 100,
    SPACEWASM_ERR_TYPE_MISMATCH = 101,
    SPACEWASM_ERR_BLOCK_RESULT_TYPE_MISMATCH = 102,
    SPACEWASM_ERR_FUNCTION_RESULT_TYPE_MISMATCH = 103,
    SPACEWASM_ERR_BR_TABLE_RESULT_TYPE_MISMATCH = 104,

    // Parse / validation errors - Memory and table validation
    SPACEWASM_ERR_ILLEGAL_MEMORY_GROW = 112,
    SPACEWASM_ERR_INVALID_ELEMENT_OFFSET = 113,
    SPACEWASM_ERR_INVALID_ELEMENT_OUT_OF_BOUNDS = 114,
    SPACEWASM_ERR_INVALID_TABLE_INDEX = 115,
    SPACEWASM_ERR_TABLE_NOT_DEFINED = 116,
    SPACEWASM_ERR_INVALID_ELEMENT_COUNT = 117,
    SPACEWASM_ERR_INVALID_MEM_INDEX = 118,
    SPACEWASM_ERR_MEMORY_NOT_DEFINED = 119,
    SPACEWASM_ERR_INVALID_MEM_OFFSET_TYPE = 120,
    SPACEWASM_ERR_INVALID_NEGATIVE_MEM_OFFSET = 121,
    SPACEWASM_ERR_INVALID_MEM_OFFSET = 122,
    SPACEWASM_ERR_MULTIPLE_MEMORIES = 123,
    SPACEWASM_ERR_MULTIPLE_TABLES = 124,

    // Parse / validation errors - Index validation
    SPACEWASM_ERR_INVALID_LABEL_INDEX = 128,
    SPACEWASM_ERR_INVALID_ELSE_BLOCK = 129,
    SPACEWASM_ERR_INVALID_END_BLOCK = 130,
    SPACEWASM_ERR_INSTRUCTION_OUTSIDE_OF_FUNCTION = 131,
    SPACEWASM_ERR_LOCAL_IDX_OUT_OF_RANGE = 132,
    SPACEWASM_ERR_FUNCTION_IDX_OUT_OF_RANGE = 133,
    SPACEWASM_ERR_TYPE_IDX_OUT_OF_RANGE = 134,
    SPACEWASM_ERR_FUNCTION_TEXT_OUT_OF_RANGE = 135,
    SPACEWASM_ERR_GLOBAL_IDX_OUT_OF_RANGE = 136,

    // Parse / validation errors - Import validation
    SPACEWASM_ERR_FUNCTION_IMPORT_NOT_FOUND = 144,
    SPACEWASM_ERR_GLOBAL_IMPORT_NOT_FOUND = 145,
    SPACEWASM_ERR_MEMORY_IMPORT_NOT_FOUND = 146,
    SPACEWASM_ERR_TABLE_IMPORT_NOT_FOUND = 147,
    SPACEWASM_ERR_FUNCTION_IMPORT_OUT_OF_RANGE = 148,
    SPACEWASM_ERR_FUNCTION_IMPORT_TYPE_MISMATCH = 149,
    SPACEWASM_ERR_GLOBAL_NOT_MUTABLE = 150,
    SPACEWASM_ERR_GLOBAL_IMPORT_TYPE_MISMATCH = 151,
    SPACEWASM_ERR_MEMORY_IMPORT_TYPE_MISMATCH = 152,
    SPACEWASM_ERR_TABLE_IMPORT_TYPE_MISMATCH = 153,
    SPACEWASM_ERR_TABLE_IMPORT_INCOMPATIBLE_SIZE = 154,
    SPACEWASM_ERR_TABLE_REF_NOT_UNIQUE = 155,

    // Parse / validation errors - Function and global validation
    SPACEWASM_ERR_FUNCTION_PARAMETERS_TOO_LARGE = 160,
    SPACEWASM_ERR_FUNCTION_RETURNS_TOO_LARGE = 161,
    SPACEWASM_ERR_TOO_MANY_LOCALS = 162,
    SPACEWASM_ERR_INVALID_CONST_INSTRUCTION = 163,
    SPACEWASM_ERR_GLOBAL_TYPE_MISMATCH = 164,
    SPACEWASM_ERR_ALIGNMENT_LARGER_THAN_TYPE = 165,
    SPACEWASM_ERR_INVALID_START_FUNCTION_SIGNATURE = 166,
    SPACEWASM_ERR_INVALID_HOST_START_FUNCTION = 167,

    // Parse / validation errors - Constant expression validation
    SPACEWASM_ERR_CONST_ALREADY_HAS_VALUE = 176,
    SPACEWASM_ERR_CONST_NO_VALUE = 177,
    SPACEWASM_ERR_CONST_INVALID_GLOBAL = 178,

    // Parse / validation errors - Miscellaneous
    SPACEWASM_ERR_POSSIBLE_BACKPATCH_CYCLE = 192,
    SPACEWASM_ERR_PAGE_FAULT = 193,
    SPACEWASM_ERR_READER_ERROR = 194,
}

pub use spacewasm_status_t::*;

/// Outcome of a call to `spacewasm_run`.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum spacewasm_run_status_t {
    SPACEWASM_RUN_FINISHED = 0,
    SPACEWASM_RUN_OUT_OF_FUEL = 1,
    SPACEWASM_RUN_PAUSE = 2,
    SPACEWASM_RUN_TRAP = 3,
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
        InvokeError::Busy => SPACEWASM_ERR_WRONG_STATE,
    }
}

pub fn validation_status(e: &ValidationError) -> spacewasm_status_t {
    match e {
        ValidationError::Eof => SPACEWASM_ERR_EOF,
        ValidationError::MalformedInteger => SPACEWASM_ERR_MALFORMED_INTEGER,
        ValidationError::I33IsNegative => SPACEWASM_ERR_I33_IS_NEGATIVE,
        ValidationError::MalformedMagic => SPACEWASM_ERR_MALFORMED_MAGIC,
        ValidationError::MalformedVersion => SPACEWASM_ERR_MALFORMED_VERSION,
        ValidationError::MalformedUtf8 => SPACEWASM_ERR_MALFORMED_UTF8,
        ValidationError::DuplicateModuleName => SPACEWASM_ERR_DUPLICATE_MODULE_NAME,
        ValidationError::DuplicateExportName => SPACEWASM_ERR_DUPLICATE_EXPORT_NAME,
        ValidationError::MalformedSectionId(_) => SPACEWASM_ERR_MALFORMED_SECTION_ID,
        ValidationError::MalformedValueType(_) => SPACEWASM_ERR_MALFORMED_VALUE_TYPE,
        ValidationError::MalformedFunction(_) => SPACEWASM_ERR_MALFORMED_FUNCTION,
        ValidationError::MalformedLimit(_) => SPACEWASM_ERR_MALFORMED_LIMIT,
        ValidationError::MalformedElemType(_) => SPACEWASM_ERR_MALFORMED_ELEM_TYPE,
        ValidationError::MalformedSectionSize => SPACEWASM_ERR_MALFORMED_SECTION_SIZE,
        ValidationError::ExpectedConstOrVar(_) => SPACEWASM_ERR_EXPECTED_CONST_OR_VAR,
        ValidationError::MalformedImportExportDesc(_) => SPACEWASM_ERR_MALFORMED_IMPORT_EXPORT_DESC,
        ValidationError::MalformedMemType(_) => SPACEWASM_ERR_MALFORMED_MEM_TYPE,
        ValidationError::InvalidPageSize(_) => SPACEWASM_ERR_INVALID_PAGE_SIZE,
        ValidationError::InvalidSectionOrdering(_, _) => SPACEWASM_ERR_INVALID_SECTION_ORDERING,
        ValidationError::DuplicateSection(_) => SPACEWASM_ERR_DUPLICATE_SECTION,
        ValidationError::InvalidMaxLimit => SPACEWASM_ERR_INVALID_MAX_LIMIT,
        ValidationError::ExpectedTerminal(_) => SPACEWASM_ERR_EXPECTED_TERMINAL,
        ValidationError::InvalidOpcode(_) => SPACEWASM_ERR_INVALID_OPCODE,
        ValidationError::MalformedCodeSize => SPACEWASM_ERR_MALFORMED_CODE_SIZE,
        ValidationError::InvalidCodeSectionFunctionCount => {
            SPACEWASM_ERR_INVALID_CODE_SECTION_FUNCTION_COUNT
        }
        ValidationError::VecTooLong => SPACEWASM_ERR_VEC_TOO_LONG,
        ValidationError::IdxTooLarge => SPACEWASM_ERR_IDX_TOO_LARGE,
        ValidationError::ModuleIdxTooLarge => SPACEWASM_ERR_MODULE_IDX_TOO_LARGE,
        ValidationError::MemoryTooLarge => SPACEWASM_ERR_MEMORY_TOO_LARGE,
        ValidationError::TableTooLarge => SPACEWASM_ERR_TABLE_TOO_LARGE,
        ValidationError::MemoryImportTooLarge => SPACEWASM_ERR_MEMORY_IMPORT_TOO_LARGE,
        ValidationError::MemAlignTooLarge => SPACEWASM_ERR_MEM_ALIGN_TOO_LARGE,
        ValidationError::ControlFlowTooDeep => SPACEWASM_ERR_CONTROL_FLOW_TOO_DEEP,
        ValidationError::StackUnderflow => SPACEWASM_ERR_STACK_UNDERFLOW,
        ValidationError::StackTooLarge => SPACEWASM_ERR_STACK_TOO_LARGE,
        ValidationError::LabelStackJumpTooDeep => SPACEWASM_ERR_LABEL_STACK_JUMP_TOO_DEEP,
        ValidationError::LabelJumpTooLarge => SPACEWASM_ERR_LABEL_JUMP_TOO_LARGE,
        ValidationError::TypeMismatch => SPACEWASM_ERR_TYPE_MISMATCH,
        ValidationError::BlockResultTypeMismatch => SPACEWASM_ERR_BLOCK_RESULT_TYPE_MISMATCH,
        ValidationError::BrTableResultTypeMismatch => SPACEWASM_ERR_BR_TABLE_RESULT_TYPE_MISMATCH,
        ValidationError::FunctionResultTypeMismatch => SPACEWASM_ERR_FUNCTION_RESULT_TYPE_MISMATCH,
        ValidationError::IllegalMemoryGrow => SPACEWASM_ERR_ILLEGAL_MEMORY_GROW,
        ValidationError::InvalidElementOffset => SPACEWASM_ERR_INVALID_ELEMENT_OFFSET,
        ValidationError::InvalidElementOutOfBounds => SPACEWASM_ERR_INVALID_ELEMENT_OUT_OF_BOUNDS,
        ValidationError::InvalidTableIndex => SPACEWASM_ERR_INVALID_TABLE_INDEX,
        ValidationError::TableNotDefined => SPACEWASM_ERR_TABLE_NOT_DEFINED,
        ValidationError::InvalidElementCount => SPACEWASM_ERR_INVALID_ELEMENT_COUNT,
        ValidationError::InvalidMemIndex => SPACEWASM_ERR_INVALID_MEM_INDEX,
        ValidationError::MemoryNotDefined => SPACEWASM_ERR_MEMORY_NOT_DEFINED,
        ValidationError::InvalidMemOffsetType => SPACEWASM_ERR_INVALID_MEM_OFFSET_TYPE,
        ValidationError::InvalidNegativeMemOffset => SPACEWASM_ERR_INVALID_NEGATIVE_MEM_OFFSET,
        ValidationError::InvalidMemOffset => SPACEWASM_ERR_INVALID_MEM_OFFSET,
        ValidationError::InvalidLabelIndex => SPACEWASM_ERR_INVALID_LABEL_INDEX,
        ValidationError::InvalidElseBlock => SPACEWASM_ERR_INVALID_ELSE_BLOCK,
        ValidationError::InvalidEndBlock => SPACEWASM_ERR_INVALID_END_BLOCK,
        ValidationError::MultipleMemories => SPACEWASM_ERR_MULTIPLE_MEMORIES,
        ValidationError::MultipleTables => SPACEWASM_ERR_MULTIPLE_TABLES,
        ValidationError::PossibleBackpatchCycle => SPACEWASM_ERR_POSSIBLE_BACKPATCH_CYCLE,
        ValidationError::PageFault => SPACEWASM_ERR_PAGE_FAULT,
        ValidationError::InstructionOutsideOfFunction => {
            SPACEWASM_ERR_INSTRUCTION_OUTSIDE_OF_FUNCTION
        }
        ValidationError::LocalIdxOutOfRange => SPACEWASM_ERR_LOCAL_IDX_OUT_OF_RANGE,
        ValidationError::FunctionIdxOutOfRange => SPACEWASM_ERR_FUNCTION_IDX_OUT_OF_RANGE,
        ValidationError::TypeIdxOutOfRange => SPACEWASM_ERR_TYPE_IDX_OUT_OF_RANGE,
        ValidationError::FunctionTextOutOfRange => SPACEWASM_ERR_FUNCTION_TEXT_OUT_OF_RANGE,
        ValidationError::GlobalIdxOutOfRange => SPACEWASM_ERR_GLOBAL_IDX_OUT_OF_RANGE,
        ValidationError::FunctionImportNotFound => SPACEWASM_ERR_FUNCTION_IMPORT_NOT_FOUND,
        ValidationError::GlobalImportNotFound => SPACEWASM_ERR_GLOBAL_IMPORT_NOT_FOUND,
        ValidationError::MemoryImportNotFound => SPACEWASM_ERR_MEMORY_IMPORT_NOT_FOUND,
        ValidationError::TableImportNotFound => SPACEWASM_ERR_TABLE_IMPORT_NOT_FOUND,
        ValidationError::FunctionImportOutOfRange => SPACEWASM_ERR_FUNCTION_IMPORT_OUT_OF_RANGE,
        ValidationError::FunctionImportTypeMismatch => SPACEWASM_ERR_FUNCTION_IMPORT_TYPE_MISMATCH,
        ValidationError::GlobalNotMutable => SPACEWASM_ERR_GLOBAL_NOT_MUTABLE,
        ValidationError::GlobalImportTypeMismatch => SPACEWASM_ERR_GLOBAL_IMPORT_TYPE_MISMATCH,
        ValidationError::MemoryImportTypeMismatch => SPACEWASM_ERR_MEMORY_IMPORT_TYPE_MISMATCH,
        ValidationError::TableImportTypeMismatch => SPACEWASM_ERR_TABLE_IMPORT_TYPE_MISMATCH,
        ValidationError::TableImportIncompatibleSize => {
            SPACEWASM_ERR_TABLE_IMPORT_INCOMPATIBLE_SIZE
        }
        ValidationError::TableRefNotUnique => SPACEWASM_ERR_TABLE_REF_NOT_UNIQUE,
        ValidationError::FunctionParametersTooLarge => SPACEWASM_ERR_FUNCTION_PARAMETERS_TOO_LARGE,
        ValidationError::FunctionReturnsTooLarge => SPACEWASM_ERR_FUNCTION_RETURNS_TOO_LARGE,
        ValidationError::TooManyLocals => SPACEWASM_ERR_TOO_MANY_LOCALS,
        ValidationError::InvalidConstInstruction => SPACEWASM_ERR_INVALID_CONST_INSTRUCTION,
        ValidationError::GlobalTypeMismatch => SPACEWASM_ERR_GLOBAL_TYPE_MISMATCH,
        ValidationError::AlignmentLargerThanType => SPACEWASM_ERR_ALIGNMENT_LARGER_THAN_TYPE,
        ValidationError::InvalidStartFunctionSignature => {
            SPACEWASM_ERR_INVALID_START_FUNCTION_SIGNATURE
        }
        ValidationError::InvalidHostStartFunction => SPACEWASM_ERR_INVALID_HOST_START_FUNCTION,
        ValidationError::InvalidConstantExpr(ce) => constant_expr_status(ce),
        ValidationError::GuestMemoryAllocationFailure => SPACEWASM_ERR_GUEST_MEMORY_ALLOC_FAILED,
        ValidationError::AllocError(ae) => alloc_status(ae.clone()),
        ValidationError::MemoryError(me) => memory_status(me.clone()),
        ValidationError::ReaderError(_) => SPACEWASM_ERR_READER_ERROR,
    }
}

pub fn constant_expr_status(e: &ConstantExprError) -> spacewasm_status_t {
    match e {
        ConstantExprError::InvalidConstantInstruction => SPACEWASM_ERR_INVALID_CONST_INSTRUCTION,
        ConstantExprError::AlreadyHasValue => SPACEWASM_ERR_CONST_ALREADY_HAS_VALUE,
        ConstantExprError::NoValue => SPACEWASM_ERR_CONST_NO_VALUE,
        ConstantExprError::InvalidGlobal => SPACEWASM_ERR_CONST_INVALID_GLOBAL,
    }
}

pub fn parse_status(e: &ParseError) -> spacewasm_status_t {
    validation_status(&e.err.err)
}

pub fn host_name_status(_e: HostNameError) -> spacewasm_status_t {
    SPACEWASM_ERR_NAME_TOO_LONG
}

pub fn host_val_list_status(e: HostFunctionError) -> spacewasm_status_t {
    match e {
        HostFunctionError::ValListInvalidItem => SPACEWASM_ERR_BAD_ARG,
        HostFunctionError::ParameterListTooLong => SPACEWASM_ERR_FUNCTION_PARAMETERS_TOO_LARGE,
        HostFunctionError::MultiReturnNotAllowed => SPACEWASM_ERR_FUNCTION_RETURNS_TOO_LARGE,
        HostFunctionError::AllocError(ae) => alloc_status(ae),
    }
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
    }
}
