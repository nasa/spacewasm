use crate::alloc::AllocError;
use crate::constant::ConstantExprError;
use crate::SectionKind;
use crate::{MemoryError, ReaderError};

#[derive(Debug, Clone)]
pub enum Error {
    Parse(ParseError),
    Memory(MemoryError),
}

#[derive(Debug, Clone)]
pub struct ParseError {
    pub offset: u32,
    pub err: SectionDecodeError,
}

impl ParseError {
    pub fn new(offset: u32, err: SectionDecodeError) -> Self {
        Self { offset, err }
    }
}

#[derive(Debug, Clone)]
pub struct SectionDecodeError {
    pub section: Option<SectionKind>,
    pub err: ValidationError,
}

impl SectionDecodeError {
    pub fn new_with_section(section: SectionKind, err: ValidationError) -> SectionDecodeError {
        SectionDecodeError {
            section: Some(section),
            err,
        }
    }

    pub fn new(err: ValidationError) -> SectionDecodeError {
        SectionDecodeError { section: None, err }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    Eof,
    MalformedInteger,
    I33IsNegative,
    MalformedMagic,
    MalformedVersion,
    MalformedUtf8,
    DuplicateModuleName,
    DuplicateExportName,
    MalformedSectionId(u8),
    MalformedValueType(u8),
    MalformedFunction(u8),
    MalformedLimit(u8),
    MalformedElemType(u8),
    MalformedSectionSize,
    ExpectedConstOrVar(u8),
    MalformedImportExportDesc(u8),
    InvalidSectionOrdering(SectionKind, SectionKind),
    DuplicateSection(SectionKind),
    InvalidMaxLimit,
    ExpectedTerminal(u8),
    InvalidOpcode(u8),
    MalformedCodeSize,
    InvalidCodeSectionFunctionCount,
    VecTooLong,
    IdxTooLarge,
    ModuleIdxTooLarge,
    MemoryTooLarge,
    MemoryImportTooLarge,
    MemAlignTooLarge,
    ControlFlowTooDeep,
    StackUnderflow,
    StackTooLarge,
    LabelStackJumpTooDeep,
    LabelJumpTooLarge,
    TypeMismatch,
    BlockResultTypeMismatch,
    FunctionResultTypeMismatch,
    IllegalMemoryGrow,
    InvalidElementOffset,
    InvalidElementOutOfBounds,
    InvalidTableIndex,
    TableNotDefined,
    InvalidElementCount,
    InvalidMemIndex,
    MemoryNotDefined,
    InvalidMemOffsetType,
    InvalidNegativeMemOffset,
    InvalidMemOffset,
    InvalidLabelIndex,
    InvalidElseBlock,
    InvalidEndBlock,
    MultipleMemories,
    PossibleBackpatchCycle,
    PageFault,
    InstructionOutsideOfFunction,
    LocalIdxOutOfRange,
    FunctionIdxOutOfRange,
    TypeIdxOutOfRange,
    FunctionTextOutOfRange,
    GlobalIdxOutOfRange,
    FunctionImportNotFound,
    GlobalImportNotFound,
    MemoryImportNotFound,
    FunctionImportOutOfRange,
    FunctionImportTypeMismatch,
    GlobalIsNotMutable,
    GlobalImportTypeMismatch,
    MemoryImportTypeMismatch,
    FunctionParametersTooLarge,
    FunctionReturnsTooLarge,
    TooManyLocals,
    InvalidConstInstruction,
    BrTableHasTooManyCases,
    GlobalTypeMismatch,
    AlignmentLargerThanType,
    InvalidStartFunctionSignature,
    TableImportsNotSupportedYet, // TODO(tumbar) Implement dynamic linking
    FunctionCallsAcrossModuleNotSupportedYet, // TODO(tumbar) Implement module context isolation
    GlobalsAcrossModuleNotSupportedYet, // TODO(tumbar) Implement module context isolation
    InvalidConstantExpr(ConstantExprError),
    AllocError(AllocError),
    MemoryError(MemoryError),
    ReaderError(ReaderError),
}

impl From<AllocError> for ValidationError {
    fn from(value: AllocError) -> Self {
        ValidationError::AllocError(value)
    }
}

impl From<MemoryError> for ValidationError {
    fn from(value: MemoryError) -> Self {
        ValidationError::MemoryError(value)
    }
}

impl From<ConstantExprError> for ValidationError {
    fn from(value: ConstantExprError) -> Self {
        ValidationError::InvalidConstantExpr(value)
    }
}

impl From<AllocError> for SectionDecodeError {
    fn from(value: AllocError) -> Self {
        Self::new(ValidationError::AllocError(value))
    }
}

impl From<ValidationError> for SectionDecodeError {
    fn from(value: ValidationError) -> Self {
        Self::new(value)
    }
}

impl From<ParseError> for Error {
    fn from(value: ParseError) -> Self {
        Error::Parse(value)
    }
}

impl From<AllocError> for Error {
    fn from(value: AllocError) -> Self {
        Self::Memory(value.into())
    }
}

impl From<MemoryError> for Error {
    fn from(value: MemoryError) -> Self {
        Self::Memory(value)
    }
}

impl ValidationError {
    pub fn with_section(self, section: SectionKind) -> SectionDecodeError {
        SectionDecodeError::new_with_section(section, self)
    }
}
