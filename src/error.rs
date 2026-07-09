use crate::MemoryError;
use crate::SectionKind;
use crate::alloc::AllocError;
use crate::constant::ConstantExprError;

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
    MultipleTables,
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
    TableImportNotFound,
    FunctionImportOutOfRange,
    FunctionImportTypeMismatch,
    GlobalIsNotMutable,
    GlobalImportTypeMismatch,
    MemoryImportTypeMismatch,
    TableImportTypeMismatch,
    TableImportIncompatibleSize,
    FunctionParametersTooLarge,
    FunctionReturnsTooLarge,
    TooManyLocals,
    InvalidConstInstruction,
    GlobalTypeMismatch,
    AlignmentLargerThanType,
    InvalidStartFunctionSignature,
    InvalidConstantExpr(ConstantExprError),
    AllocError(AllocError),
    MemoryError(MemoryError),
    ReaderError(u8),
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

impl ValidationError {
    pub fn with_section(self, section: SectionKind) -> SectionDecodeError {
        SectionDecodeError::new_with_section(section, self)
    }
}
